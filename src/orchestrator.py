"""Drives the Gauntlet loop (threads for parallel lanes).

The orchestrator is a deterministic state machine; no LLM sits in the
control loop. Phases (DESIGN.md "State machine"):

INIT -> PLAN -> [checkpoint: plan] -> IMPLEMENT(wave=0) -> INSPECT
-> INTEGRATE -> GATES -> REVIEW -> JUDGE
-> actionable: PLAN_FIX -> IMPLEMENT(wave+=1) -> ... -> JUDGE
-> no actionable: [checkpoint: deliver] -> DELIVER -> READY
Terminals: READY | READY_NO_CHANGE | BLOCKED.
"""
from __future__ import annotations

import json
import shutil
import threading
import time
from pathlib import Path

from src import capsules, config as config_mod, gates as gates_mod
from src import mission as mission_mod
from src import statemachine, verdicts, worktrees
from src.adapters import ADAPTER_CLASSES, EchoAdapter
from src.adapters.base import FailureKind
from src.adapters.human import read_decision
from src.fallback import (AuthAbort, ChainExhausted, HarnessHealth,
                          execute_chain)
from src.report import Report


class GauntletError(Exception):
    pass


_WRITE_ROLES = {"implementer", "fixer"}


class Orchestrator:
    def __init__(self, *, tool_dir, mission_path=None, resume_dir=None,
                 config_path=None, auto: bool = False, dry_run: bool = False,
                 log=print):
        self.tool_dir = Path(tool_dir)
        self.auto = auto
        self.dry_run = dry_run
        self.log = log
        self._state_lock = threading.Lock()
        self.git = worktrees.Git(dry_run=dry_run, log=log)
        self.report: Report | None = None

        if resume_dir:
            self.run_dir = Path(resume_dir)
            self.state = statemachine.load(self.run_dir)
            self.mission = mission_mod.load_mission(self.run_dir / "mission.md")
            self.config = config_mod.load_config(
                config_file=self.run_dir / "config.toml")
            if config_path:
                # An explicit --config overrides the stored snapshot.
                self.config = config_mod.load_config(
                    config_file=self.run_dir / "config.toml")
                extra = config_mod.load_config(config_file=config_path)
                self.config = config_mod._merge(self.config, extra)
                config_mod.validate_config(self.config)
            self.report = Report(self.run_dir / "report.md")
        else:
            self.mission = mission_mod.load_mission(mission_path)
            self.config = config_mod.load_config(
                tool_dir=self.tool_dir,
                mission_dir=Path(mission_path).parent,
                config_file=config_path)
            repo = self.mission.repos[0]
            self.state = statemachine.State(
                slug=self.mission.slug,
                repo=str(Path(repo.path).resolve()),
                target_branch=repo.target_branch,
                gates=list(repo.gates),
                auto=auto,
                dry_run=dry_run,
            )
            self.run_dir = None

        self.health = HarnessHealth(self.state.harness_health)
        self.adapters = {
            hname: ADAPTER_CLASSES[hcfg["adapter"]](hname, hcfg)
            for hname, hcfg in self.config["harnesses"].items()
        }
        self.echo = EchoAdapter("echo", {"supports_write": True})

    # ------------------------------------------------------------ plumbing

    @property
    def repo_path(self) -> Path:
        return Path(self.state.repo)

    def integration_wt(self) -> Path:
        return Path(f"{self.state.repo}-worktree-gauntlet-{self.state.run_id}")

    def lane_wt(self, lane_id: str) -> Path:
        return Path(
            f"{self.state.repo}-worktree-gauntlet-{self.state.run_id}-{lane_id}")

    def integration_branch(self) -> str:
        return f"gauntlet/{self.state.run_id}/integration"

    def lane_branch(self, lane_id: str) -> str:
        return f"gauntlet/{self.state.run_id}/{lane_id}"

    def work_wt(self) -> Path:
        """cwd for read-only roles: the integration worktree when it exists."""
        wt = self.integration_wt()
        return wt if wt.is_dir() else self.repo_path

    def _save(self) -> None:
        with self._state_lock:
            self.state.harness_health = self.health.snapshot()
            if self.state.run_dir:
                statemachine.save(self.state)

    def _transition(self, phase: str) -> None:
        self.state.phase = phase
        self._save()
        self.log(f"phase -> {phase}")

    def _blocked(self, reason: str) -> None:
        self.log(f"BLOCKED: {reason}")
        self.state.blocked_reason = reason
        self.state.phase = "BLOCKED"
        if self.report:
            self.report.section("BLOCKED", reason)
        self._save()

    def _write_capsule(self, name: str, text: str) -> Path:
        path = self.run_dir / "capsules" / f"{name}.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    # --------------------------------------------------------- role runner

    def _validate_report(self, result):
        try:
            report = verdicts.validate_report(
                verdicts.extract_block_from_file(result.output_path, "report"))
        except verdicts.VerdictError as exc:
            return FailureKind.OUTPUT_INVALID, str(exc)
        if report["partial"]:
            return (FailureKind.PARTIAL_DELIVERY,
                    "worker declared partial delivery")
        return None, ""

    def _validate_verdict(self, result):
        try:
            verdicts.validate_verdict(
                verdicts.extract_block_from_file(result.output_path, "verdict"),
                self.mission.contract_ids)
        except verdicts.VerdictError as exc:
            return FailureKind.OUTPUT_INVALID, str(exc)
        return None, ""

    def _validate_plan(self, result):
        try:
            verdicts.validate_plan(
                verdicts.extract_block_from_file(result.output_path, "plan"))
        except verdicts.VerdictError as exc:
            return FailureKind.OUTPUT_INVALID, str(exc)
        return None, ""

    def _run_role(self, role: str, capsule: Path, worktree: Path,
                  write: bool, lane_id: str | None = None):
        links = self.config["roles"][role]["chain"]
        policy = self.config["policy"]
        validators = {
            "implementer": self._validate_report,
            "fixer": self._validate_report,
            "reviewer": self._validate_verdict,
            "judge": self._validate_verdict,
            "planner": self._validate_plan,
        }

        def run_once(link, attempt):
            hname = link["harness"]
            adapter = self.adapters[hname]
            hcfg = self.config["harnesses"][hname]
            model = link.get("model") or hcfg.get("default_model")
            effort = link.get("effort")
            if role in _WRITE_ROLES:
                hard_s, idle_s = policy["lane_timeout_s"], None
            else:
                hard_s, idle_s = policy["hard_timeout_s"], policy["idle_timeout_s"]
            if self.dry_run and hname not in ("echo", "human"):
                self.log("DRY-RUN: would run: " + adapter.describe(
                    capsule=capsule, worktree=worktree, write=write,
                    model=model, effort=effort))
                adapter = self.echo
            return adapter.run(
                capsule=capsule, worktree=worktree, write=write,
                model=model, effort=effort, hard_timeout_s=hard_s,
                idle_timeout_s=idle_s, out_dir=self.run_dir / "outputs")

        return execute_chain(
            role=role, links=links, health=self.health,
            policy=self.config["fallback"], run_once=run_once,
            validate=validators.get(role), auto=self.auto,
            checkpoint=(None if self.auto else
                        (lambda msg: self._ask_human("attempts", msg))),
            log=self.log)

    # --------------------------------------------------------- checkpoints

    def _ask_human(self, name: str, context: str) -> bool:
        """Ungated director consult (used for attempts-exhaustion)."""
        if self.auto:
            return False
        capsule = self._write_capsule(f"checkpoint-{name}",
                                      capsules.checkpoint(name, context))
        try:
            outcome = self._run_role("director", capsule, self.work_wt(),
                                     write=False)
        except (ChainExhausted, AuthAbort) as exc:
            self.log(f"director consult failed: {exc}")
            return False
        decision = read_decision(outcome.result.output_path)
        self.log(f"director decision on '{name}': {decision}")
        return decision == "approve"

    def _checkpoint(self, name: str, context: str) -> bool:
        if name not in self.config["policy"].get("checkpoints", []):
            return True
        if self.auto:
            self.log(f"checkpoint '{name}': auto-approved (--auto)")
            return True
        return self._ask_human(name, context)

    # ------------------------------------------------------------- phases

    def run(self) -> int:
        handlers = {
            "INIT": self._phase_init,
            "PLAN": self._phase_plan,
            "PLAN_CHECKPOINT": self._phase_plan_checkpoint,
            "IMPLEMENT": self._phase_implement,
            "INSPECT": self._phase_inspect,
            "INTEGRATE": self._phase_integrate,
            "GATES": self._phase_gates,
            "REVIEW": self._phase_review,
            "JUDGE": self._phase_judge,
            "PLAN_FIX": self._phase_plan_fix,
            "DELIVER_CHECKPOINT": self._phase_deliver_checkpoint,
            "DELIVER": self._phase_deliver,
        }
        try:
            while self.state.phase not in statemachine.TERMINALS:
                handlers[self.state.phase]()
        except GauntletError as exc:
            self._blocked(str(exc))
        except (ChainExhausted, AuthAbort) as exc:
            self._blocked(str(exc))
        phase = self.state.phase
        reason = f" — {self.state.blocked_reason}" if phase == "BLOCKED" else ""
        self.log(f"terminal phase: {phase}{reason}")
        if self.report:
            self.report.section("TERMINAL", phase + reason)
        return 0 if phase in ("READY", "READY_NO_CHANGE") else 2

    def _phase_init(self) -> None:
        repo = self.repo_path
        if not worktrees.is_git_repo(self.git, repo):
            raise GauntletError(f"{repo} is not a git repository")
        target = self.state.target_branch
        if not worktrees.branch_exists(self.git, repo, target):
            raise GauntletError(
                f"target branch '{target}' does not exist in {repo}")
        if worktrees.staged_changes(self.git, repo):
            raise GauntletError(
                f"refusing INIT: {repo} has staged changes on the checkout; "
                "commit or stash them first")
        self.state.base_commit = worktrees.base_commit(self.git, repo, target)

        date = time.strftime("%Y%m%d")
        missions_root = repo / ".missions"
        run_id = f"{date}-{self.state.slug}"
        n = 2
        while (missions_root / run_id).exists():
            run_id = f"{date}-{self.state.slug}-{n}"
            n += 1
        self.state.run_id = run_id
        self.run_dir = missions_root / run_id
        for sub in ("capsules", "outputs", "verdicts"):
            (self.run_dir / sub).mkdir(parents=True, exist_ok=True)
        self.state.run_dir = str(self.run_dir)
        shutil.copy(self.mission.source_path, self.run_dir / "mission.md")
        (self.run_dir / "config.toml").write_text(
            config_mod.dump_toml(self.config), encoding="utf-8")
        self.report = Report(self.run_dir / "report.md",
                             title=f"Gauntlet run {run_id}")

        # Integration worktree from the target branch HEAD.
        wt = self.integration_wt()
        branch = self.integration_branch()
        worktrees.create_worktree(self.git, repo, wt, branch,
                                  self.state.base_commit)
        if str(wt) not in self.state.worktrees:
            self.state.worktrees.append(str(wt))
        if branch not in self.state.branches:
            self.state.branches.append(branch)

        self.state.lanes = [
            statemachine.LaneState(
                id=lane.id, owns=lane.owns, forbidden=lane.forbidden,
                tests=lane.tests, brief=lane.brief, addresses=lane.addresses)
            for lane in self.mission.lanes
        ]
        self.report.section(
            "INIT",
            f"repo: {repo}\nbase: {self.state.base_commit}\n"
            f"target branch: {target}\n"
            f"pre-written lanes: {len(self.state.lanes)}")
        self._transition("PLAN")

    def _run_planner(self, *, groups=None, complaint=None):
        capsule = self._write_capsule(
            f"planner-w{self.state.wave}",
            capsules.planner(self.mission, run_id=self.state.run_id,
                             groups=groups, complaint=complaint))
        outcome = self._run_role("planner", capsule, self.work_wt(),
                                 write=False)
        data = verdicts.extract_block_from_file(outcome.result.output_path,
                                                "plan")
        planned = verdicts.validate_plan(data)
        return [
            statemachine.LaneState(
                id=p["id"], owns=p["owns"], forbidden=p["forbidden"],
                tests=p["tests"], brief=p["brief"], addresses=p["addresses"])
            for p in planned
        ]

    def _check_overlaps(self, lanes) -> list:
        files = worktrees.tracked_files(self.git, self.repo_path)
        return worktrees.find_overlaps(lanes, files)

    def _phase_plan(self) -> None:
        if self.state.lanes:
            overlaps = self._check_overlaps(self.state.lanes)
            if overlaps:
                raise GauntletError(
                    f"config error: pre-written lane owns globs overlap: "
                    f"{overlaps}")
        else:
            lanes = self._run_planner()
            overlaps = self._check_overlaps(lanes)
            if overlaps:
                # Back to the planner once, then the human director.
                lanes = self._run_planner(
                    complaint=f"lane owns globs overlapped: {overlaps}")
                overlaps = self._check_overlaps(lanes)
                if overlaps and not self._ask_human(
                        "plan",
                        f"planner produced overlapping lanes twice: "
                        f"{overlaps}\napprove to proceed anyway"):
                    raise GauntletError(
                        f"planner could not produce orthogonal lanes: "
                        f"{overlaps}")
            self.state.lanes = lanes
        summary = "\n".join(
            f"- {lane.id}: owns={lane.owns} forbidden={lane.forbidden}"
            for lane in self.state.lanes)
        self.report.section("PLAN", summary)
        self._save()
        self._transition("PLAN_CHECKPOINT")

    def _phase_plan_checkpoint(self) -> None:
        summary = "\n".join(
            f"- {lane.id}: owns={lane.owns}" for lane in self.state.lanes)
        if not self._checkpoint("plan", f"Planned lanes:\n{summary}"):
            raise GauntletError("plan checkpoint rejected by director")
        self._transition("IMPLEMENT")

    def _phase_implement(self) -> None:
        role = "implementer" if self.state.wave == 0 else "fixer"
        todo = [lane for lane in self.state.lanes
                if lane.status in statemachine.LANE_ACTIVE]
        if not todo:
            self._transition("INSPECT")
            return
        for lane in todo:  # worktree creation stays serial and idempotent
            wt = self.lane_wt(lane.id)
            if not wt.exists():
                worktrees.create_worktree(self.git, self.repo_path, wt,
                                          self.lane_branch(lane.id),
                                          self.state.base_commit)
            if str(wt) not in self.state.worktrees:
                self.state.worktrees.append(str(wt))
            if self.lane_branch(lane.id) not in self.state.branches:
                self.state.branches.append(self.lane_branch(lane.id))
        self._save()

        def worker(lane):
            capsule = self._write_capsule(
                f"{role}-{lane.id}-w{self.state.wave}",
                capsules.implementer(self.mission, lane,
                                     wave=self.state.wave,
                                     run_id=self.state.run_id, role=role))
            try:
                outcome = self._run_role(role, capsule, self.lane_wt(lane.id),
                                         write=True, lane_id=lane.id)
                lane.status = "done"
                lane.detail = ""
                try:  # validation already passed in _run_role; keep claims
                    report = verdicts.validate_report(
                        verdicts.extract_block_from_file(
                            outcome.result.output_path, "report"))
                    lane.claimed = list(report.get("files_changed", []))
                except verdicts.VerdictError:
                    lane.claimed = []
            except (ChainExhausted, AuthAbort) as exc:
                lane.status = "failed"
                lane.detail = str(exc)
            except Exception as exc:  # defensive: never lose a lane thread
                lane.status = "failed"
                lane.detail = f"unexpected: {exc}"
            self._save()

        # Snapshot the main checkout before lanes run: a worker escaping its
        # worktree shows up here (INSPECT compares).
        self._main_before = worktrees.checkout_status(self.git, self.repo_path)

        threads = [threading.Thread(target=worker, args=(lane,),
                                    name=f"lane-{lane.id}") for lane in todo]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()

        failed = [lane for lane in todo if lane.status == "failed"]
        if failed:
            raise GauntletError(
                "lane(s) failed: "
                + "; ".join(f"{lane.id}: {lane.detail}" for lane in failed))
        self.report.section(
            f"IMPLEMENT wave {self.state.wave}",
            "\n".join(f"- {lane.id}: done ({role})" for lane in todo))
        self._transition("INSPECT")

    def _phase_inspect(self) -> None:
        drift = worktrees.checkout_drift(
            getattr(self, "_main_before", []),
            worktrees.checkout_status(self.git, self.repo_path))
        if drift:
            raise GauntletError(
                "SAFETY: main checkout modified while lanes ran "
                "(worker escaped its worktree): " + ", ".join(drift))
        rejected = []
        for lane in self.state.lanes:
            if lane.status != "done":
                continue
            lane.changed = worktrees.lane_changed_files(
                self.git, self.lane_wt(lane.id), self.state.base_commit)
            violations = worktrees.check_lane_diff(
                lane.changed, lane.owns, lane.forbidden)
            violations += worktrees.check_claimed_vs_diff(
                lane.claimed, lane.changed)
            if violations:
                lane.status = "rejected"  # automatic, no debate
                lane.detail = "; ".join(violations)
                rejected.append(lane)
            else:
                lane.detail = f"{len(lane.changed)} file(s) changed"
        self._save()
        self.report.section(
            "INSPECT",
            "\n".join(f"- {lane.id}: {lane.status} {lane.detail}"
                      for lane in self.state.lanes))
        if rejected:
            raise GauntletError(
                "INSPECT rejected lane(s): "
                + "; ".join(f"{lane.id}: {lane.detail}" for lane in rejected))
        self._transition("INTEGRATE")

    def _phase_integrate(self) -> None:
        integrated = []
        for lane in self.state.lanes:
            if lane.status != "done":
                continue
            if lane.changed:
                worktrees.commit_all(
                    self.git, self.lane_wt(lane.id),
                    f"gauntlet({self.state.run_id}): lane {lane.id} "
                    f"wave {self.state.wave}")
                try:
                    worktrees.merge_branch(self.git, self.integration_wt(),
                                           self.lane_branch(lane.id))
                except worktrees.GitError as exc:
                    raise GauntletError(
                        f"merge conflict integrating lane {lane.id}: {exc}")
                self.state.integrated_changes = True
                integrated.append(lane.id)
                capsule = (self.run_dir / "capsules"
                           / f"{'implementer' if self.state.wave == 0 else 'fixer'}"
                             f"-{lane.id}-w{self.state.wave}.md")
                capsule.unlink(missing_ok=True)  # capsules are ephemeral
            lane.status = "integrated"
            self._save()
        self.report.section(
            "INTEGRATE",
            f"merged lanes: {', '.join(integrated) or '(no changes)'}")
        self._transition("GATES")

    def _phase_gates(self) -> None:
        results = gates_mod.run_gates(
            self.state.gates, cwd=self.integration_wt(),
            out_dir=self.run_dir / "outputs", dry_run=self.dry_run,
            log=self.log, timeout_s=self.config["policy"]["lane_timeout_s"])
        failed = [r for r in results if not r.ok]
        self.report.section(
            "GATES",
            "\n".join(f"- {'ok' if r.ok else 'FAIL'}: {r.command}"
                      for r in results) or "(no gates)")
        if failed:
            raise GauntletError(
                "required gate(s) failed: "
                + "; ".join(f"{r.command} ({r.detail})" for r in failed))
        self._transition("REVIEW")

    def _phase_review(self) -> None:
        diff_path = self.run_dir / "reviews" / f"diff-w{self.state.wave}.patch"
        diff_path.parent.mkdir(parents=True, exist_ok=True)
        wt = self.integration_wt()
        if wt.is_dir():
            diff = self.git.run(["diff", self.state.base_commit], cwd=wt,
                                mutating=False) or "(empty diff)\n"
        else:  # dry-run: the integration worktree was never created
            diff = "(integration worktree unavailable — dry-run)\n"
        diff_path.write_text(diff, encoding="utf-8")
        capsule = self._write_capsule(
            f"reviewer-w{self.state.wave}",
            capsules.reviewer(self.mission, wave=self.state.wave,
                              run_id=self.state.run_id,
                              diff_path=str(diff_path)))
        outcome = self._run_role("reviewer", capsule, self.work_wt(),
                                 write=False)
        data = verdicts.extract_block_from_file(outcome.result.output_path,
                                                "verdict")
        path = self.run_dir / "verdicts" / f"review-w{self.state.wave}.json"
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        self.state.reviews.append(str(path))
        self.report.section(
            f"REVIEW wave {self.state.wave}",
            f"groups: {len(data.get('groups', []))} "
            f"(harness: {outcome.harness}, attempts: {outcome.attempts})")
        self._transition("JUDGE")

    def _phase_judge(self) -> None:
        review_json = Path(self.state.reviews[-1]).read_text(encoding="utf-8")
        capsule = self._write_capsule(
            f"judge-w{self.state.wave}",
            capsules.judge(self.mission, wave=self.state.wave,
                           run_id=self.state.run_id, review_json=review_json))
        outcome = self._run_role("judge", capsule, self.work_wt(),
                                 write=False)
        data = verdicts.extract_block_from_file(outcome.result.output_path,
                                                "verdict")
        groups = verdicts.validate_verdict(data, self.mission.contract_ids)
        path = self.run_dir / "verdicts" / f"judgment-w{self.state.wave}.json"
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        self.state.judgments.append(str(path))
        self._save()
        actionable = [g for g in groups if g.actionable]
        self.report.section(
            f"JUDGE wave {self.state.wave}",
            f"groups: {len(groups)}, actionable: {len(actionable)}")
        if actionable:
            if self.state.wave >= self.config["policy"]["max_fix_waves"]:
                raise GauntletError(
                    f"architecture not converging: still {len(actionable)} "
                    f"actionable group(s) after {self.state.wave} fix wave(s)")
            self.state.wave += 1
            self._pending_groups = [
                {"root_cause": g.root_cause, "verdict": g.verdict,
                 "fix": g.fix} for g in actionable]
            self._transition("PLAN_FIX")
        else:
            self._transition("DELIVER_CHECKPOINT")

    def _phase_plan_fix(self) -> None:
        groups = getattr(self, "_pending_groups", [])
        # Rebuild lightweight group objects for capsule rendering.
        groups = [verdicts.ClaimGroup(**g) for g in groups] if groups else None
        if groups is None and self.state.judgments:
            data = json.loads(Path(self.state.judgments[-1]).read_text())
            all_groups = verdicts.validate_verdict(data,
                                                   self.mission.contract_ids)
            groups = [g for g in all_groups if g.actionable]
        lanes = self._run_planner(groups=groups)
        overlaps = self._check_overlaps(lanes)
        if overlaps:
            lanes = self._run_planner(
                groups=groups,
                complaint=f"lane owns globs overlapped: {overlaps}")
            overlaps = self._check_overlaps(lanes)
            if overlaps and not self._ask_human(
                    "plan-fix",
                    f"fix-wave lanes overlap: {overlaps}\n"
                    "approve to proceed anyway"):
                raise GauntletError(
                    f"fix-wave planner could not produce orthogonal lanes: "
                    f"{overlaps}")
        self.state.lanes = lanes
        self.report.section(
            f"PLAN_FIX wave {self.state.wave}",
            "\n".join(f"- {lane.id}: owns={lane.owns} "
                      f"addresses={lane.addresses}" for lane in lanes))
        self._save()
        self._transition("IMPLEMENT")

    def _phase_deliver_checkpoint(self) -> None:
        context = (
            f"run: {self.state.run_id}\nwave: {self.state.wave}\n"
            f"changes integrated: {self.state.integrated_changes}\n"
            f"judgment: no actionable groups\n"
            "approve to deliver into the target branch")
        if not self._checkpoint("deliver", context):
            raise GauntletError("deliver checkpoint rejected by director")
        self._transition("DELIVER")

    def _phase_deliver(self) -> None:
        repo = self.repo_path
        target = self.state.target_branch
        branch = self.integration_branch()
        head = worktrees.rev_parse(self.git, repo, branch)
        if (not self.state.integrated_changes or head is None
                or head == self.state.base_commit):
            final = "READY_NO_CHANGE"
        else:
            worktrees.rebase_onto(self.git, self.integration_wt(), target)
            results = gates_mod.run_gates(
                self.state.gates, cwd=self.integration_wt(),
                out_dir=self.run_dir / "outputs", dry_run=self.dry_run,
                log=self.log,
                timeout_s=self.config["policy"]["lane_timeout_s"])
            failed = [r for r in results if not r.ok]
            if failed:
                raise GauntletError(
                    "gate(s) failed after rebase: "
                    + "; ".join(r.command for r in failed))
            if worktrees.current_branch(self.git, repo) != target:
                raise GauntletError(
                    f"main checkout is not on '{target}'; refusing "
                    "fast-forward merge")
            worktrees.ff_merge(self.git, repo, branch)
            final = "READY"
        self._cleanup()
        self.report.section("DELIVER", final)
        self._transition(final)

    def _cleanup(self) -> None:
        for wt in self.state.worktrees:
            if Path(wt).is_dir():
                try:
                    worktrees.remove_worktree(self.git, self.repo_path, wt)
                except worktrees.GitError as exc:
                    self.log(f"warning: cleanup of {wt} failed: {exc}")
            elif self.dry_run:
                worktrees.remove_worktree(self.git, self.repo_path, wt)
        for branch in self.state.branches:
            try:
                worktrees.delete_branch(self.git, self.repo_path, branch)
            except worktrees.GitError as exc:
                self.log(f"warning: cleanup of branch {branch} failed: {exc}")

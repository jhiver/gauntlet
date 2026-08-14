"""Drives the Gauntlet loop (threads for parallel lanes).

The orchestrator is a deterministic state machine; no LLM sits in the
control loop. Phases (DESIGN.md "State machine"):

INIT -> PLAN -> [checkpoint: plan] -> IMPLEMENT(wave=0) -> INSPECT
-> INTEGRATE -> GATES -> REVIEW -> JUDGE
-> blocking groups: PLAN_FIX -> IMPLEMENT(wave+=1) -> ... -> JUDGE
-> none blocking: POLISH -> [checkpoint: deliver] -> DELIVER -> READY
Terminals: READY | READY_NO_CHANGE | BLOCKED* (a blocked terminal always
means "a human decision is required", and its kind says which one).
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
    """A blocking condition. `kind` selects the blocked terminal."""

    def __init__(self, message: str, kind: str = "BLOCKED"):
        super().__init__(message)
        self.kind = kind


_WRITE_ROLES = {"implementer", "fixer"}


class Orchestrator:
    def __init__(self, *, tool_dir, mission_path=None, resume_dir=None,
                 config_path=None, auto: bool = True, dry_run: bool = False,
                 profile: str | None = None, replan: bool = False,
                 depth: int = 0, max_depth: int = 2,
                 log=print):
        self.tool_dir = Path(tool_dir)
        self.auto = auto
        self.dry_run = dry_run
        self.depth = depth
        self.max_depth = max_depth
        self.log = log
        self._state_lock = threading.Lock()
        self.git = worktrees.Git(dry_run=dry_run, log=log)
        self.report: Report | None = None
        self.profile_info = None

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
            from src.autoroute import analyze_mission
            self.mission = mission_mod.load_mission(mission_path)
            if replan:
                self.mission.lanes = []
            self.profile_info = analyze_mission(self.mission)
            self.config = config_mod.load_config(
                tool_dir=self.tool_dir,
                mission_dir=Path(mission_path).parent,
                config_file=config_path)
            
            # Super-Auto: apply intelligent Pareto routing if requested or if standard defaults are loaded (non-test echo)
            is_echo_test = all(
                all(link.get("harness") == "echo" for link in self.config.get("roles", {}).get(r, {}).get("chain", []))
                for r in ("implementer", "fixer", "reviewer", "judge", "planner")
                if r in self.config.get("roles", {})
            )
            has_custom_config = config_path is not None or (Path(mission_path).parent / "gauntlet.toml").is_file()
            if (profile or not has_custom_config) and not is_echo_test:
                self.config["roles"] = config_mod._merge(self.config["roles"], self.profile_info.roles)
            
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
        from src.ui import default_ui
        default_ui.phase_card(phase, wave=self.state.wave)
        self.log(f"phase -> {phase}")

    _NEXT_STEP = {
        "BLOCKED_CONVERGENCE": (
            "fix waves stopped reducing the defect count: re-scope the "
            "contract, or fix the remaining groups by hand and --resume."),
        "BLOCKED_ARCHITECTURE": (
            "the judge accepted a REDESIGN group — no proportionate local fix "
            "exists. A human redesign decision is required."),
        "BLOCKED_GATE": (
            "a required gate failed on the candidate; see outputs/gate-*.log, "
            "fix the cause, then --resume."),
        "BLOCKED_HARNESS": (
            "no harness in the role chain could deliver (quota, auth, or "
            "timeout). Restore access or edit the chain, then --resume."),
        "BLOCKED": (
            "the run hit a condition it must not decide alone; see the reason "
            "above."),
    }

    def _diagnosis(self) -> str:
        """Everything a human needs to pick the recovery path, in one read."""
        lines = [
            "### Diagnosis",
            "",
            f"- phase at failure: {self.state.blocked_phase}",
            f"- wave: {self.state.wave} of "
            f"{self.config['policy']['max_total_waves']} max",
        ]
        if self.state.blocking_history:
            lines.append("- blocking-group trajectory: "
                         + " → ".join(str(n)
                                      for n in self.state.blocking_history))
        for group in self._last_judgment_groups():
            if group.blocking:
                lines.append(
                    f"- remaining: {group.root_cause} "
                    f"[{group.verdict}, {group.defect_class}]"
                    + (f" owns={group.owns}" if group.owns else ""))
        if self.state.gates:
            lines.append("- gates: " + "; ".join(self.state.gates))
        if self.health.snapshot():
            lines.append(f"- harness health: {self.health.snapshot()}")
        if self.state.branches:
            lines.append("- candidate preserved on branch "
                         f"{self.integration_branch()} (worktrees kept)")
        lines += ["", "Next: " + self._NEXT_STEP.get(
            self.state.blocked_kind, self._NEXT_STEP["BLOCKED"])]
        return "\n".join(lines)

    def _blocked(self, reason: str, kind: str = "BLOCKED") -> None:
        self.log(f"{kind}: {reason}")
        from src.ui import default_ui
        default_ui.error(f"{kind}: {reason}")
        self.state.blocked_reason = reason
        self.state.blocked_kind = kind
        self.state.blocked_phase = self.state.phase
        self.state.phase = kind
        if self.report:
            self.report.section(kind, f"{reason}\n\n{self._diagnosis()}")
        self._save()

    def _write_capsule(self, name: str, text: str) -> Path:
        path = self.run_dir / "capsules" / f"{name}.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    # --------------------------------------------------------- judgment log

    @staticmethod
    def _slot(items: list, index: int, value) -> list:
        """Wave-indexed write: re-entering a phase (--resume) overwrites its
        own entry instead of appending a phantom round."""
        return items[:index] + [value]

    def _judgment_groups(self, path) -> list:
        try:
            data = json.loads(Path(path).read_text(encoding="utf-8"))
            return verdicts.validate_verdict(data, self.mission.contract_ids)
        except (OSError, json.JSONDecodeError, verdicts.VerdictError) as exc:
            self.log(f"warning: unreadable judgment {path}: {exc}")
            return []

    def _last_judgment_groups(self) -> list:
        if not self.state.judgments:
            return []
        return self._judgment_groups(self.state.judgments[-1])

    def _prior_findings(self) -> tuple[list[str], list[str], list[str]]:
        """Root causes of the earlier rounds: fixed, deferred, dismissed.

        Handed to the next reviewer so a fresh review verifies the previous
        fixes instead of re-sampling the defect distribution from zero.
        """
        fixed: list[str] = []
        deferred: list[str] = []
        dismissed: list[str] = []
        for wave, path in enumerate(self.state.judgments):
            for group in self._judgment_groups(path):
                entry = f"[wave {wave}] {group.root_cause}"
                if group.blocking:
                    fixed.append(entry)
                elif group.polish:
                    deferred.append(f"{entry} [{group.defect_class}]")
                else:
                    reason = f" — {group.verdict}"
                    if group.fix:
                        reason += f": {group.fix}"
                    dismissed.append(entry + reason)
        return fixed, deferred, dismissed

    # --------------------------------------------------------- role runner

    def _validate_report(self, result):
        try:
            report = verdicts.validate_report(
                verdicts.extract_block_from_file(result.output_path, "report"))
            if report.get("partial", False):
                return (FailureKind.PARTIAL_DELIVERY,
                        "worker declared partial delivery")
        except (verdicts.VerdictError, OSError) as exc:
            return FailureKind.OUTPUT_INVALID, str(exc)
        return None, ""

    def _validate_verdict(self, result):
        try:
            verdicts.validate_verdict(
                verdicts.extract_block_from_file(result.output_path, "verdict"),
                self.mission.contract_ids)
        except (verdicts.VerdictError, OSError) as exc:
            return FailureKind.OUTPUT_INVALID, str(exc)
        return None, ""

    def _validate_plan(self, result):
        try:
            text = Path(result.output_path).read_text(encoding="utf-8", errors="replace")
            verdicts.extract_planner_result(text, self.mission.contract_ids)
        except (verdicts.VerdictError, OSError) as exc:
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
                idle_timeout_s=idle_s, out_dir=self.run_dir / "outputs",
                role=role, lane_id=lane_id)

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
            "STAGES": self._phase_stages,
            "IMPLEMENT": self._phase_implement,
            "INSPECT": self._phase_inspect,
            "INTEGRATE": self._phase_integrate,
            "GATES": self._phase_gates,
            "REVIEW": self._phase_review,
            "JUDGE": self._phase_judge,
            "PLAN_FIX": self._phase_plan_fix,
            "POLISH": self._phase_polish,
            "DELIVER_CHECKPOINT": self._phase_deliver_checkpoint,
            "DELIVER": self._phase_deliver,
        }
        try:
            while self.state.phase not in statemachine.TERMINALS:
                handlers[self.state.phase]()
        except GauntletError as exc:
            self._blocked(str(exc), exc.kind)
        except (ChainExhausted, AuthAbort) as exc:
            self._blocked(str(exc), "BLOCKED_HARNESS")
        phase = self.state.phase
        reason = (f" — {self.state.blocked_reason}"
                  if phase in statemachine.BLOCKED_TERMINALS else "")
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

        from src.ui import default_ui
        meta = {
            "Repository": str(repo),
            "Target Branch": target,
            "Base Commit": str(self.state.base_commit)[:12],
            "Gates Suite": f"{len(self.state.gates)} gate(s)",
            "Lanes Planned": f"{len(self.state.lanes)} lane(s)",
        }
        if getattr(self, "profile_info", None):
            tier_title = self.profile_info.tier.upper()
            reasons_str = "; ".join(self.profile_info.reasons[:2])
            meta["Pareto Profile"] = f"⚡ {tier_title} ({reasons_str})"
        
        default_ui.banner(
            title=f"GAUNTLET MISSION • {self.state.slug}",
            subtitle="Autonomous Multi-Agent Pareto State Machine",
            meta=meta
        )
        self._transition("PLAN")

    def _run_planner(self, *, groups=None, complaint=None):
        capsule = self._write_capsule(
            f"planner-w{self.state.wave}",
            capsules.planner(self.mission, run_id=self.state.run_id,
                             groups=groups, complaint=complaint))
        outcome = self._run_role("planner", capsule, self.work_wt(),
                                 write=False)
        text = Path(outcome.result.output_path).read_text(encoding="utf-8", errors="replace")
        kind, data = verdicts.extract_planner_result(text, self.mission.contract_ids)
        if kind == "stages":
            return "stages", data
        lanes = [
            statemachine.LaneState(
                id=p["id"], owns=p["owns"], forbidden=p["forbidden"],
                tests=p["tests"], brief=p["brief"], addresses=p["addresses"])
            for p in data
        ]
        return "lanes", lanes

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
            summary = "\n".join(
                f"- {lane.id}: owns={lane.owns} forbidden={lane.forbidden}"
                for lane in self.state.lanes)
            self.report.section("PLAN", f"(pre-written lanes)\n{summary}")
            self._save()
            self._transition("IMPLEMENT")
            return

        kind, data = self._run_planner()
        if kind == "stages" and self.depth < self.max_depth:
            self.state.stages = data
            summary = "\n".join(
                f"- Stage {i+1} [{s['slug']}]: {s.get('brief', '')} (owns: {s.get('owns', [])})"
                for i, s in enumerate(data))
            self.report.section("PLAN", f"(sequential stages)\n{summary}")
            self._save()
            from src.ui import default_ui
            default_ui.step("PLAN", f"Decomposed into {len(data)} sequential stage(s)")
            for i, s in enumerate(data, 1):
                default_ui.step(f"STAGE {i}/{len(data)}", f"{s['slug']}: {s.get('brief', '')}")
            self._transition("STAGES")
            return

        lanes = data
        overlaps = self._check_overlaps(lanes)
        if overlaps:
            # Back to the planner once, then the human director.
            kind, lanes = self._run_planner(
                complaint=f"lane owns globs overlapped: {overlaps}")
            if kind == "lanes":
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

    def _phase_stages(self) -> None:
        """Execute composite mission sub-stages sequentially."""
        from src.ui import default_ui
        from src.mission import create_stage_mission
        stages = self.state.stages
        total = len(stages)
        
        for i, stage in enumerate(stages, 1):
            slug = stage["slug"]
            brief = stage.get("brief", "")
            default_ui.stage_header(i, total, slug, brief)
            
            sub_mission_path = Path(self.state.run_dir) / "sub-missions" / f"{i:02d}-{slug}.input.md"
            sub_mission = create_stage_mission(
                self.mission, stage, target_branch=self.integration_branch(),
                path=sub_mission_path)
            
            # Check if this sub-mission was already started or completed
            run_date = self.state.run_id.split("-")[0] if self.state.run_id and len(self.state.run_id.split("-")[0]) == 8 and self.state.run_id.split("-")[0].isdigit() else time.strftime("%Y%m%d")
            missions_root = Path(self.mission.repos[0].path) / ".missions"
            expected_run_dir = missions_root / f"{run_date}-{sub_mission.slug}"
            if not expected_run_dir.exists():
                today_dir = missions_root / f"{time.strftime('%Y%m%d')}-{sub_mission.slug}"
                if today_dir.exists():
                    expected_run_dir = today_dir
            resume_dir = None
            if (expected_run_dir / "state.json").exists():
                try:
                    prior_state = statemachine.load(expected_run_dir)
                    if prior_state.phase == "READY":
                        default_ui.success(f"Stage {i}/{total} ({slug}) already completed (READY).")
                        self.state.integrated_changes = True
                        continue
                    resume_dir = expected_run_dir
                except Exception:
                    pass
            
            sub_orch = Orchestrator(
                tool_dir=self.tool_dir,
                mission_path=sub_mission_path,
                resume_dir=resume_dir,
                config_path=None,
                auto=self.auto,
                dry_run=self.dry_run,
                depth=self.depth + 1,
                max_depth=self.max_depth,
                log=self.log,
            )
            rc = sub_orch.run()
            if rc != 0:
                kind = sub_orch.state.blocked_kind or "BLOCKED_STAGE"
                raise GauntletError(
                    f"Stage {i}/{total} ({slug}) blocked in phase {sub_orch.state.phase}: "
                    f"{sub_orch.state.blocked_reason}", kind)
            self.state.integrated_changes = True
            default_ui.success(f"Stage {i}/{total} ({slug}) completed successfully.")
        
        self._transition("DELIVER_CHECKPOINT")

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
        branch_base = (self.state.base_commit
                       if self.state.wave == 0
                       else self.integration_branch())
        for lane in todo:  # worktree creation stays serial and idempotent
            wt = self.lane_wt(lane.id)
            if not wt.exists():
                worktrees.create_worktree(self.git, self.repo_path, wt,
                                          self.lane_branch(lane.id),
                                          branch_base)
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
            base = (self.state.base_commit
                    if self.state.wave == 0
                    else self.integration_branch())
            lane.changed = worktrees.lane_changed_files(
                self.git, self.lane_wt(lane.id), base)
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
            from src.ui import default_ui
            for lane in rejected:
                default_ui.error(f"Lane {lane.id} rejected", lane.detail)
            raise GauntletError(
                "INSPECT rejected lane(s): "
                + "; ".join(f"{lane.id}: {lane.detail}" for lane in rejected))
        else:
            from src.ui import default_ui
            for lane in self.state.lanes:
                if lane.status == "done":
                    default_ui.success(f"Lane {lane.id} inspection passed", lane.detail)
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
                + "; ".join(f"{r.command} ({r.detail})" for r in failed),
                "BLOCKED_GATE")
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
        fixed, deferred, dismissed = self._prior_findings()
        capsule = self._write_capsule(
            f"reviewer-w{self.state.wave}",
            capsules.reviewer(self.mission, wave=self.state.wave,
                              run_id=self.state.run_id,
                              diff_path=str(diff_path), fixed=fixed,
                              deferred=deferred, dismissed=dismissed))
        outcome = self._run_role("reviewer", capsule, self.work_wt(),
                                 write=False)
        data = verdicts.extract_block_from_file(outcome.result.output_path,
                                                "verdict")
        path = self.run_dir / "verdicts" / f"review-w{self.state.wave}.json"
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        self.state.reviews = self._slot(self.state.reviews, self.state.wave,
                                        str(path))
        self.report.section(
            f"REVIEW wave {self.state.wave}",
            f"groups: {len(data.get('groups', []))} "
            f"(harness: {outcome.harness}, attempts: {outcome.attempts})")
        from src.ui import default_ui
        default_ui.step("REVIEW", f"Review completed with {len(data.get('groups', []))} finding group(s)")
        self._transition("JUDGE")

    def _phase_judge(self) -> None:
        review_json = Path(self.state.reviews[-1]).read_text(encoding="utf-8")
        _, deferred, dismissed = self._prior_findings()
        capsule = self._write_capsule(
            f"judge-w{self.state.wave}",
            capsules.judge(self.mission, wave=self.state.wave,
                           run_id=self.state.run_id, review_json=review_json,
                           deferred=deferred, dismissed=dismissed))
        outcome = self._run_role("judge", capsule, self.work_wt(),
                                 write=False)
        data = verdicts.extract_block_from_file(outcome.result.output_path,
                                                "verdict")
        groups = verdicts.validate_verdict(data, self.mission.contract_ids)
        path = self.run_dir / "verdicts" / f"judgment-w{self.state.wave}.json"
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        self.state.judgments = self._slot(self.state.judgments,
                                          self.state.wave, str(path))
        blocking = [g for g in groups if g.blocking]
        polish = [g for g in groups if g.polish]
        history = self.state.blocking_history[:self.state.wave]
        self.state.blocking_history = history + [len(blocking)]
        self._save()
        trajectory = " → ".join(str(n) for n in self.state.blocking_history)
        self.report.section(
            f"JUDGE wave {self.state.wave}",
            f"groups: {len(groups)}, blocking: {len(blocking)}, "
            f"polish: {len(polish)}\nblocking trajectory: {trajectory}")
        
        from src.ui import default_ui
        default_ui.verdicts_table(groups)
        if not blocking:
            self._transition("POLISH")
            return

        policy = self.config["policy"]
        decision = statemachine.convergence_state(
            history, len(blocking), wave=self.state.wave,
            max_total_waves=policy["max_total_waves"])
        # A REDESIGN group is the architecture case: no proportionate local
        # fix exists, so no number of extra waves would have closed it.
        kind = ("BLOCKED_ARCHITECTURE"
                if any(g.verdict == "REDESIGN" for g in blocking)
                else "BLOCKED_CONVERGENCE")
        if decision == statemachine.CAPPED:
            raise GauntletError(
                f"fix waves hit the absolute cap "
                f"(max_total_waves={policy['max_total_waves']}) with "
                f"{len(blocking)} blocking group(s) left "
                f"(trajectory {trajectory})", kind)
        if decision == statemachine.STALLED:
            context = (
                f"convergence stalled: {len(blocking)} blocking group(s) "
                f"after {self.state.wave} fix wave(s); trajectory "
                f"{trajectory} did not beat its own best round.\n"
                "approve to grant one more fix wave anyway")
            if not self.auto and (policy["on_wave_cap"] != "checkpoint"
                    or not self._ask_human("wave-cap", context)):
                raise GauntletError(
                    f"convergence stalled: {len(blocking)} blocking group(s) "
                    f"left, trajectory {trajectory}", kind)
            self.log("auto mode or director granted one more fix wave despite the stall")
        self.state.wave += 1
        self._pending_groups = [
            {"root_cause": g.root_cause, "verdict": g.verdict, "fix": g.fix,
             "owns": g.owns, "defect_class": g.defect_class}
            for g in blocking]
        self._transition("PLAN_FIX")

    def _phase_plan_fix(self) -> None:
        groups = getattr(self, "_pending_groups", [])
        # Rebuild lightweight group objects for capsule rendering.
        groups = [verdicts.ClaimGroup(**g) for g in groups] if groups else None
        if groups is None:  # --resume: rebuild from the judgment on disk
            groups = [g for g in self._last_judgment_groups() if g.blocking]
        # Fast path: deterministic fix lane if 1 blocking group (0 LLM overhead)
        is_echo = self.config.get("roles", {}).get("planner", {}).get("chain", [{}])[0].get("harness") == "echo"
        if not is_echo and groups and len(groups) == 1:
            g = groups[0]
            owns = [g.owns] if g.owns else []
            if owns and owns[0].startswith("lib/"):
                cand = owns[0].replace("lib/", "test/").replace(".js", ".test.js")
                if (self.repo_path / cand).is_file() and cand not in owns:
                    owns.append(cand)
            lanes = [mission_mod.Lane(
                id="L1",
                owns=owns,
                forbidden=[],
                tests=[f"node --test {owns[-1]}"] if len(owns) > 1 and owns[-1].endswith(".test.js") else [],
                brief=g.fix or g.root_cause,
                addresses=[g.root_cause]
            )]
        else:
            _, lanes = self._run_planner(groups=groups)
            overlaps = self._check_overlaps(lanes)
            if overlaps:
                _, lanes = self._run_planner(
                    groups=groups,
                    complaint=f"lane owns globs overlapped: {overlaps}")
                overlaps = self._check_overlaps(lanes)
                if overlaps:
                    if self.auto or not self._ask_human(
                            "plan-fix",
                            f"fix-wave lanes overlap: {overlaps}\n"
                            "coalesce overlapping lanes automatically"):
                        # Auto-coalesce overlapping lanes into unified orthogonal lanes
                        files = worktrees.tracked_files(self.git, self.repo_path)
                        merged = list(lanes)
                        changed = True
                        while changed:
                            changed = False
                            # Find first overlapping pair of indices directly
                            pair = None
                            for i in range(len(merged)):
                                for j in range(i + 1, len(merged)):
                                    owns_i = merged[i].owns if hasattr(merged[i], "owns") else merged[i].get("owns", [])
                                    owns_j = merged[j].owns if hasattr(merged[j], "owns") else merged[j].get("owns", [])
                                    if any(worktrees.globs_may_overlap(ga, gb, files) for ga in owns_i for gb in owns_j):
                                        pair = (i, j)
                                        break
                                if pair:
                                    break
                            if not pair:
                                break
                            idx_a, idx_b = pair
                            la, lb = merged[idx_a], merged[idx_b]
                            owns_a = list(la.owns if hasattr(la, "owns") else la.get("owns", []))
                            owns_b = list(lb.owns if hasattr(lb, "owns") else lb.get("owns", []))
                            addr_a = list(la.addresses if hasattr(la, "addresses") else la.get("addresses", []))
                            addr_b = list(lb.addresses if hasattr(lb, "addresses") else lb.get("addresses", []))
                            tests_a = list(la.tests if hasattr(la, "tests") else la.get("tests", []))
                            tests_b = list(lb.tests if hasattr(lb, "tests") else lb.get("tests", []))
                            brief_a = la.brief if hasattr(la, "brief") else la.get("brief", "")
                            brief_b = lb.brief if hasattr(lb, "brief") else lb.get("brief", "")
                            
                            combined_owns = list(dict.fromkeys(owns_a + owns_b))
                            combined_addr = list(dict.fromkeys(addr_a + addr_b))
                            combined_tests = list(dict.fromkeys(tests_a + tests_b))
                            combined_brief = f"{brief_a}\nAlso: {brief_b}" if brief_a != brief_b else brief_a
                            
                            new_lane = statemachine.LaneState(
                                id=f"L{idx_a + 1}",
                                owns=combined_owns,
                                forbidden=[],
                                tests=combined_tests,
                                brief=combined_brief,
                                addresses=combined_addr
                            )
                            merged.pop(max(idx_a, idx_b))
                            merged.pop(min(idx_a, idx_b))
                            merged.insert(min(idx_a, idx_b), new_lane)
                            changed = True
                        for idx, lane in enumerate(merged):
                            if hasattr(lane, "id"):
                                lane.id = f"L{idx + 1}"
                            else:
                                lane["id"] = f"L{idx + 1}"
                        lanes = merged
        self.state.lanes = lanes
        self.report.section(
            f"PLAN_FIX wave {self.state.wave}",
            "\n".join(f"- {lane.id}: owns={lane.owns} "
                      f"addresses={lane.addresses}" for lane in lanes))
        self._save()
        self._transition("IMPLEMENT")

    def _polish_groups(self) -> list:
        """Non-blocking findings of every round, deduplicated by root cause.

        They accumulate across waves: a doc drift raised at wave 0 is not in
        any fix lane, so it survives until this pass clears it.
        """
        seen: set[str] = set()
        out = []
        for path in self.state.judgments:
            for group in self._judgment_groups(path):
                if group.polish and group.root_cause not in seen:
                    seen.add(group.root_cause)
                    out.append(group)
        return out

    def _phase_polish(self) -> None:
        """One pass over the non-blocking findings, after the last judgment.

        By construction nothing here can block delivery: a failed, contained,
        or gate-breaking polish is discarded and reported, and the candidate
        ships as judged.
        """
        wt = self.integration_wt()
        groups = self._polish_groups()
        if self.state.polish_done or not groups or not wt.is_dir():
            self.state.polish_done = True
            if groups and not wt.is_dir():
                self.state.polish_detail = (
                    f"{len(groups)} non-blocking finding(s) left unpolished: "
                    "no integration worktree")
            self._transition("DELIVER_CHECKPOINT")
            return

        owns = [g.owns for g in groups if g.owns]
        contained = len(owns) == len(groups)
        capsule = self._write_capsule(
            f"polish-w{self.state.wave}",
            capsules.polish(self.mission, groups, wave=self.state.wave,
                            run_id=self.state.run_id,
                            owns=owns if contained else None))
        before = worktrees.checkout_status(self.git, self.repo_path)
        try:
            self._run_role("fixer", capsule, wt, write=True)
        except (ChainExhausted, AuthAbort) as exc:
            detail = f"polish pass failed, candidate unchanged: {exc}"
        else:
            detail = self._settle_polish(wt, groups, owns, contained)
        drift = worktrees.checkout_drift(
            before, worktrees.checkout_status(self.git, self.repo_path))
        if drift:
            raise GauntletError(
                "SAFETY: main checkout modified during the polish pass: "
                + ", ".join(drift))
        self.state.polish_done = True
        self.state.polish_detail = detail
        self.report.section(f"POLISH wave {self.state.wave}", detail)
        self._save()
        self._transition("DELIVER_CHECKPOINT")

    def _settle_polish(self, wt, groups, owns, contained: bool) -> str:
        """Keep the polish only if it stayed in bounds and the gates hold."""
        changed = worktrees.checkout_status(self.git, wt)
        if not changed:
            return f"{len(groups)} finding(s) submitted, nothing changed"
        if contained:
            violations = worktrees.check_lane_diff(changed, owns, [])
            if violations:
                worktrees.discard_changes(self.git, wt)
                return ("polish discarded (wrote outside the findings' owns): "
                        + "; ".join(violations))
        else:
            self.log("polish: some findings declare no owns; "
                     "containment not enforced")
        results = gates_mod.run_gates(
            self.state.gates, cwd=wt, out_dir=self.run_dir / "outputs",
            dry_run=self.dry_run, log=self.log,
            timeout_s=self.config["policy"]["lane_timeout_s"])
        failed = [r for r in results if not r.ok]
        if failed:
            worktrees.discard_changes(self.git, wt)
            return ("polish discarded (gates failed on its result): "
                    + "; ".join(r.command for r in failed))
        worktrees.commit_all(
            self.git, wt,
            f"gauntlet({self.state.run_id}): polish pass "
            f"({len(groups)} non-blocking finding(s))")
        return (f"{len(groups)} finding(s) cleared, "
                f"{len(changed)} file(s) changed: " + ", ".join(changed))

    def _phase_deliver_checkpoint(self) -> None:
        context = (
            f"run: {self.state.run_id}\nwave: {self.state.wave}\n"
            f"changes integrated: {self.state.integrated_changes}\n"
            f"judgment: no blocking groups\n"
            f"polish: {self.state.polish_detail or 'nothing to polish'}\n"
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
                    + "; ".join(r.command for r in failed), "BLOCKED_GATE")
            if self.depth > 0:
                # Sub-stage: target branch is either in a parent worktree or ref
                target_wt = worktrees.find_worktree_for_branch(self.git, repo, target)
                if target_wt:
                    worktrees.ff_merge(self.git, target_wt, branch)
                else:
                    self.git.run(["branch", "-f", target, branch], cwd=repo)
            else:
                if worktrees.current_branch(self.git, repo) != target:
                    raise GauntletError(
                        f"main checkout is not on '{target}'; refusing "
                        "fast-forward merge")
                worktrees.ff_merge(self.git, repo, branch)
            final = "READY"
        self._cleanup()
        self.report.section("DELIVER", final)
        from src.ui import default_ui
        if final == "READY":
            default_ui.success(f"Mission '{self.state.slug}' delivered successfully into '{target}'!")
        elif final == "READY_NO_CHANGE":
            default_ui.success(f"Mission '{self.state.slug}' verified: behavior already satisfies contract.")
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

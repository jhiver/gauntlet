"""Git worktree lifecycle (orchestrator-owned) + glob machinery for the
mechanical checks (lane owns-glob overlap, forbidden-path diff rejection).

Workers never run git; the orchestrator alone runs git and gate commands.
With dry_run=True every mutating git command is printed instead of executed;
read-only commands (status, diff, rev-parse, ls-files) still run.
"""
from __future__ import annotations

import re
import shlex
import subprocess
from functools import lru_cache
from pathlib import Path


class GitError(Exception):
    pass


class Git:
    def __init__(self, *, dry_run: bool = False, log=print):
        self.dry_run = dry_run
        self.log = log

    def run(self, args, *, cwd=None, mutating: bool = True,
            check: bool = True) -> str | None:
        cmd = ["git"] + [str(a) for a in args]
        if self.dry_run and mutating:
            prefix = f"(cd {cwd} && " if cwd else "("
            suffix = ")" if cwd else ")"
            self.log(f"DRY-RUN: {prefix}{shlex.join(cmd)}{suffix}")
            return None
        if cwd is not None and not Path(cwd).is_dir():
            raise GitError(f"git cwd does not exist: {cwd}")
        proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
        if proc.returncode != 0:
            if not check:
                return None
            raise GitError(
                f"{shlex.join(cmd)} failed (rc={proc.returncode}): "
                f"{proc.stderr.strip()}")
        return proc.stdout

    def rc(self, args, *, cwd=None) -> int:
        if cwd is not None and not Path(cwd).is_dir():
            return 128
        proc = subprocess.run(["git"] + [str(a) for a in args], cwd=cwd,
                              capture_output=True, text=True)
        return proc.returncode


# ---------------------------------------------------------------- git helpers

def is_git_repo(git: Git, repo) -> bool:
    return git.rc(["rev-parse", "--git-dir"], cwd=repo) == 0


def staged_changes(git: Git, repo) -> bool:
    """True when the main checkout has staged changes (INIT refuses then)."""
    return git.rc(["diff", "--cached", "--quiet"], cwd=repo) != 0


def branch_exists(git: Git, repo, branch: str) -> bool:
    return git.rc(["rev-parse", "--verify", branch], cwd=repo) == 0


def base_commit(git: Git, repo, branch: str) -> str:
    return git.run(["rev-parse", branch], cwd=repo, mutating=False).strip()


def current_branch(git: Git, repo) -> str:
    return git.run(["rev-parse", "--abbrev-ref", "HEAD"],
                   cwd=repo, mutating=False).strip()


def rev_parse(git: Git, repo, ref: str) -> str | None:
    out = git.run(["rev-parse", "--verify", ref], cwd=repo,
                  mutating=False, check=False)
    return out.strip() if out else None


def tracked_files(git: Git, repo) -> list[str]:
    out = git.run(["ls-files"], cwd=repo, mutating=False)
    return [line for line in out.splitlines() if line]


def create_worktree(git: Git, repo, wt, branch: str, base: str) -> None:
    git.run(["worktree", "add", "-b", branch, str(wt), base], cwd=repo)
    # Symlink node_modules if present in main repo
    import os
    src = Path(repo) / "node_modules"
    dst = Path(wt) / "node_modules"
    if src.exists() and not dst.exists():
        try:
            os.symlink(src, dst)
        except OSError:
            pass


def remove_worktree(git: Git, repo, wt) -> None:
    git.run(["worktree", "remove", "--force", str(wt)], cwd=repo)


def delete_branch(git: Git, repo, branch: str) -> None:
    git.run(["branch", "-D", branch], cwd=repo)


def find_worktree_for_branch(git: Git, repo, branch: str) -> Path | None:
    output = git.run(["worktree", "list", "--porcelain"], cwd=repo, mutating=False) or ""
    current_wt = None
    for line in output.splitlines():
        if line.startswith("worktree "):
            current_wt = line[len("worktree "):].strip()
        elif line.startswith("branch ") and current_wt:
            b = line[len("branch "):].strip()
            if b == f"refs/heads/{branch}" or b == branch:
                return Path(current_wt)
    return None


def lane_changed_files(git: Git, wt, base: str) -> list[str]:
    """Files the lane touched vs the base commit: uncommitted work (workers
    never run git) plus any committed diff."""
    if not Path(wt).is_dir():
        return []
    changed: set[str] = set()
    status = git.run(["status", "--porcelain", "-uall"], cwd=wt,
                     mutating=False) or ""
    for line in status.splitlines():
        if len(line) < 4:
            continue
        path = line[3:]
        if " -> " in path:  # rename: keep the new path
            path = path.split(" -> ", 1)[1]
        cleaned = path.strip('"').rstrip("/")
        if cleaned in _ALWAYS_IGNORED_PATHS or cleaned.split("/")[0] in _ALWAYS_IGNORED_PATHS:
            continue
        changed.add(cleaned)
    mb = git.run(["merge-base", base, "HEAD"], cwd=wt, mutating=False)
    diff_target = mb.strip() if mb and mb.strip() else base
    diff = git.run(["diff", "--name-only", diff_target], cwd=wt, mutating=False) or ""
    for line in diff.splitlines():
        if not line:
            continue
        cleaned = line.strip('"').rstrip("/")
        if cleaned in _ALWAYS_IGNORED_PATHS or cleaned.split("/")[0] in _ALWAYS_IGNORED_PATHS:
            continue
        changed.add(cleaned)
    return sorted(changed)


def commit_all(git: Git, wt, message: str) -> bool:
    """Orchestrator commits the worker's changes (workers never run git)."""
    status = git.run(["status", "--porcelain"], cwd=wt, mutating=False)
    if not status or not status.strip():
        return False
    git.run(["add", "-A"], cwd=wt)
    for ignored in _ALWAYS_IGNORED_PATHS:
        git.run(["reset", "HEAD", "--", ignored], cwd=wt, check=False)
    staged = git.run(["diff", "--cached", "--name-only"], cwd=wt, mutating=False)
    if not staged or not staged.strip():
        return False
    git.run(["-c", "user.name=Gauntlet", "-c",
             "user.email=gauntlet@localhost", "commit", "-m", message], cwd=wt)
    return True


def discard_changes(git: Git, wt) -> None:
    """Drop every uncommitted change in an orchestrator-owned worktree.

    Used to throw away a polish pass that broke containment or the gates:
    everything already integrated is committed, so nothing else is at risk.
    """
    git.run(["reset", "--hard"], cwd=wt)
    git.run(["clean", "-fd"], cwd=wt)


def merge_branch(git: Git, wt, branch: str) -> None:
    git.run(["merge", "--no-edit", branch], cwd=wt)


def rebase_onto(git: Git, wt, onto: str) -> None:
    git.run(["rebase", onto], cwd=wt)


def is_ancestor(git: Git, repo, ancestor: str, commit: str) -> bool:
    try:
        git.run(["merge-base", "--is-ancestor", ancestor, commit], cwd=repo, mutating=False)
        return True
    except Exception:
        return False


def ff_merge(git: Git, repo, branch: str) -> None:
    git.run(["merge", "--ff-only", branch], cwd=repo)


# ------------------------------------------------------------- glob machinery

@lru_cache(maxsize=None)
def glob_to_regex(pattern: str) -> re.Pattern:
    """Translate a gitwildmatch-ish glob subset to a regex matched against
    repo-relative paths: `**` crosses directories, `*`/`?` stay within one
    path segment, `[...]` is a character class."""
    out: list[str] = []
    i = 0
    n = len(pattern)
    while i < n:
        c = pattern[i]
        if c == "*":
            if pattern[i:i + 2] == "**":
                i += 2
                if i < n and pattern[i] == "/":
                    i += 1
                    out.append("(?:.*/)?")  # **/ matches zero or more dirs
                else:
                    out.append(".*")
            else:
                out.append("[^/]*")
                i += 1
        elif c == "?":
            out.append("[^/]")
            i += 1
        elif c == "[":
            j = pattern.find("]", i + 1)
            if j == -1:
                out.append(re.escape(c))
                i += 1
            else:
                content = pattern[i + 1:j]
                if content.startswith("!"):
                    content = "^" + content[1:]
                out.append("[" + content + "]")
                i = j + 1
        elif c == "\\" and i + 1 < n:
            out.append(re.escape(pattern[i + 1]))
            i += 2
        else:
            out.append(re.escape(c))
            i += 1
    return re.compile("^" + "".join(out) + "$")


def glob_matches(pattern: str, path: str) -> bool:
    return glob_to_regex(pattern).match(path) is not None


def static_prefix(pattern: str) -> str:
    """Leading literal directory portion of a glob ('src/auth/**' -> 'src/auth')."""
    parts = []
    for part in pattern.split("/"):
        if any(ch in part for ch in "*?["):
            break
        parts.append(part)
    return "/".join(parts)


def _sample(pattern: str) -> str:
    """A concrete path matched by `pattern` (metacharacters -> 'x')."""
    out: list[str] = []
    i = 0
    n = len(pattern)
    while i < n:
        c = pattern[i]
        if c == "*":
            out.append("x")
            i += 2 if pattern[i:i + 2] == "**" else 1
        elif c == "?":
            out.append("x")
            i += 1
        elif c == "[":
            j = pattern.find("]", i + 1)
            if j == -1:
                out.append(c)
                i += 1
            else:
                content = pattern[i + 1:j]
                out.append(content[1] if content.startswith("!") else content[0])
                i = j + 1
        elif c == "\\" and i + 1 < n:
            out.append(pattern[i + 1])
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def globs_may_overlap(a: str, b: str, repo_files=()) -> bool:
    """Conservative overlap test: any tracked file matching both globs, or a
    sample path of either glob matched by the other."""
    for path in repo_files:
        if glob_matches(a, path) and glob_matches(b, path):
            return True
    return (glob_matches(a, _sample(b))
            or glob_matches(b, _sample(a)))


def find_overlaps(lanes, repo_files=()) -> list[tuple[str, str, str, str]]:
    """Pairwise intersection of `owns` globs across lanes (PLAN check).
    Returns (lane_a, lane_b, glob_a, glob_b) tuples."""
    overlaps = []
    for i in range(len(lanes)):
        for j in range(i + 1, len(lanes)):
            lane_a = lanes[i]
            lane_b = lanes[j]
            owns_a = lane_a.owns if hasattr(lane_a, "owns") else lane_a.get("owns", [])
            owns_b = lane_b.owns if hasattr(lane_b, "owns") else lane_b.get("owns", [])
            id_a = lane_a.id if hasattr(lane_a, "id") else lane_a.get("id", f"L{i+1}")
            id_b = lane_b.id if hasattr(lane_b, "id") else lane_b.get("id", f"L{j+1}")
            for ga in owns_a:
                for gb in owns_b:
                    if globs_may_overlap(ga, gb, repo_files):
                        overlaps.append((id_a, id_b, ga, gb))
    return overlaps


def check_lane_diff(changed: list[str], owns: list[str],
                    forbidden: list[str]) -> list[str]:
    """INSPECT check: every changed path must match at least one owns glob
    and no forbidden glob. Returns a list of violations."""
    violations = []
    for path in changed:
        if any(glob_matches(p, path) for p in forbidden):
            violations.append(f"{path} (forbidden path)")
        elif not any(glob_matches(p, path) for p in owns):
            violations.append(f"{path} (outside lane owns)")
    return violations


_ALWAYS_IGNORED_PATHS = {
    "node_modules",
    ".puppeteer-cache",
    ".pw-browsers",
    ".chrome-home",
    ".gauntlet",
    "gauntlet.toml",
    ".DS_Store",
}


def checkout_status(git: Git, repo) -> list[str]:
    """Paths with uncommitted changes in a checkout (porcelain -uall)."""
    out = git.run(["status", "--porcelain", "-uall"], cwd=repo,
                  mutating=False) or ""
    paths = []
    for line in out.splitlines():
        if len(line) < 4:
            continue
        path = line[3:]
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        cleaned = path.strip('"').rstrip("/")
        if cleaned in _ALWAYS_IGNORED_PATHS or cleaned.split("/")[0] in _ALWAYS_IGNORED_PATHS:
            continue
        paths.append(cleaned)
    return sorted(paths)


def checkout_drift(before: list[str], after: list[str],
                   ignore_prefixes=(".missions/", "gauntlet.toml")) -> list[str]:
    """INSPECT check: paths that appeared in a checkout while lanes ran,
    excluding run-local noise. Non-empty means a worker escaped its
    worktree into that checkout (containment breach)."""
    ignored = {p for p in before
               if any(p.startswith(pre) for pre in ignore_prefixes)}
    base = set(before) - ignored
    now = {p for p in after
           if not any(p.startswith(pre) for pre in ignore_prefixes)}
    return sorted(now - base)


def check_claimed_vs_diff(claimed: list[str], changed: list[str]) -> list[str]:
    """INSPECT check: every file the worker's gauntlet-report claims must
    appear in the lane diff. A miss means the write landed elsewhere."""
    changed_set = set(changed)
    return [f"{path} (claimed by worker but absent from lane diff)"
            for path in claimed if path not in changed_set]

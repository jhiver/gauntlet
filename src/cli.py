"""Gauntlet CLI entry point.

Usage: ./gauntlet [--config FILE] [--auto] [--resume RUN_DIR] [--dry-run] MISSION.md
"""
from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from src.config import ConfigError
from src.mission import MissionError
from src.orchestrator import Orchestrator

TOOL_DIR = Path(__file__).resolve().parent.parent


def _resolve(path: str | None, base: Path) -> Path | None:
    if not path:
        return None
    p = Path(path)
    return p if p.is_absolute() else (base / p).resolve()


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        prog="gauntlet",
        description="Execute a structured engineering mission "
                    "(see DESIGN.md).")
    parser.add_argument("mission", nargs="?",
                        help="mission contract file (markdown + TOML "
                             "frontmatter); optional with --resume")
    parser.add_argument("--config", metavar="FILE",
                        help="extra config file (highest precedence)")
    parser.add_argument("--auto", action="store_true",
                        help="auto-approve all director checkpoints")
    parser.add_argument("--resume", metavar="RUN_DIR",
                        help="resume a run from its run directory")
    parser.add_argument("--dry-run", action="store_true",
                        help="print git/harness commands instead of executing "
                             "(harnesses run as echo)")
    args = parser.parse_args(argv)

    # The wrapper exports the caller's cwd so relative paths keep working
    # even though the wrapper cd's into the tool directory.
    invoked_cwd = Path(os.environ.get("GAUNTLET_INVOKED_CWD", os.getcwd()))
    mission_path = _resolve(args.mission, invoked_cwd)
    config_path = _resolve(args.config, invoked_cwd)
    resume_dir = _resolve(args.resume, invoked_cwd)

    if not resume_dir and not mission_path:
        parser.error("MISSION.md is required unless --resume is given")
    if mission_path and not mission_path.is_file():
        parser.error(f"mission file not found: {mission_path}")
    if config_path and not config_path.is_file():
        parser.error(f"config file not found: {config_path}")
    if resume_dir and not (resume_dir / "state.json").is_file():
        parser.error(f"run directory has no state.json: {resume_dir}")

    try:
        orch = Orchestrator(
            tool_dir=TOOL_DIR,
            mission_path=mission_path,
            resume_dir=resume_dir,
            config_path=config_path,
            auto=args.auto,
            dry_run=args.dry_run,
        )
    except (MissionError, ConfigError) as exc:
        print(f"gauntlet: {exc}", file=sys.stderr)
        return 1
    return orch.run()


if __name__ == "__main__":
    sys.exit(main())

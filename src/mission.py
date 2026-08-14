"""Mission file parsing: TOML frontmatter delimited by +++ plus markdown body.

The body is the immutable root contract. AC/INV/NG entries carry stable IDs,
extracted with the regex from DESIGN.md to inject into capsules and to
validate verdict contract_ids.
"""
from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

_FRONTMATTER_RE = re.compile(r"\A\+\+\+[ \t]*\n(.*?)\n\+\+\+[ \t]*\n?", re.S)
_CONTRACT_ID_RE = re.compile(r"^- ((?:AC|INV|NG)-\w+):", re.M)


class MissionError(Exception):
    pass


@dataclass
class Repo:
    path: str
    target_branch: str = "main"
    gates: list[str] = field(default_factory=list)


@dataclass
class Lane:
    id: str
    owns: list[str]
    forbidden: list[str] = field(default_factory=list)
    tests: list[str] = field(default_factory=list)
    brief: str = ""
    addresses: list[str] = field(default_factory=list)


@dataclass
class Mission:
    slug: str
    repos: list[Repo]
    lanes: list[Lane]
    body: str
    contract_ids: set[str]
    source_path: Path


def parse_mission(text: str, source_path: Path) -> Mission:
    m = _FRONTMATTER_RE.match(text)
    if not m:
        raise MissionError(f"{source_path}: missing +++ TOML frontmatter")
    try:
        front = tomllib.loads(m.group(1))
    except tomllib.TOMLDecodeError as exc:
        raise MissionError(f"{source_path}: invalid TOML frontmatter: {exc}")
    body = text[m.end():]
    slug = front.get("slug") or Path(source_path).stem
    if not isinstance(slug, str) or not slug:
        raise MissionError(f"{source_path}: slug must be a non-empty string")

    repos = []
    for entry in front.get("repos", []):
        if not isinstance(entry, dict) or not entry.get("path"):
            raise MissionError(f"{source_path}: every [[repos]] needs a path")
        repos.append(Repo(
            path=str(entry["path"]),
            target_branch=str(entry.get("target_branch", "main")),
            gates=[str(g) for g in entry.get("gates", [])],
        ))
    if not repos:
        raise MissionError(f"{source_path}: at least one [[repos]] entry required")
    if len(repos) > 1:
        raise MissionError(
            f"{source_path}: this implementation supports exactly one "
            "[[repos]] entry per mission (see DESIGN.md state machine)")

    lanes = []
    seen_ids: set[str] = set()
    for entry in front.get("lanes", []):
        if not isinstance(entry, dict) or not entry.get("id"):
            raise MissionError(f"{source_path}: every [[lanes]] needs an id")
        lane = Lane(
            id=str(entry["id"]),
            owns=[str(g) for g in entry.get("owns", [])],
            forbidden=[str(g) for g in entry.get("forbidden", [])],
            tests=[str(t) for t in entry.get("tests", [])],
            brief=str(entry.get("brief", "")),
            addresses=[str(a) for a in entry.get("addresses", [])],
        )
        if lane.id in seen_ids:
            raise MissionError(f"{source_path}: duplicate lane id '{lane.id}'")
        if not lane.owns:
            raise MissionError(
                f"{source_path}: lane '{lane.id}' must own at least one glob")
        seen_ids.add(lane.id)
        lanes.append(lane)

    contract_ids = set(_CONTRACT_ID_RE.findall(body))
    return Mission(slug=slug, repos=repos, lanes=lanes, body=body,
                   contract_ids=contract_ids, source_path=Path(source_path))


def load_mission(path) -> Mission:
    path = Path(path)
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise MissionError(f"cannot read mission file {path}: {exc}")
    return parse_mission(text, path)

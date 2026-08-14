"""gauntlet-report / gauntlet-verdict / gauntlet-plan block parsing+validation.

Extracts the LAST matching fenced block, JSON-parses, and schema-validates:
verdict enum, defect class enum, contract IDs must exist in the contract, lane
ownership globs non-empty. See DESIGN.md "Structured I/O protocol".
"""
from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from pathlib import Path

_BLOCK_RE = re.compile(r"```gauntlet-(report|verdict|plan|stages)[ \t]*\n(.*?)```", re.S)

VERDICT_VALUES = {"FIX", "REDESIGN", "REPORT_ONLY", "DISMISS"}
ACTIONABLE_VERDICTS = {"FIX", "REDESIGN"}

# Defect class: what kind of gap the group describes. Only code defects hold
# up delivery; documentation drift and evidence gaps are real work but they
# are collected for a single polish pass instead of costing a fix wave.
CODE_DEFECT = "code_defect"
CLASS_VALUES = {CODE_DEFECT, "doc_drift", "evidence_gap"}


class VerdictError(Exception):
    pass


@dataclass
class ClaimGroup:
    root_cause: str
    claims: list[str] = field(default_factory=list)
    contract_ids: list[str] = field(default_factory=list)
    verdict: str = "REPORT_ONLY"
    fix: str = ""
    owns: str = ""
    defect_class: str = CODE_DEFECT  # JSON key: "class"

    @property
    def actionable(self) -> bool:
        return self.verdict in ACTIONABLE_VERDICTS

    @property
    def blocking(self) -> bool:
        """Actionable groups that must be fixed before delivery.

        REDESIGN always blocks whatever its class: only a code defect can
        make the smallest additive patch disproportionate.
        """
        return self.actionable and (self.verdict == "REDESIGN"
                                    or self.defect_class == CODE_DEFECT)

    @property
    def polish(self) -> bool:
        """Actionable but non-blocking: handled by the pre-delivery polish."""
        return self.actionable and not self.blocking


def extract_block(text: str, kind: str) -> dict:
    """Return the parsed JSON of the LAST ```gauntlet-<kind> block."""
    found = None
    for match in _BLOCK_RE.finditer(text):
        if match.group(1) == kind:
            found = match.group(2)
    if found is None:
        raise VerdictError(f"no gauntlet-{kind} fenced block found")
    try:
        data = json.loads(found)
    except json.JSONDecodeError as exc:
        raise VerdictError(f"gauntlet-{kind} block is not valid JSON: {exc}")
    if not isinstance(data, dict):
        raise VerdictError(f"gauntlet-{kind} block must be a JSON object")
    return data


def extract_block_from_file(path, kind: str) -> dict:
    return extract_block(Path(path).read_text(encoding="utf-8", errors="replace"),
                         kind)


def _str_list(value, what: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise VerdictError(f"{what} must be a list of strings")
    return list(value)


def validate_report(data: dict) -> dict:
    try:
        report = {
            "files_changed": _str_list(data["files_changed"], "files_changed"),
            "tests_run": _str_list(data["tests_run"], "tests_run"),
            "tests_passed": data["tests_passed"],
            "partial": data["partial"],
            "notes": data.get("notes", ""),
        }
    except KeyError as exc:
        raise VerdictError(f"gauntlet-report missing key {exc}")
    if not isinstance(report["tests_passed"], bool):
        raise VerdictError("tests_passed must be a boolean")
    if not isinstance(report["partial"], bool):
        raise VerdictError("partial must be a boolean")
    if not isinstance(report["notes"], str):
        raise VerdictError("notes must be a string")
    return report


def validate_verdict(data: dict, contract_ids: set[str]) -> list[ClaimGroup]:
    groups_raw = data.get("groups")
    if not isinstance(groups_raw, list):
        raise VerdictError("gauntlet-verdict 'groups' must be a list")
    groups: list[ClaimGroup] = []
    for i, raw in enumerate(groups_raw):
        if not isinstance(raw, dict):
            raise VerdictError(f"verdict group {i} must be an object")
        verdict = raw.get("verdict")
        if verdict not in VERDICT_VALUES:
            raise VerdictError(
                f"verdict group {i}: verdict must be one of "
                f"{sorted(VERDICT_VALUES)}, got {verdict!r}")
        ids = _str_list(raw.get("contract_ids", []),
                        f"verdict group {i} contract_ids")
        unknown = [c for c in ids if c not in contract_ids]
        if unknown:
            raise VerdictError(
                f"verdict group {i}: unknown contract IDs {unknown}")
        root_cause = raw.get("root_cause")
        if not isinstance(root_cause, str) or not root_cause:
            raise VerdictError(f"verdict group {i}: root_cause required")
        defect_class = raw.get("class", CODE_DEFECT)
        if defect_class not in CLASS_VALUES:
            raise VerdictError(
                f"verdict group {i}: class must be one of "
                f"{sorted(CLASS_VALUES)}, got {defect_class!r}")
        groups.append(ClaimGroup(
            root_cause=root_cause,
            claims=_str_list(raw.get("claims", []), f"group {i} claims"),
            contract_ids=ids,
            verdict=verdict,
            fix=str(raw.get("fix", "")),
            owns=str(raw.get("owns", "")),
            defect_class=defect_class,
        ))
    return groups


def validate_plan(data: dict) -> list[dict]:
    lanes_raw = data.get("lanes")
    if not isinstance(lanes_raw, list) or not lanes_raw:
        raise VerdictError("gauntlet-plan 'lanes' must be a non-empty list")
    lanes = []
    seen: set[str] = set()
    for i, raw in enumerate(lanes_raw):
        if not isinstance(raw, dict):
            raise VerdictError(f"plan lane {i} must be an object")
        lane_id = raw.get("id")
        if not isinstance(lane_id, str) or not lane_id:
            raise VerdictError(f"plan lane {i}: id required")
        if lane_id in seen:
            raise VerdictError(f"plan lane {i}: duplicate lane id '{lane_id}'")
        seen.add(lane_id)
        owns = _str_list(raw.get("owns", []), f"plan lane {i} owns")
        if not owns:
            raise VerdictError(
                f"plan lane {i} ('{lane_id}') must own at least one glob")
        lanes.append({
            "id": lane_id,
            "owns": owns,
            "forbidden": _str_list(raw.get("forbidden", []),
                                   f"plan lane {i} forbidden"),
            "tests": _str_list(raw.get("tests", []), f"plan lane {i} tests"),
            "brief": str(raw.get("brief", "")),
            "addresses": _str_list(raw.get("addresses", []),
                                   f"plan lane {i} addresses"),
        })
    return lanes


def validate_stages(data: dict, valid_contract_ids: set[str] | None = None) -> list[dict]:
    stages_raw = data.get("stages")
    if not isinstance(stages_raw, list) or not stages_raw:
        raise VerdictError("gauntlet-stages 'stages' must be a non-empty list")
    stages = []
    seen: set[str] = set()
    for i, raw in enumerate(stages_raw):
        if not isinstance(raw, dict):
            raise VerdictError(f"stage {i} must be an object")
        slug = raw.get("slug")
        if not isinstance(slug, str) or not slug:
            raise VerdictError(f"stage {i}: slug required")
        if slug in seen:
            raise VerdictError(f"stage {i}: duplicate slug '{slug}'")
        seen.add(slug)
        brief = str(raw.get("brief", ""))
        owns = _str_list(raw.get("owns", []), f"stage {i} owns")
        cids = _str_list(raw.get("contract_ids", []), f"stage {i} contract_ids")
        if valid_contract_ids:
            unknown = set(cids) - valid_contract_ids
            if unknown:
                raise VerdictError(f"stage {i} mentions unknown contract IDs: {sorted(unknown)}")
        stages.append({
            "slug": slug,
            "brief": brief,
            "owns": owns,
            "contract_ids": cids,
            "gates": _str_list(raw.get("gates", []), f"stage {i} gates"),
        })
    return stages


def extract_planner_result(text: str, valid_contract_ids: set[str] | None = None) -> tuple[str, list[dict]]:
    """Extract either gauntlet-plan (lanes) or gauntlet-stages (sequential stages).
    Returns ("lanes", list[lane_dict]) or ("stages", list[stage_dict]).
    """
    found_kind = None
    found_text = None
    for match in _BLOCK_RE.finditer(text):
        kind = match.group(1)
        if kind in ("plan", "stages"):
            found_kind = kind
            found_text = match.group(2)
    if found_kind is None or found_text is None:
        raise VerdictError("no gauntlet-plan or gauntlet-stages fenced block found")
    try:
        data = json.loads(found_text)
    except json.JSONDecodeError as exc:
        raise VerdictError(f"gauntlet-{found_kind} block is not valid JSON: {exc}")
    if not isinstance(data, dict):
        raise VerdictError(f"gauntlet-{found_kind} block must be a JSON object")
    if found_kind == "stages":
        return "stages", validate_stages(data, valid_contract_ids)
    return "lanes", validate_plan(data)

"""Super-Auto Routing: Automatic model selection and fallback chain generator
calibrated against the Artificial Analysis Pareto frontier (Speed vs Intelligence).

Evaluates mission contract complexity, security/safety risk factors, and scope
to construct optimal model routing and fallback chains without requiring
manual configuration files.
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

HIGH_RISK_PATTERNS = [
    r"\bauth\w*", r"\btoken\w*", r"\bsecret\w*", r"\bcredential\w*",
    r"\bpasskey\w*", r"\bvault\w*", r"\bcrypto\w*", r"\btakeover\w*",
    r"\bsession\w*", r"\bconcurren\w*", r"\brace\w*", r"\bthread\w*",
    r"\bmutex\w*", r"\block\w*", r"\bdistributed\w*", r"\brecovery\w*",
    r"\bsafety\w*", r"\bpayment\w*", r"\btransaction\w*", r"\bdataloss\w*",
    r"\bsecurity\w*", r"\bexploit\w*", r"\bpermission\w*", r"\bisolation\w*",
]


@dataclass
class MissionProfile:
    tier: str  # "high-risk", "standard", "fast"
    score: int
    reasons: list[str]
    roles: dict[str, dict[str, Any]]


def analyze_mission(mission: Any) -> MissionProfile:
    """Analyze a Mission instance and return the recommended profile + role routing."""
    text_to_scan = " ".join([
        getattr(mission, "body", ""),
        getattr(mission, "slug", ""),
        " ".join(lane.brief for lane in getattr(mission, "lanes", [])),
    ])

    reasons: list[str] = []
    score = 0

    # 1. High-risk keyword matching
    matched_keywords = set()
    for pat in HIGH_RISK_PATTERNS:
        matches = re.findall(pat, text_to_scan, flags=re.IGNORECASE)
        if matches:
            matched_keywords.update(m.lower() for m in matches[:3])
    if matched_keywords:
        kw_sample = ", ".join(sorted(list(matched_keywords))[:5])
        score += 3
        reasons.append(f"Security/High-risk concepts detected ({kw_sample})")

    # 2. Scope analysis (owned paths)
    total_owns = sum(len(lane.owns) for lane in getattr(mission, "lanes", []))
    if total_owns >= 20:
        score += 2
        reasons.append(f"Large scope: {total_owns} owned path patterns")
    elif total_owns >= 8:
        score += 1
        reasons.append(f"Moderate scope: {total_owns} owned path patterns")

    # 3. Gates complexity
    gates = []
    for r in getattr(mission, "repos", []):
        gates.extend(r.gates)
    if len(gates) >= 5:
        score += 1
        reasons.append(f"High-assurance gate suite ({len(gates)} gates)")

    # 4. Invariants and AC count
    contract_ids = getattr(mission, "contract_ids", set())
    if len(contract_ids) >= 8:
        score += 1
        reasons.append(f"Rigorous contract ({len(contract_ids)} AC/INV clauses)")

    # Tier selection
    if score >= 3:
        tier = "high-risk"
        roles = {
            "implementer": {"chain": [
                {"harness": "agy"},  # gemini-3.7-flash-high
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"},
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
                {"harness": "kimi", "model": "kimi-code/k3"},
            ]},
            "fixer": {"chain": [
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"},
                {"harness": "agy"},
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
            ]},
            "reviewer": {"chain": [
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"},
                {"harness": "kimi", "model": "kimi-code/k3"},
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
            ]},
            "judge": {"chain": [
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"},
                {"harness": "kimi", "model": "kimi-code/k3"},
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
            ]},
            "planner": {"chain": [
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"},
                {"harness": "kimi", "model": "kimi-code/k3"},
            ]},
            "director": {"chain": [{"harness": "human"}]},
        }
    elif score >= 1:
        tier = "standard"
        roles = {
            "implementer": {"chain": [
                {"harness": "agy"},  # gemini-3.7-flash-high
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
                {"harness": "kimi", "model": "kimi-code/k3"},
            ]},
            "fixer": {"chain": [
                {"harness": "agy"},
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "high"},
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
            ]},
            "reviewer": {"chain": [
                {"harness": "kimi", "model": "kimi-code/k3"},
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "high"},
            ]},
            "judge": {"chain": [
                {"harness": "kimi", "model": "kimi-code/k3"},
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "high"},
            ]},
            "planner": {"chain": [
                {"harness": "kimi", "model": "kimi-code/k3"},
                {"harness": "codex", "model": "gpt-5.6-sol", "effort": "high"},
            ]},
            "director": {"chain": [{"harness": "human"}]},
        }
    else:
        tier = "fast"
        reasons.append("Localized scope without high-risk invariants")
        roles = {
            "implementer": {"chain": [
                {"harness": "agy"},  # gemini-3.7-flash-high (fast Pareto optimal)
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
            ]},
            "fixer": {"chain": [
                {"harness": "agy"},
                {"harness": "cmd", "model": "gpt-5.6-luna", "effort": "max"},
            ]},
            "reviewer": {"chain": [
                {"harness": "kimi", "model": "kimi-code/k3"},
                {"harness": "agy"},
            ]},
            "judge": {"chain": [
                {"harness": "kimi", "model": "kimi-code/k3"},
                {"harness": "agy"},
            ]},
            "planner": {"chain": [
                {"harness": "kimi", "model": "kimi-code/k3"},
            ]},
            "director": {"chain": [{"harness": "human"}]},
        }

    return MissionProfile(tier=tier, score=score, reasons=reasons, roles=roles)

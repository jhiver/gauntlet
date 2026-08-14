"""TOML config load + merge + chain validation.

Resolution order (later overrides): built-in defaults -> <tool>/gauntlet.toml
-> <mission-dir>/gauntlet.toml -> --config FILE.
"""
from __future__ import annotations

import copy
import json
import tomllib
from pathlib import Path

ADAPTER_NAMES = {"agy", "cmd", "codex", "kimi", "reasonix", "human", "echo"}
ROLES = ("implementer", "fixer", "reviewer", "judge", "planner", "director")
WRITE_ROLES = {"implementer", "fixer"}
FALLBACK_ACTIONS = {"next", "next_and_break", "break",
                    "backoff_retry_then_next", "retry_once_then_next"}
WAVE_CAP_ACTIONS = {"checkpoint", "block"}

BUILTIN_DEFAULTS = {
    "harnesses": {
        "echo": {"adapter": "echo", "supports_write": True},
        "human": {"adapter": "human", "supports_write": True},
    },
    "roles": {
        "implementer": {"chain": [{"harness": "echo"}]},
        "fixer": {"chain": [{"harness": "echo"}]},
        "reviewer": {"chain": [{"harness": "echo"}]},
        "judge": {"chain": [{"harness": "echo"}]},
        "planner": {"chain": [{"harness": "echo"}]},
        "director": {"chain": [{"harness": "human"}]},
    },
    "policy": {
        "checkpoints": ["plan", "deliver"],
        "max_total_waves": 5,
        "on_wave_cap": "checkpoint",
        "idle_timeout_s": 900,
        "hard_timeout_s": 2700,
        "lane_timeout_s": 5400,
    },
    "fallback": {
        "on_quota": "next_and_break",
        "on_auth": "break",
        "on_rate_limit": "backoff_retry_then_next",
        "on_model_unavailable": "next",
        "on_timeout": "retry_once_then_next",
        "on_crash": "retry_once_then_next",
        "on_invalid_output": "retry_once_then_next",
        "max_attempts_per_task": 3,
    },
}


class ConfigError(Exception):
    pass


def _merge(base: dict, override: dict) -> dict:
    """Deep merge: tables merge recursively, arrays/scalars are replaced."""
    out = copy.deepcopy(base)
    for key, value in override.items():
        if isinstance(value, dict) and isinstance(out.get(key), dict):
            out[key] = _merge(out[key], value)
        else:
            out[key] = copy.deepcopy(value)
    return out


def validate_config(cfg: dict) -> None:
    harnesses = cfg.get("harnesses", {})
    for hname, hcfg in harnesses.items():
        if hcfg.get("adapter") not in ADAPTER_NAMES:
            raise ConfigError(
                f"harness '{hname}': unknown adapter '{hcfg.get('adapter')}'")
    roles = cfg.get("roles", {})
    for role in ROLES:
        chain = roles.get(role, {}).get("chain", [])
        if not chain:
            raise ConfigError(f"role '{role}' has an empty chain")
        for link in chain:
            hname = link.get("harness")
            if hname not in harnesses:
                raise ConfigError(
                    f"role '{role}': unknown harness '{hname}' in chain")
            if (role in WRITE_ROLES
                    and not harnesses[hname].get("supports_write")):
                raise ConfigError(
                    f"role '{role}': harness '{hname}' does not support write")
    for key, value in cfg.get("fallback", {}).items():
        if key.startswith("on_") and value not in FALLBACK_ACTIONS:
            raise ConfigError(f"fallback '{key}': unknown action '{value}'")
    policy = cfg.get("policy", {})
    if "max_fix_waves" in policy:
        # Loud migration: a stale key would silently cap fix waves while the
        # mission is still converging — the exact failure this replaced.
        raise ConfigError(
            "policy 'max_fix_waves' was replaced by 'max_total_waves' (an "
            "absolute safety cap; fix waves are now granted while the "
            "blocking-group count keeps falling). Rename the key.")
    for key in ("max_total_waves", "idle_timeout_s", "hard_timeout_s",
                "lane_timeout_s"):
        if not isinstance(policy.get(key), int):
            raise ConfigError(f"policy '{key}' must be an integer")
    if policy.get("on_wave_cap") not in WAVE_CAP_ACTIONS:
        raise ConfigError(
            f"policy 'on_wave_cap' must be one of {sorted(WAVE_CAP_ACTIONS)}")
    if not isinstance(cfg.get("fallback", {}).get("max_attempts_per_task"), int):
        raise ConfigError("fallback 'max_attempts_per_task' must be an integer")


def load_config(*, tool_dir=None, mission_dir=None, config_file=None) -> dict:
    cfg = copy.deepcopy(BUILTIN_DEFAULTS)
    candidates = []
    if tool_dir:
        candidates.append(Path(tool_dir) / "gauntlet.toml")
    if mission_dir:
        candidates.append(Path(mission_dir) / "gauntlet.toml")
    if config_file:
        candidates.append(Path(config_file))
    for path in candidates:
        if path.is_file():
            try:
                loaded = tomllib.loads(path.read_text(encoding="utf-8"))
            except tomllib.TOMLDecodeError as exc:
                raise ConfigError(f"{path}: invalid TOML: {exc}")
            cfg = _merge(cfg, loaded)
    # "human" is the implicit terminal link of every chain; make sure the
    # harness entry always exists even if no config file defines it.
    cfg["harnesses"].setdefault(
        "human", {"adapter": "human", "supports_write": True})
    validate_config(cfg)
    return cfg


def _fmt(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, str):
        return json.dumps(value)  # JSON string escaping is valid TOML
    if isinstance(value, list):
        if value and all(isinstance(item, dict) for item in value):
            tables = []
            for item in value:
                inner = ", ".join(f"{k} = {_fmt(v)}" for k, v in item.items())
                tables.append("{ " + inner + " }")
            return "[\n  " + ",\n  ".join(tables) + ",\n]"
        return "[" + ", ".join(_fmt(v) for v in value) + "]"
    raise ConfigError(f"cannot serialize value to TOML: {value!r}")


def dump_toml(cfg: dict) -> str:
    """Minimal TOML serializer for the effective config (run-dir snapshot)."""
    lines: list[str] = []

    def emit(prefix: str, table: dict) -> None:
        scalars = {k: v for k, v in table.items() if not isinstance(v, dict)}
        subtables = {k: v for k, v in table.items() if isinstance(v, dict)}
        if prefix:
            lines.append(f"[{prefix}]")
        for key, value in scalars.items():
            lines.append(f"{key} = {_fmt(value)}")
        if scalars or prefix:
            lines.append("")
        for key, value in subtables.items():
            emit(f"{prefix}.{key}" if prefix else key, value)

    emit("", cfg)
    return "\n".join(lines).strip() + "\n"

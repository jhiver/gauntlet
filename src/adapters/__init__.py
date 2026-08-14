"""Harness adapters. Registry maps adapter module names to classes."""
from src.adapters.agy import AgyAdapter
from src.adapters.cmd import CmdAdapter
from src.adapters.codex import CodexAdapter
from src.adapters.echo import EchoAdapter
from src.adapters.human import HumanAdapter
from src.adapters.kimi import KimiAdapter
from src.adapters.reasonix import ReasonixAdapter

ADAPTER_CLASSES = {
    "agy": AgyAdapter,
    "cmd": CmdAdapter,
    "codex": CodexAdapter,
    "echo": EchoAdapter,
    "human": HumanAdapter,
    "kimi": KimiAdapter,
    "reasonix": ReasonixAdapter,
}

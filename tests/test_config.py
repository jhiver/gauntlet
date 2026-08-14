import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.config import (BUILTIN_DEFAULTS, ConfigError, dump_toml,  # noqa: E402
                        load_config)

import helpers  # noqa: E402


class ConfigTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def test_builtin_defaults_are_valid_and_echo_based(self):
        cfg = load_config(tool_dir=self.dir)  # empty dir: no config files
        self.assertEqual(cfg["roles"]["implementer"]["chain"],
                         [{"harness": "echo"}])
        self.assertEqual(cfg["roles"]["director"]["chain"],
                         [{"harness": "human"}])
        self.assertEqual(cfg["fallback"]["max_attempts_per_task"], 3)

    def test_merge_precedence(self):
        tool = self.dir / "tool"
        tool.mkdir()
        (tool / "gauntlet.toml").write_text(
            "[policy]\nmax_fix_waves = 1\n"
            "[harnesses.cmd]\nadapter = \"cmd\"\nsupports_write = true\n")
        mission_dir = self.dir / "mission"
        mission_dir.mkdir()
        (mission_dir / "gauntlet.toml").write_text(
            "[policy]\nmax_fix_waves = 2\n")
        override = self.dir / "override.toml"
        override.write_text("[policy]\nmax_fix_waves = 5\n")

        cfg = load_config(tool_dir=tool)
        self.assertEqual(cfg["policy"]["max_fix_waves"], 1)
        self.assertIn("cmd", cfg["harnesses"])  # tool harness survives

        cfg = load_config(tool_dir=tool, mission_dir=mission_dir)
        self.assertEqual(cfg["policy"]["max_fix_waves"], 2)

        cfg = load_config(tool_dir=tool, mission_dir=mission_dir,
                          config_file=override)
        self.assertEqual(cfg["policy"]["max_fix_waves"], 5)

    def test_roles_from_later_config_replace_chain(self):
        override = self.dir / "c.toml"
        override.write_text(
            "[roles.implementer]\nchain = [ { harness = \"echo\" } ]\n")
        cfg = load_config(tool_dir=helpers.TOOL_DIR, config_file=override)
        self.assertEqual(cfg["roles"]["implementer"]["chain"],
                         [{"harness": "echo"}])
        # Non-overridden roles keep the tool config's chains.
        self.assertNotEqual(cfg["roles"]["reviewer"]["chain"],
                            [{"harness": "echo"}])

    def test_write_role_rejects_read_only_harness(self):
        override = self.dir / "bad.toml"
        override.write_text(
            "[harnesses.ro]\nadapter = \"reasonix\"\nsupports_write = false\n"
            "[roles.implementer]\nchain = [ { harness = \"ro\" } ]\n")
        with self.assertRaises(ConfigError):
            load_config(tool_dir=self.dir, config_file=override)

    def test_unknown_harness_in_chain_rejected(self):
        override = self.dir / "bad.toml"
        override.write_text(
            "[roles.reviewer]\nchain = [ { harness = \"nope\" } ]\n")
        with self.assertRaises(ConfigError):
            load_config(tool_dir=self.dir, config_file=override)

    def test_unknown_adapter_rejected(self):
        override = self.dir / "bad.toml"
        override.write_text(
            "[harnesses.x]\nadapter = \"nope\"\nsupports_write = true\n")
        with self.assertRaises(ConfigError):
            load_config(tool_dir=self.dir, config_file=override)

    def test_unknown_fallback_action_rejected(self):
        override = self.dir / "bad.toml"
        override.write_text("[fallback]\non_quota = \"panic\"\n")
        with self.assertRaises(ConfigError):
            load_config(tool_dir=self.dir, config_file=override)

    def test_dump_toml_round_trip(self):
        cfg = load_config(tool_dir=helpers.TOOL_DIR)
        dumped = dump_toml(cfg)
        reloaded = tomllib.loads(dumped)
        self.assertEqual(reloaded, cfg)

    def test_builtin_defaults_dump_round_trip(self):
        dumped = dump_toml(BUILTIN_DEFAULTS)
        self.assertEqual(tomllib.loads(dumped), BUILTIN_DEFAULTS)


if __name__ == "__main__":
    unittest.main()

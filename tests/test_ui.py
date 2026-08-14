import io
import unittest
from src.ui import UI
from src.verdicts import ClaimGroup


class UITest(unittest.TestCase):
    def setUp(self):
        self.stream = io.StringIO()
        self.ui = UI(stream=self.stream, enable_color=False)

    def test_banner_renders_cleanly(self):
        self.ui.banner(
            title="GAUNTLET MISSION • test-slug",
            subtitle="Testing UI",
            meta={"Repository": "/path/to/repo", "Lanes": "1"}
        )
        out = self.stream.getvalue()
        self.assertIn("GAUNTLET MISSION", out)
        self.assertIn("test-slug", out)
        self.assertIn("Repository: /path/to/repo", out)

    def test_phase_card(self):
        self.ui.phase_card("IMPLEMENT", wave=1, detail="Testing phase")
        out = self.stream.getvalue()
        self.assertIn("PHASE: IMPLEMENT [Wave 1]", out)

    def test_gate_result(self):
        self.ui.gate_result(1, 5, "npm test", True, 2.3)
        out = self.stream.getvalue()
        self.assertIn("[1/5]", out)
        self.assertIn("npm test", out)
        self.assertIn("PASS", out)
        self.assertIn("2.3s", out)

    def test_verdicts_table(self):
        groups = [
            ClaimGroup(
                root_cause="Missing null check in token parser",
                claims=["Crash when token is empty"],
                contract_ids=["AC-1"],
                verdict="FIX",
                defect_class="code_defect",
                fix="Add early return if token is None",
                owns="lib/auth.js"
            )
        ]
        self.ui.verdicts_table(groups)
        out = self.stream.getvalue()
        self.assertIn("JUDGMENT VERDICTS (1 group(s))", out)
        self.assertIn("Missing null check in token parser", out)
        self.assertIn("lib/auth.js", out)


if __name__ == "__main__":
    unittest.main()

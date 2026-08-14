import unittest
from pathlib import Path
from src.autoroute import analyze_mission
from src.mission import Lane, Mission, Repo


class AutoRouteTest(unittest.TestCase):
    def test_high_risk_mission_detection(self):
        m = Mission(
            slug="auth-secret-vault",
            source_path=Path("/tmp/m.md"),
            repos=[Repo(path="/repo", gates=["test1", "test2", "test3", "test4", "test5"])],
            lanes=[
                Lane("L1", owns=["lib/auth/**", "lib/passkey/**", "lib/vault/**"] * 8,
                     forbidden=[], tests=[], brief="Implement passkey auth takeover"),
            ],
            body="# Objective\nImplement secure authentication with passkey credentials\n## AC\n- AC-1: secret",
            contract_ids={"AC-1"},
        )
        profile = analyze_mission(m)
        self.assertEqual(profile.tier, "high-risk")
        self.assertTrue(profile.score >= 3)
        self.assertEqual(profile.roles["reviewer"]["chain"][0]["harness"], "codex")
        self.assertEqual(profile.roles["reviewer"]["chain"][0]["effort"], "xhigh")

    def test_standard_mission_detection(self):
        m = Mission(
            slug="button-color",
            source_path=Path("/tmp/m.md"),
            repos=[Repo(path="/repo", gates=["npm test"])],
            lanes=[
                Lane("L1", owns=["src/button.ts", "src/styles.css"],
                     forbidden=[], tests=[], brief="Change button color"),
            ],
            body="# Objective\nChange button color from blue to green\n## AC\n- AC-1: green",
            contract_ids={"AC-1"},
        )
        profile = analyze_mission(m)
        self.assertEqual(profile.tier, "fast")
        self.assertEqual(profile.roles["implementer"]["chain"][0]["harness"], "agy")


if __name__ == "__main__":
    unittest.main()

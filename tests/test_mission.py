import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.mission import MissionError, load_mission, parse_mission  # noqa: E402

import helpers  # noqa: E402


class MissionParsingTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def test_parse_full_mission(self):
        repo = self.dir / "repo"
        mission_path = helpers.write_mission(self.dir / "m.md", repo)
        m = load_mission(mission_path)
        self.assertEqual(m.slug, "example")
        self.assertEqual(len(m.repos), 1)
        self.assertEqual(m.repos[0].target_branch, "main")
        self.assertEqual(m.repos[0].gates, ["true"])
        self.assertEqual([l.id for l in m.lanes], ["L1"])
        self.assertEqual(m.lanes[0].owns, ["src/example/**"])
        self.assertEqual(m.lanes[0].forbidden, [])
        self.assertEqual(m.contract_ids, {"AC-1", "INV-1", "NG-1"})
        self.assertIn("# Objective", m.body)

    def test_slug_derived_from_filename(self):
        repo = self.dir / "repo"
        mission_path = helpers.write_mission(self.dir / "auth-refactor.md",
                                             repo, slug="")
        m = load_mission(mission_path)
        # slug = "" is falsy in the frontmatter, so filename wins
        self.assertEqual(m.slug, "auth-refactor")

    def test_missing_frontmatter_rejected(self):
        with self.assertRaises(MissionError):
            parse_mission("# just markdown\n", Path("x.md"))

    def test_no_repos_rejected(self):
        text = "+++\nslug = \"x\"\n+++\n\n# Body\n"
        with self.assertRaises(MissionError):
            parse_mission(text, Path("x.md"))

    def test_multiple_repos_rejected(self):
        text = ('+++\n[[repos]]\npath = "/a"\n[[repos]]\npath = "/b"\n+++\n'
                "\n# Body\n")
        with self.assertRaises(MissionError):
            parse_mission(text, Path("x.md"))

    def test_duplicate_lane_ids_rejected(self):
        repo = self.dir / "repo"
        path = helpers.write_mission(
            self.dir / "m.md", repo, lanes=[
                {"lid": "L1", "owns": '"a/**"'},
                {"lid": "L1", "owns": '"b/**"'},
            ])
        with self.assertRaises(MissionError):
            load_mission(path)

    def test_empty_owns_rejected(self):
        repo = self.dir / "repo"
        path = helpers.write_mission(
            self.dir / "m.md", repo, lanes=[{"lid": "L1", "owns": ""}])
        with self.assertRaises(MissionError):
            load_mission(path)

    def test_no_lanes_is_allowed(self):
        repo = self.dir / "repo"
        path = helpers.write_mission(self.dir / "m.md", repo, lanes=[])
        m = load_mission(path)
        self.assertEqual(m.lanes, [])


if __name__ == "__main__":
    unittest.main()

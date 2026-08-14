import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.mission import Lane  # noqa: E402
from src.worktrees import (check_lane_diff, find_overlaps,  # noqa: E402
                           glob_matches, globs_may_overlap, static_prefix)


class GlobMatchTest(unittest.TestCase):
    def test_double_star_crosses_directories(self):
        self.assertTrue(glob_matches("src/auth/**", "src/auth/session.rs"))
        self.assertTrue(glob_matches("src/auth/**", "src/auth/deep/x.rs"))
        self.assertFalse(glob_matches("src/auth/**", "src/api/x.rs"))

    def test_single_star_stays_in_segment(self):
        self.assertTrue(glob_matches("src/*", "src/x.rs"))
        self.assertFalse(glob_matches("src/*", "src/a/x.rs"))

    def test_leading_double_star(self):
        self.assertTrue(glob_matches("**/x.rs", "a/b/x.rs"))
        self.assertTrue(glob_matches("**/x.rs", "x.rs"))

    def test_full_tree_glob(self):
        self.assertTrue(glob_matches("**", "anything/at/all.txt"))

    def test_static_prefix(self):
        self.assertEqual(static_prefix("src/auth/**"), "src/auth")
        self.assertEqual(static_prefix("**"), "")
        self.assertEqual(static_prefix("src/x.rs"), "src/x.rs")


class OverlapTest(unittest.TestCase):
    def test_disjoint_dirs_do_not_overlap(self):
        self.assertFalse(globs_may_overlap("src/auth/**", "src/api/**"))

    def test_nested_globs_overlap(self):
        self.assertTrue(globs_may_overlap("src/**", "src/auth/**"))

    def test_exact_file_vs_dir_glob_overlap(self):
        self.assertTrue(globs_may_overlap("src/auth/**", "src/auth/session.rs"))

    def test_identical_globs_overlap(self):
        self.assertTrue(globs_may_overlap("a/**", "a/**"))

    def test_tracked_file_matching_both_overlaps(self):
        # Heuristic samples alone would miss this pair; the tracked file
        # matching both patterns proves the overlap.
        self.assertTrue(globs_may_overlap("src/*/x.rs", "src/a/*.rs",
                                          repo_files=["src/a/x.rs"]))

    def test_find_overlaps_across_lanes(self):
        lanes = [Lane(id="L1", owns=["src/auth/**"]),
                 Lane(id="L2", owns=["src/api/**"]),
                 Lane(id="L3", owns=["src/**"])]
        overlaps = find_overlaps(lanes)
        pairs = {(a, b) for a, b, _, _ in overlaps}
        self.assertIn(("L1", "L3"), pairs)
        self.assertIn(("L2", "L3"), pairs)
        self.assertNotIn(("L1", "L2"), pairs)


class LaneDiffCheckTest(unittest.TestCase):
    def test_owned_changes_pass(self):
        violations = check_lane_diff(
            ["src/example/a.md", "src/example/deep/b.md"],
            ["src/example/**"], [])
        self.assertEqual(violations, [])

    def test_forbidden_path_rejected(self):
        violations = check_lane_diff(
            ["src/api/routes.rs"], ["src/**"], ["src/api/**"])
        self.assertEqual(len(violations), 1)
        self.assertIn("forbidden", violations[0])

    def test_outside_owns_rejected(self):
        violations = check_lane_diff(
            ["README.md"], ["src/example/**"], [])
        self.assertEqual(len(violations), 1)
        self.assertIn("outside lane owns", violations[0])


if __name__ == "__main__":
    unittest.main()


class ContainmentCheckTest(unittest.TestCase):
    def test_drift_flags_new_paths_outside_missions(self):
        from src.worktrees import checkout_drift
        before = [".missions/run/state.json", "src/old.py"]
        after = [".missions/run/state.json", ".missions/run/report.md",
                 "src/old.py", "src/example/hello.py"]
        self.assertEqual(checkout_drift(before, after),
                         ["src/example/hello.py"])

    def test_drift_empty_when_only_missions_noise(self):
        from src.worktrees import checkout_drift
        self.assertEqual(checkout_drift(["a.py"], [".missions/r/x", "a.py"]),
                         [])

    def test_claimed_file_missing_from_diff_is_flagged(self):
        from src.worktrees import check_claimed_vs_diff
        violations = check_claimed_vs_diff(
            ["src/example/hello.py"], [])
        self.assertEqual(len(violations), 1)
        self.assertIn("hello.py", violations[0])

    def test_claimed_subset_of_diff_passes(self):
        from src.worktrees import check_claimed_vs_diff
        self.assertEqual(check_claimed_vs_diff(["a.py"], ["a.py", "b.py"]),
                         [])

import unittest
from pathlib import Path
import tempfile
import shutil

from src.mission import Mission, Repo, Lane, create_stage_mission
from src.verdicts import validate_stages, extract_planner_result, VerdictError
from src.orchestrator import Orchestrator
from src import worktrees


class StagesTest(unittest.TestCase):
    def test_validate_stages_valid(self):
        data = {
            "stages": [
                {"slug": "01-core", "brief": "Core types", "owns": ["src/types/**"], "contract_ids": ["AC-1"]},
                {"slug": "02-engine", "brief": "Engine", "owns": ["src/engine/**"], "contract_ids": ["AC-2"]},
            ]
        }
        stages = validate_stages(data, valid_contract_ids={"AC-1", "AC-2", "AC-3"})
        self.assertEqual(len(stages), 2)
        self.assertEqual(stages[0]["slug"], "01-core")
        self.assertEqual(stages[1]["slug"], "02-engine")

    def test_validate_stages_invalid_contract_id(self):
        data = {
            "stages": [
                {"slug": "01-core", "brief": "Core types", "owns": ["src/**"], "contract_ids": ["UNKNOWN-99"]},
            ]
        }
        with self.assertRaises(VerdictError):
            validate_stages(data, valid_contract_ids={"AC-1"})

    def test_extract_planner_result_detects_both_kinds(self):
        plan_text = "```gauntlet-plan\n{\"lanes\": [{\"id\": \"L1\", \"owns\": [\"a\"], \"brief\": \"b\"}]}\n```"
        kind1, data1 = extract_planner_result(plan_text)
        self.assertEqual(kind1, "lanes")
        self.assertEqual(data1[0]["id"], "L1")

        stage_text = "```gauntlet-stages\n{\"stages\": [{\"slug\": \"s1\", \"brief\": \"b\", \"owns\": [\"a\"]}]}\n```"
        kind2, data2 = extract_planner_result(stage_text)
        self.assertEqual(kind2, "stages")
        self.assertEqual(data2[0]["slug"], "s1")

    def test_create_stage_mission_retains_parent_invariants(self):
        tmp = Path(tempfile.mkdtemp())
        try:
            parent = Mission(
                slug="parent-epic",
                source_path=tmp / "parent.md",
                repos=[Repo(path=str(tmp), gates=["npm test"])],
                lanes=[],
                body="# Objective\nBig Epic\n## Invariants\n- INV-1: Never delete logs\n## Non-Goals\n- NG-1: No Rust",
                contract_ids={"INV-1", "NG-1"},
            )
            stage_spec = {
                "slug": "01-schema",
                "brief": "Implement DB schema",
                "owns": ["db/**"],
                "contract_ids": [],
            }
            sub_path = tmp / "sub.md"
            sub = create_stage_mission(parent, stage_spec, target_branch="master", path=sub_path)
            self.assertEqual(sub.slug, "parent-epic-01-schema")
            self.assertIn("INV-1: Never delete logs", sub.body)
            self.assertIn("NG-1: No Rust", sub.body)
            self.assertIn("parent-epic", sub.body)
            self.assertEqual(len(sub.lanes), 1)
            self.assertEqual(sub.lanes[0].id, "L1")
            self.assertEqual(sub.lanes[0].owns, ["db/**"])
        finally:
            shutil.rmtree(tmp)


if __name__ == "__main__":
    unittest.main()

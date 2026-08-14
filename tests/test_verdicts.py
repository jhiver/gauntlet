import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.verdicts import (VerdictError, extract_block,  # noqa: E402
                          validate_plan, validate_report, validate_verdict)

CONTRACT_IDS = {"AC-1", "AC-2", "INV-1", "NG-1"}


class ExtractionTest(unittest.TestCase):
    def test_extracts_last_matching_block(self):
        text = (
            "prose\n```gauntlet-verdict\n{\"groups\": [{\"root_cause\": "
            "\"old\", \"verdict\": \"DISMISS\"}]}\n```\nmore prose\n"
            "```gauntlet-verdict\n{\"groups\": []}\n```\ntrailing\n")
        data = extract_block(text, "verdict")
        self.assertEqual(data, {"groups": []})

    def test_ignores_other_block_kinds(self):
        text = "```gauntlet-report\n{\"files_changed\": [], \"tests_run\": [],"
        text += " \"tests_passed\": true, \"partial\": false, \"notes\": \"\"}\n```\n"
        with self.assertRaises(VerdictError):
            extract_block(text, "verdict")

    def test_missing_block_rejected(self):
        with self.assertRaises(VerdictError):
            extract_block("no blocks here", "report")

    def test_invalid_json_rejected(self):
        with self.assertRaises(VerdictError):
            extract_block("```gauntlet-verdict\n{not json}\n```", "verdict")

    def test_non_object_rejected(self):
        with self.assertRaises(VerdictError):
            extract_block("```gauntlet-verdict\n[1, 2]\n```", "verdict")


class ReportValidationTest(unittest.TestCase):
    def test_valid_report(self):
        report = validate_report({
            "files_changed": ["src/a.rs"], "tests_run": ["cargo test"],
            "tests_passed": True, "partial": False, "notes": "ok"})
        self.assertEqual(report["files_changed"], ["src/a.rs"])
        self.assertFalse(report["partial"])

    def test_missing_key_rejected(self):
        with self.assertRaises(VerdictError):
            validate_report({"files_changed": []})

    def test_wrong_type_rejected(self):
        with self.assertRaises(VerdictError):
            validate_report({
                "files_changed": "src/a.rs", "tests_run": [],
                "tests_passed": True, "partial": False, "notes": ""})
        with self.assertRaises(VerdictError):
            validate_report({
                "files_changed": [], "tests_run": [],
                "tests_passed": "yes", "partial": False, "notes": ""})


class VerdictValidationTest(unittest.TestCase):
    def _group(self, **kw):
        base = {"root_cause": "rc", "claims": ["c"], "contract_ids": ["AC-2"],
                "verdict": "FIX", "fix": "do x", "owns": "src/a.rs"}
        base.update(kw)
        return base

    def test_valid_groups(self):
        groups = validate_verdict({"groups": [self._group()]}, CONTRACT_IDS)
        self.assertEqual(len(groups), 1)
        self.assertTrue(groups[0].actionable)
        self.assertEqual(groups[0].contract_ids, ["AC-2"])

    def test_empty_groups_is_valid_no_claims(self):
        self.assertEqual(validate_verdict({"groups": []}, CONTRACT_IDS), [])

    def test_bad_verdict_enum_rejected(self):
        with self.assertRaises(VerdictError):
            validate_verdict({"groups": [self._group(verdict="MAYBE")]},
                             CONTRACT_IDS)

    def test_unknown_contract_id_rejected(self):
        with self.assertRaises(VerdictError):
            validate_verdict(
                {"groups": [self._group(contract_ids=["AC-99"])]},
                CONTRACT_IDS)

    def test_missing_root_cause_rejected(self):
        with self.assertRaises(VerdictError):
            validate_verdict({"groups": [self._group(root_cause="")]},
                             CONTRACT_IDS)

    def test_actionable_verdicts(self):
        groups = validate_verdict({"groups": [
            self._group(verdict="FIX"),
            self._group(verdict="REDESIGN"),
            self._group(verdict="REPORT_ONLY"),
            self._group(verdict="DISMISS"),
        ]}, CONTRACT_IDS)
        self.assertEqual([g.actionable for g in groups],
                         [True, True, False, False])

    def test_class_defaults_to_code_defect(self):
        groups = validate_verdict({"groups": [self._group()]}, CONTRACT_IDS)
        self.assertEqual(groups[0].defect_class, "code_defect")
        self.assertTrue(groups[0].blocking)
        self.assertFalse(groups[0].polish)

    def test_unknown_class_rejected(self):
        with self.assertRaises(VerdictError):
            validate_verdict({"groups": [self._group(**{"class": "style"})]},
                             CONTRACT_IDS)

    def test_doc_and_evidence_classes_are_polish_not_blocking(self):
        groups = validate_verdict({"groups": [
            self._group(**{"class": "doc_drift"}),
            self._group(**{"class": "evidence_gap"}),
        ]}, CONTRACT_IDS)
        self.assertEqual([g.blocking for g in groups], [False, False])
        self.assertEqual([g.polish for g in groups], [True, True])

    def test_redesign_blocks_whatever_its_class(self):
        groups = validate_verdict(
            {"groups": [self._group(verdict="REDESIGN",
                                    **{"class": "doc_drift"})]},
            CONTRACT_IDS)
        self.assertTrue(groups[0].blocking)

    def test_non_actionable_verdicts_are_neither_blocking_nor_polish(self):
        groups = validate_verdict({"groups": [
            self._group(verdict="REPORT_ONLY", **{"class": "doc_drift"}),
            self._group(verdict="DISMISS"),
        ]}, CONTRACT_IDS)
        self.assertEqual([g.blocking for g in groups], [False, False])
        self.assertEqual([g.polish for g in groups], [False, False])


class PlanValidationTest(unittest.TestCase):
    def test_valid_plan(self):
        lanes = validate_plan({"lanes": [{
            "id": "F1", "owns": ["src/auth/**"], "forbidden": [],
            "tests": ["t"], "brief": "b", "addresses": ["rc"]}]})
        self.assertEqual(lanes[0]["id"], "F1")
        self.assertEqual(lanes[0]["addresses"], ["rc"])

    def test_empty_owns_rejected(self):
        with self.assertRaises(VerdictError):
            validate_plan({"lanes": [{"id": "F1", "owns": []}]})

    def test_empty_lane_list_rejected(self):
        with self.assertRaises(VerdictError):
            validate_plan({"lanes": []})

    def test_duplicate_lane_ids_rejected(self):
        with self.assertRaises(VerdictError):
            validate_plan({"lanes": [
                {"id": "F1", "owns": ["a/**"]},
                {"id": "F1", "owns": ["b/**"]},
            ]})

    def test_defaults_for_optional_fields(self):
        lanes = validate_plan({"lanes": [{"id": "F1", "owns": ["a/**"]}]})
        self.assertEqual(lanes[0]["forbidden"], [])
        self.assertEqual(lanes[0]["tests"], [])
        self.assertEqual(lanes[0]["brief"], "")


if __name__ == "__main__":
    unittest.main()

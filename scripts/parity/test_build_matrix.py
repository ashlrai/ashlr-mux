import unittest

from build_matrix import (
    ALLOWED_STATUSES,
    apply_overrides,
    ratio,
    validate_entries,
    windows_cli_is_implemented,
)


def entry(status="missing"):
    return {
        "id": "cli:test",
        "canonical_sources": [{"path": "CLI/cmux.swift"}],
        "acceptance_tests": ["differential.cli.test"],
        "dependencies": [],
        "status": status,
        "latest_verifying_commit": None,
        "rationale": None,
        "user_approval": None,
    }


class MatrixBuilderTests(unittest.TestCase):
    def test_windows_source_presence_never_claims_verified(self):
        evidence = {"top_level_known": True, "executor": "local", "control_methods": []}
        self.assertTrue(windows_cli_is_implemented(evidence))
        self.assertIn("implemented_unverified", ALLOWED_STATUSES)

    def test_conditional_control_mapping_is_implemented_but_unverified(self):
        evidence = {
            "top_level_known": True,
            "dispatch_outcome": "control_mapping",
            "control_mapping_kind": "conditional",
            "control_methods": [],
        }
        self.assertTrue(windows_cli_is_implemented(evidence))
        evidence["dispatch_outcome"] = "explicit_socket_command_not_ported"
        self.assertFalse(windows_cli_is_implemented(evidence))

    def test_resolved_status_requires_commit_and_equivalence_rationale(self):
        with self.assertRaisesRegex(ValueError, "verifying commit"):
            validate_entries([entry("verified")])
        value = entry("platform_equivalent")
        value["latest_verifying_commit"] = "a" * 40
        with self.assertRaisesRegex(ValueError, "rationale"):
            validate_entries([value])

    def test_not_applicable_requires_explicit_user_approval(self):
        value = entry("not_applicable")
        value["rationale"] = "not present on Windows"
        with self.assertRaisesRegex(ValueError, "user approval"):
            validate_entries([value])

    def test_unknown_override_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "unknown entries"):
            apply_overrides([entry()], {"entries": {"cli:missing": {"status": "red"}}})

    def test_dangling_dependency_is_rejected(self):
        value = entry()
        value["dependencies"] = ["v2:not.canonical"]
        with self.assertRaisesRegex(ValueError, "unknown dependencies"):
            validate_entries([value])

    def test_ratio_reports_numerator_and_denominator(self):
        self.assertEqual(ratio(1, 4), {"numerator": 1, "denominator": 4, "percentage": 25.0})


if __name__ == "__main__":
    unittest.main()

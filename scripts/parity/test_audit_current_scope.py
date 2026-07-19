#!/usr/bin/env python3
"""Focused tests for current parity audit metadata layering."""

import unittest

from audit_current_scope import merged_overrides


class CurrentAuditOverrideTests(unittest.TestCase):
    def test_current_entries_layer_without_mutating_frozen_entries(self):
        frozen = {
            "schema_version": 1,
            "entries": {"v2:existing": {"status": "verified"}},
        }
        current = {
            "schema_version": 1,
            "entries": {
                "v2:existing": {"latest_verifying_commit": "a" * 40},
                "v2:new": {"status": "platform_equivalent"},
            },
        }

        merged = merged_overrides(frozen, current)

        self.assertEqual(
            merged["entries"]["v2:existing"],
            {"status": "verified", "latest_verifying_commit": "a" * 40},
        )
        self.assertEqual(
            merged["entries"]["v2:new"], {"status": "platform_equivalent"}
        )
        self.assertEqual(
            frozen["entries"]["v2:existing"], {"status": "verified"}
        )


if __name__ == "__main__":
    unittest.main()

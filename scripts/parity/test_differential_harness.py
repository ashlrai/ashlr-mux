import unittest

from differential_harness import OBSERVATION_KEYS, compare_observations, remove_pointer


def observation(**changes):
    value = {key: None for key in OBSERVATION_KEYS}
    value.update(
        {
            "exit_status": 0,
            "stdout": "OK\n",
            "stderr": "",
            "error": None,
            "response": {"id": "stable", "platform": "canonical"},
            "state": {},
            "events": [],
            "persistence": {},
            "selectors": {},
            "multiwindow": {},
        }
    )
    value.update(changes)
    return value


class DifferentialHarnessTests(unittest.TestCase):
    def test_compares_all_required_lanes(self):
        right = observation(events=[{"name": "workspace.selected"}])
        self.assertEqual(compare_observations(observation(), right, {}), ["events"])

    def test_approved_pointer_requires_rationale_and_normalizes_both_sides(self):
        right = observation(response={"id": "stable", "platform": "windows"})
        case = {
            "approved_differences": [
                {"path": "/response/platform", "rationale": "platform label"}
            ]
        }
        self.assertEqual(compare_observations(observation(), right, case), [])
        with self.assertRaises(ValueError):
            compare_observations(
                observation(), right, {"approved_differences": [{"path": "/response/platform"}]}
            )

    def test_json_pointer_removal_supports_escaped_object_keys(self):
        value = {"a/b": {"~key": 1, "keep": 2}}
        remove_pointer(value, "/a~1b/~0key")
        self.assertEqual(value, {"a/b": {"keep": 2}})

    def test_missing_observation_lane_is_rejected(self):
        right = observation()
        del right["persistence"]
        with self.assertRaisesRegex(ValueError, "persistence"):
            compare_observations(observation(), right, {})


if __name__ == "__main__":
    unittest.main()

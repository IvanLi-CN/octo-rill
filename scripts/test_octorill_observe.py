#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
LOADER = importlib.machinery.SourceFileLoader("octorill_observe", str(ROOT / "scripts" / "octorill-observe"))
SPEC = importlib.util.spec_from_loader("octorill_observe", LOADER)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class FakeClient:
    def __init__(self):
        self.expressions = []

    def query_range(self, expression, start, end, step):
        self.expressions.append(expression)
        return {"status": "success", "data": {"resultType": "matrix", "result": [{"metric": {}, "values": [[start, "2"], [end, "3"]]}]}}


class ObserveContractTest(unittest.TestCase):
    def args(self):
        return type("Args", (), {"service": "octorill", "environment": "test", "instance": "one"})()

    def test_catalog_commands_are_fixed_and_queries_are_bounded(self):
        catalog = MODULE.load_catalog()
        self.assertEqual(set(catalog["commands"]), {"overview", "http", "sqlite", "queues", "upstream", "health"})
        self.assertNotIn("raw", catalog["commands"])
        self.assertTrue(all("octorill_" in expression for expression in catalog["queries"].values()))

    def test_query_range_replaces_only_fixed_selector_and_window(self):
        client = FakeClient()
        catalog = MODULE.load_catalog()
        start, end, step = 1000.0, 1900.0, 15
        results = MODULE.run_queries(client, catalog, ["http_rate"], self.args(), start, end, step)
        self.assertTrue(results["http_rate"]["ok"])
        self.assertIn('service=~"octorill"', client.expressions[0])
        self.assertIn("[60s]", client.expressions[0])
        self.assertNotIn("SERVICE_SELECTOR", client.expressions[0])

    def test_window_limit_is_enforced(self):
        args = self.args()
        args.to = None
        args.from_time = None
        args.window_minutes = 24 * 60 + 1
        args.step = 15
        with self.assertRaises(ValueError):
            MODULE.window(args)

    def test_non_finite_values_are_reported_as_missing_samples(self):
        summary = MODULE.summarize(
            {
                "data": {
                    "result": [
                        {"values": [[1000, "NaN"], [1015, "Infinity"]]},
                        {"values": [[1000, "2.5"], [1015, "3.5"]]},
                    ]
                }
            }
        )
        self.assertEqual(summary["series"], 2)
        self.assertEqual(summary["samples"], 2)
        self.assertEqual(summary["latest"], [3.5])
        self.assertTrue(summary["has_samples"])
        self.assertEqual(summary["last_sample_at"], 1015.0)

        missing = MODULE.summarize({"data": {"result": [{"values": [[1000, "NaN"]]}]}})
        self.assertEqual(missing["samples"], 0)
        self.assertEqual(missing["latest"], [])
        self.assertFalse(missing["has_samples"])
        self.assertIsNone(missing["last_sample_at"])

    def test_compare_accepts_explicit_baseline_window(self):
        args = self.args()
        args.baseline_from = "1970-01-01T00:00:00Z"
        args.baseline_to = "1970-01-01T00:10:00Z"
        start, end, step = MODULE.baseline_window(args, 2000.0, 2600.0, 15)
        self.assertEqual((start, end), (0.0, 600.0))
        self.assertEqual(step, 15)

    def test_compare_rejects_partial_baseline_window(self):
        args = self.args()
        args.baseline_from = "1970-01-01T00:00:00Z"
        args.baseline_to = None
        with self.assertRaises(ValueError):
            MODULE.baseline_window(args, 2000.0, 2600.0, 15)


if __name__ == "__main__":
    unittest.main()

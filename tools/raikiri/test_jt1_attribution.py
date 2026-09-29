"""Unit tests for the shodo-jt1 attribution helpers, on real probe records."""
import gzip
import importlib.util
import json
import unittest
from pathlib import Path

FIXTURES = Path(__file__).with_name("fixtures") / "raikiri"


def load_module():
    path = Path(__file__).with_name("jt1_attribution.py")
    spec = importlib.util.spec_from_file_location("jt1_attribution", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def record(name):
    return json.loads(gzip.decompress((FIXTURES / f"{name}.json.gz").read_bytes()))


class Attribution(unittest.TestCase):
    def setUp(self):
        self.m = load_module()

    def test_first_call_is_excluded(self):
        report = record("pipeline-native-time")
        warm = self.m.warm_samples_ns(report, "pipeline")
        self.assertEqual(len(warm), len(report["samples"]) - 1)
        first = report["samples"][0]["parse_cascade_layout"]["duration_ns"]
        self.assertEqual(warm, [s["parse_cascade_layout"]["duration_ns"] for s in report["samples"][1:]])
        self.assertNotIn("first-call-in-process", [s["state"] for s in report["samples"][1:]])
        self.assertIsInstance(first, int)

    def test_process_median_is_the_median_of_warm_calls(self):
        report = record("pipeline-candidate-time")
        warm = sorted(self.m.warm_samples_ns(report, "pipeline"))
        self.assertEqual(self.m.process_median_ns(report, "pipeline"), warm[len(warm) // 2] if len(warm) % 2 else (warm[len(warm) // 2 - 1] + warm[len(warm) // 2]) / 2)

    def test_layout_operation_uses_the_layout_window(self):
        report = record("layout-native")
        # memory-mode fixture: the window object exists for every sample
        self.assertTrue(all("layout" in s for s in report["samples"]))
        with self.assertRaises(ValueError):
            self.m.warm_samples_ns({"samples": [{"state": "warm-process"}]}, "layout")

    def test_wrong_state_layout_is_rejected(self):
        with self.assertRaises(ValueError):
            self.m.warm_samples_ns({"samples": [{"state": "warm-process", "layout": {"duration_ns": 1}}, {"state": "warm-process", "layout": {"duration_ns": 2}}]}, "layout")

    def test_pairs_are_matched_by_repeat(self):
        summary = self.m.paired_summary([100.0, 200.0, 300.0], [150.0, 200.0, 240.0])
        self.assertEqual(summary["paired_ratios"], [1.5, 1.0, 0.8])
        self.assertEqual(summary["pairs"], 3)
        self.assertEqual(summary["paired_ratio_median"], 1.0)
        self.assertEqual(summary["pairs_candidate_slower"], 1)
        self.assertEqual(summary["warm_process_median_ns"], {"native": 200.0, "candidate": 200.0})

    def test_unequal_or_empty_pairs_are_an_error(self):
        with self.assertRaises(ValueError):
            self.m.paired_summary([1.0], [1.0, 2.0])
        with self.assertRaises(ValueError):
            self.m.paired_summary([], [])

    def test_perf_rows_ignore_non_sample_lines(self):
        text = "\n".join([
            "# Samples: 4K of event 'cycles'",
            "#",
            "     120  [.] <parley::bidi::BidiResolver>::resolve::<X>",
            "      30  [.] shodo::paragraph::build::run",
            "       5  [k] some_kernel_symbol",
            "",
            "garbage line",
        ])
        rows = self.m.perf_samples(text)
        self.assertEqual(rows, [(120, "<parley::bidi::BidiResolver>::resolve::<X>"), (30, "shodo::paragraph::build::run")])

    def test_buckets(self):
        b = self.m.bucket
        self.assertEqual(b("shodo::line::scan::next"), "shodo::line")
        self.assertEqual(b("<parley::bidi::BidiResolver>::resolve::<X>"), "parley")
        self.assertEqual(b("raikiri_style::cascade::apply"), "raikiri_style")
        self.assertEqual(b("core::slice::sort::merge"), "runtime")
        self.assertEqual(b("malloc"), "runtime")
        self.assertEqual(b("totally_unknown_symbol"), "other")

    def test_every_sample_lands_in_exactly_one_bucket(self):
        rows = [(10, "shodo::line::a"), (7, "parley::x"), (3, "malloc"), (2, "mystery")]
        totals = self.m.aggregate(rows)
        self.assertEqual(sum(totals.values()), 22)
        self.assertEqual(totals, {"shodo::line": 10, "parley": 7, "runtime": 3, "other": 2})


if __name__ == "__main__":
    unittest.main()

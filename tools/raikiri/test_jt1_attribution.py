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

    @staticmethod
    def synthetic(first, warm):
        """first: dict of window -> duration; warm: list of such dicts."""
        def sample(state, windows):
            out = {"state": state}
            for key, ns in windows.items():
                out[key] = {"duration_ns": ns}
            return out
        return {"samples": [sample("first-call-in-process", first)] + [sample("warm-process", w) for w in warm]}

    def test_first_call_is_excluded(self):
        report = record("pipeline-native-time")
        warm = self.m.warm_samples_ns(report, "pipeline")
        self.assertEqual(len(warm), len(report["samples"]) - 1)
        self.assertEqual(warm, [s["parse_cascade_layout"]["duration_ns"] for s in report["samples"][1:]])
        rep = self.synthetic({"parse_cascade_layout": 987654321}, [{"parse_cascade_layout": 7}, {"parse_cascade_layout": 9}])
        self.assertEqual(self.m.warm_samples_ns(rep, "pipeline"), [7, 9])
        self.assertNotIn(987654321, self.m.warm_samples_ns(rep, "pipeline"))

    def test_process_median_is_the_median_of_warm_calls(self):
        odd = self.synthetic({"parse_cascade_layout": 10**9}, [{"parse_cascade_layout": v} for v in (5, 1, 3)])
        self.assertEqual(self.m.process_median_ns(odd, "pipeline"), 3)
        even = self.synthetic({"parse_cascade_layout": 10**9}, [{"parse_cascade_layout": v} for v in (2, 8, 4, 6)])
        self.assertEqual(self.m.process_median_ns(even, "pipeline"), 5.0)

    def test_windows_map_to_their_own_durations(self):
        self.assertEqual(
            self.m.WINDOWS,
            {"pipeline": "parse_cascade_layout", "layout": "layout", "isolated": "initial_text_pipeline"},
        )
        rep = self.synthetic(
            {"parse_cascade_layout": 1000, "layout": 2000, "initial_text_pipeline": 3000},
            [
                {"parse_cascade_layout": 11, "layout": 21, "initial_text_pipeline": 31},
                {"parse_cascade_layout": 12, "layout": 22, "initial_text_pipeline": 32},
            ],
        )
        self.assertEqual(self.m.warm_samples_ns(rep, "pipeline"), [11, 12])
        self.assertEqual(self.m.warm_samples_ns(rep, "layout"), [21, 22])
        self.assertEqual(self.m.warm_samples_ns(rep, "isolated"), [31, 32])

    def test_malformed_state_sequences_are_rejected(self):
        window = {"pipeline": "parse_cascade_layout", "layout": "layout", "isolated": "initial_text_pipeline"}
        for operation, key in window.items():
            first = {"state": "first-call-in-process", key: {"duration_ns": 1}}
            warm = {"state": "warm-process", key: {"duration_ns": 2}}
            with self.assertRaises(ValueError):
                self.m.warm_samples_ns({"samples": [warm, dict(warm)]}, operation)  # no first call
            with self.assertRaises(ValueError):
                self.m.warm_samples_ns({"samples": [first, warm, {"state": "cold-process", key: {"duration_ns": 3}}]}, operation)  # later bad state
            with self.assertRaises(ValueError):
                self.m.warm_samples_ns({"samples": [first]}, operation)  # no warm samples

    def test_pairs_are_matched_by_repeat(self):
        summary = self.m.paired_summary([100.0, 200.0, 300.0], [240.0, 200.0, 150.0])
        self.assertEqual(summary["paired_ratios"], [2.4, 1.0, 0.5])
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

    def test_perf_rows_parse_real_perf_report_lines(self):
        # Verbatim shape of `perf report --stdio --no-children -g none --fields sample,sym --sort sym`.
        text = "\n".join([
            "# Samples: 425  of event 'cpu/cycles/Pu'",
            "# Overhead       Samples  Symbol",
            "            10  [.] <<serde_json::value::Value as serde_core::de::Deserialize>::deserialize::ValueVisitor as serde_core::de::Visitor>::visit_map",
            "            16  [.] cssparser::tokenizer::consume_comment",
            "             1  [.] core::unicode::unicode_data::conversions::to_lower",
        ])
        rows = self.m.perf_samples(text)
        self.assertEqual([c for c, _ in rows], [10, 16, 1])
        self.assertEqual(rows[1][1], "cssparser::tokenizer::consume_comment")
        self.assertEqual(self.m.bucket(rows[1][1]), "cssparser")

    def test_window_memory_on_real_memory_fixtures(self):
        # Hand-read from the fixtures: the four asserted fields are identical in every warm sample of these two records (live_bytes and start_live_bytes vary, and are not asserted).
        native = self.m.window_memory(record("pipeline-native"), "pipeline")
        candidate = self.m.window_memory(record("pipeline-candidate"), "pipeline")
        self.assertEqual(native, {"allocated_bytes": 558186, "calls": 1350, "peak_extra_bytes": 157847, "net_bytes": 50927, "warm_samples": 9})
        self.assertEqual(candidate, {"allocated_bytes": 490338, "calls": 1302, "peak_extra_bytes": 157847, "net_bytes": 46895, "warm_samples": 9})

    def test_window_memory_takes_median_and_skips_first_call(self):
        def sample(state, alloc):
            return {"state": state, "layout": {"counts": {"allocated_bytes": alloc, "calls": 1, "peak_extra_bytes": 2, "net_bytes": 3}}}
        report = {"samples": [sample("first-call-in-process", 999), sample("warm-process", 10), sample("warm-process", 30), sample("warm-process", 20)]}
        self.assertEqual(self.m.window_memory(report, "layout")["allocated_bytes"], 20)
        with self.assertRaises(ValueError):
            self.m.window_memory({"samples": [sample("warm-process", 1)]}, "layout")

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

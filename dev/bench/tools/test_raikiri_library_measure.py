"""Reject missing real construction work and altered original source evidence."""
import copy
import gzip
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
FIXTURES = HERE / "fixtures/raikiri"


class LibraryTests(unittest.TestCase):
    def setUp(self):
        path = HERE / "raikiri_library_measure.py"
        self.assertTrue(path.is_file(), "library collector/validator missing")
        spec = importlib.util.spec_from_file_location("library_measure", path)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.cases = {c["id"]: c for c in json.loads((FIXTURES / "selection.json").read_text())["cases"]}

    def record(self, engine="candidate"):
        return json.loads(gzip.decompress((FIXTURES / f"library-{engine}.json.gz").read_bytes()))

    def validate(self, record, engine="candidate", mode="memory"):
        reference = json.loads(gzip.decompress((FIXTURES / f"isolated-{engine}-time-reference.json.gz").read_bytes()))
        self.module.validate(record, self.cases[record["id"]], mode, engine, reference)

    def test_real_fresh_construction_records_match_independent_outputs_and_balance(self):
        for engine in ["candidate", "native"]:
            self.validate(self.record(engine), engine)

    def test_deleted_library_phase_and_forged_pair_mapping_are_rejected(self):
        for mutate in [lambda v: v["samples"][0]["exclusive_phases"].pop(1),
                       lambda v: v["samples"][0]["exclusive_phases"][1].update(paired_root=None)]:
            value = self.record()
            mutate(value)
            with self.assertRaises(ValueError):
                self.validate(value)

    def test_lost_source_output_and_different_cold_warm_input_are_rejected(self):
        for mutate in [lambda v: v["samples"][0].update(output=[]),
                       lambda v: v["samples"][1]["exclusive_phases"][1].update(input_sha256="0" * 64)]:
            value = self.record()
            mutate(value)
            with self.assertRaises(ValueError):
                self.validate(value)

    def test_missing_owner_release_and_unbalanced_cache_cannot_pass(self):
        for mutate in [lambda v: v.pop("release_font_registry"),
                       lambda v: v["release_input_geometry"]["counts"].update(net_bytes=1)]:
            value = self.record()
            mutate(value)
            with self.assertRaises(ValueError):
                self.validate(value)

    def test_instrumented_records_and_negative_peak_cannot_be_time_results(self):
        value = self.record()
        value["mode"] = "time"
        with self.assertRaises(ValueError):
            self.validate(value, mode="time")
        value = self.record()
        value["samples"][0]["exclusive_phases"][1]["measurement"]["counts"]["peak_extra_bytes"] = -1
        with self.assertRaises(ValueError):
            self.validate(value)

    def test_input_drift_or_absent_library_limits_are_rejected(self):
        for mutate in [lambda v: v["font_sha256"].reverse(),
                       lambda v: v.pop("library_input_semantics"),
                       lambda v: v.update(pure_glyph_shaping_ratio=1.0)]:
            value = self.record()
            mutate(value)
            with self.assertRaises(ValueError):
                self.validate(value)


if __name__ == "__main__":
    unittest.main()

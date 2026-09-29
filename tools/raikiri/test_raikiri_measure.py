"""Mutate unchanged real probe records at the report acceptance boundary."""
import copy
import gzip
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

FIXTURES = Path(__file__).with_name("fixtures") / "raikiri"


def record(operation, engine="candidate"):
    return json.loads(gzip.decompress((FIXTURES / f"{operation}-{engine}.json.gz").read_bytes()))


class RecordTests(unittest.TestCase):
    def setUp(self):
        path = Path(__file__).with_name("raikiri_measure.py")
        self.assertTrue(path.is_file(), "real record validator/orchestrator is not implemented")
        spec = importlib.util.spec_from_file_location("raikiri_measure", path)
        self.runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.runner)
        self.cases = {c["id"]: c for c in json.loads((FIXTURES / "selection.json").read_text())["cases"]}

    def validate(self, value, operation, *, reference=None, mode="memory", engine="candidate", comparison=False):
        self.runner.validate_record(value, self.cases[value["id"]], operation, mode, engine,
                                    reference=reference, for_comparison=comparison)

    def test_all_real_memory_records_are_valid_with_their_original_output_reference(self):
        for operation in ["layout", "pipeline", "isolated", "pagination"]:
            for engine in ["native", "candidate"]:
                with self.subTest(operation=operation, engine=engine):
                    value = record(operation, engine)
                    self.validate(value, operation, engine=engine, reference=value)

    def test_font_resource_warning_and_viewport_drift_are_rejected(self):
        original = record("pipeline")
        mutations = [lambda r: r["font_sha256"].reverse(),
                     lambda r: r["resources"][0].update(sha256="0" * 64),
                     lambda r: r["parse_warnings"].append("new warning"),
                     lambda r: r.update(viewport_css_px=[400, 600])]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                value = copy.deepcopy(original)
                mutate(value)
                with self.assertRaises(ValueError):
                    self.validate(value, "pipeline", reference=original)

    def test_allocator_results_cannot_be_accepted_as_uninstrumented_time(self):
        value = record("pipeline")
        value["mode"] = "time"
        with self.assertRaises(ValueError):
            self.validate(value, "pipeline", mode="time", comparison=True)

    def test_missing_measurement_and_owner_release_are_rejected(self):
        original = record("isolated")
        for field in ["initial_text_pipeline", "release_prepared_owner", "mutable_reuse_owner_setup"]:
            with self.subTest(field=field):
                value = copy.deepcopy(original)
                del value["samples"][0][field]
                with self.assertRaises(ValueError):
                    self.validate(value, "isolated", reference=original)

    def test_shortened_accepted_source_cannot_pass_against_original_reference(self):
        original = record("isolated")
        value = copy.deepcopy(original)
        value["initial_output"][0]["processed_source_bytes"] -= 1
        with self.assertRaises(ValueError):
            self.validate(value, "isolated", reference=original)

    def test_reused_root_width_drift_is_rejected(self):
        value = record("isolated")
        value["root_content_width_css_px"][0]["width"] += 1
        with self.assertRaises(ValueError):
            self.validate(value, "isolated")

    def test_restored_width_or_retry_output_loss_is_rejected(self):
        original = record("isolated")
        value = copy.deepcopy(original)
        value["samples"][0]["reused_widths_and_retries"][-1]["output"] = []
        with self.assertRaises(ValueError):
            self.validate(value, "isolated", reference=original)

    def test_negative_peak_and_unbalanced_real_cache_ownership_are_rejected(self):
        original = record("pagination")
        for field, delta in [("peak_extra_bytes", -1), ("net_bytes", 697)]:
            with self.subTest(field=field):
                value = copy.deepcopy(original)
                counts = value["release_font_registry"]["counts"]
                counts[field] = delta if field == "peak_extra_bytes" else counts[field] + delta
                with self.assertRaises(ValueError):
                    self.validate(value, "pagination", reference=original)

    def test_missing_fragments_and_false_source_completeness_are_rejected(self):
        original = record("pagination")
        for mutate in [lambda r: r["samples"][0]["complete_caller_height_reentry"][1]["output"].update(source_partition_complete=False),
                       lambda r: r["samples"][0]["complete_caller_height_reentry"][1]["output"].update(partition=[])]:
            value = copy.deepcopy(original)
            mutate(value)
            with self.assertRaises(ValueError):
                self.validate(value, "pagination", reference=original)

    def test_known_different_paged_width_and_partitions_cannot_form_a_ratio(self):
        native, candidate = record("pagination", "native"), record("pagination")
        with self.assertRaises(ValueError):
            self.runner.validate_pair(native, candidate, self.cases[candidate["id"]], "pagination", "memory")

    def test_correctness_only_work_cannot_be_used_as_a_timing_measurement(self):
        value = record("pagination")
        with self.assertRaises(ValueError):
            self.validate(value, "pagination", comparison=True)

    def test_real_uninstrumented_release_time_matches_fresh_same_engine_output(self):
        for operation in ["pipeline", "isolated"]:
            for engine in ["native", "candidate"]:
                with self.subTest(operation=operation, engine=engine):
                    value = record(operation, f"{engine}-time")
                    reference = record(operation, f"{engine}-time-reference")
                    self.validate(value, operation, engine=engine, mode="time", reference=reference, comparison=True)

    def test_actual_cargo_artifact_cannot_be_mislabelled_when_features_or_profile_differ(self):
        self.assertTrue(callable(getattr(self.runner, "validate_build", None)), "actual build provenance validator is missing")
        original = json.loads((FIXTURES / "time-build-artifact.json").read_text())
        self.runner.validate_build(original, "time")
        for mutate in [lambda r: r.update(features=["allocation-counting"]),
                       lambda r: r["profile"].update(opt_level="0"),
                       lambda r: r["profile"].update(debug_assertions=True)]:
            with self.subTest(mutation=mutate):
                value = copy.deepcopy(original)
                mutate(value)
                with self.assertRaises(ValueError):
                    self.runner.validate_build(value, "time")

    def test_cli_accepts_real_records_and_rejects_input_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "record.json"
            value = record("pipeline")
            argv = [sys.executable, str(Path(__file__).with_name("raikiri_measure.py")),
                    "--validate-record", str(path), "--selection", str(FIXTURES / "selection.json"),
                    "--operation", "pipeline", "--mode", "memory", "--engine", "candidate"]
            path.write_text(json.dumps(value))
            good = subprocess.run(argv, capture_output=True, text=True)
            self.assertEqual(good.returncode, 0, good.stderr)
            self.assertTrue(json.loads(good.stdout)["valid"])
            value["font_sha256"].reverse()
            path.write_text(json.dumps(value))
            bad = subprocess.run(argv, capture_output=True, text=True)
            self.assertNotEqual(bad.returncode, 0)
            self.assertEqual(bad.stdout, "")

    def test_fixed_commit_archive_preserves_an_advanced_dirty_checkout(self):
        # Reading HEAD/working files would borrow another task's source instead
        # of the requested immutable commit. Exercise real git objects/archives.
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary) / "source"
            repo.mkdir()
            def git(*argv):
                return subprocess.check_output(["git", *argv], cwd=repo, stderr=subprocess.DEVNULL)
            git("init")
            (repo / "dev/raikiri/src").mkdir(parents=True)
            page = repo / "dev/raikiri/src/candidate_page.rs"
            original = b"// original immutable fixture\n"
            page.write_bytes(original)
            (repo / ".gitignore").write_text("Cargo.lock\n")
            git("add", ".")
            git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "original")
            revision = git("rev-parse", "HEAD").decode().strip()
            page.write_text("// newer committed source\n")
            git("add", ".")
            git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "newer")
            page.write_text("// another task's current uncommitted source\n")
            (repo / "untracked.txt").write_text("keep")
            lock = repo / "Cargo.lock"
            lock.write_text("version = 4\n")
            before = (git("rev-parse", "HEAD"), git("status", "--porcelain"), page.read_bytes(), lock.read_bytes())
            import hashlib
            with patch.object(self.runner.overlay, "S4_REVISION", revision), \
                 patch.object(self.runner.overlay, "PAGE_SOURCE_SHA256", hashlib.sha256(original).hexdigest()), \
                 patch.object(self.runner.overlay, "LOCK_SOURCE_SHA256", hashlib.sha256(lock.read_bytes()).hexdigest(), create=True):
                self.runner.overlay.prepare(repo, Path(temporary) / "archive", expose_layout_boundary=False)
            self.assertEqual((Path(temporary) / "archive/s4/dev/raikiri/src/candidate_page.rs").read_bytes(), original)
            self.assertEqual(before, (git("rev-parse", "HEAD"), git("status", "--porcelain"), page.read_bytes(), lock.read_bytes()))


if __name__ == "__main__":
    unittest.main()

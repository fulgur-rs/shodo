import copy
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("bench_runner", Path(__file__).with_name("run.py"))
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)
OPS = ["build", "next_line", "all_lines", "intrinsic", "reuse_widths", "rebuild_widths", "page_retry"]
PHASES = ["context_init", "font_initialization_registration", "build", "all_lines"]
SCOPES = ["font_context_init", "build", "plain_lines", "release_plain_lines", "justify_lines", "release_justify_lines", "reuse_widths", "release_reuse", "rebuild_widths", "release_rebuild", "page_retry", "release_pages", "intrinsic", "release_intrinsic", "drop_paragraphs", "context_shrink_zero", "drop_context", "drop_fonts"]

def valid_report():
    digest = dict(sha256="a" * 64, lines=2, glyphs=10, runs=2, synthetic_glyphs=0, float_reports=0, height_retries=0,intrinsic_measurements=0)
    key = "latin-short/1"
    settings = dict(id="latin-short", scale=1, width=320, text="fixed input",paragraphs=1)
    counts = dict(calls=1, allocated_bytes=16, deallocated_bytes=0, start_live_bytes=32, live_bytes=48, peak_extra_bytes=16, net_bytes=16)
    memory = dict(schema=1, mode="memory", instrumented=True, settings=settings, scopes=[dict(name=name, counts=copy.deepcopy(counts)) for name in SCOPES], digests={k:copy.deepcopy(digest) for k in ["default", "plain", "justify", "reuse", "rebuild", "pages", "intrinsic"]})
    cold = dict(schema=1, mode="cold", instrumented=False, settings=settings, durations_ns=dict(zip(PHASES,[1,2,3,4])), digest=digest)
    memory["digests"]["pages"]["height_retries"]=2
    memory["digests"]["intrinsic"].update(lines=0,glyphs=0,runs=0,intrinsic_measurements=1)
    timing = {op:dict(digest=copy.deepcopy(digest), median_ns=100, sample=dict(iters=[1]*10,times=[100]*10)) for op in OPS}
    timing["page_retry"]["digest"]["height_retries"]=2
    timing["intrinsic"]["digest"].update(lines=0,glyphs=0,runs=0,intrinsic_measurements=1)
    timing["next_line"]["digest"].update(lines=1,glyphs=5)
    return dict(schema=1, metadata=dict(revision="one",conditions=dict(rustc="rust",cargo="cargo",cpu="cpu",os="os",features=["complex-scripts"],profile="release",flags=[],font_hashes=["b"*64],input_hash="c"*64,harness_hash="d"*64,lock_hash="e"*64,measurement_config=dict(quick=True,cold_samples=2))), selected=[dict(key=key,settings=settings)], rows=[dict(key=key,settings=settings,timing=timing,cold=[cold,cold],process_wall_ns=[20,20],memory=memory)])

class ReportTests(unittest.TestCase):
    def test_fresh_checkout_resolves_lock_and_existing_lock_is_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/"src").mkdir();(root/"src/lib.rs").write_text("")
            (root/"Cargo.toml").write_text('[package]\nname="fresh-benchmark-lock"\nversion="0.0.0"\nedition="2024"\n[workspace]\n')
            runner.ensure_lock(root,root/"resolve.log")
            lock=(root/"Cargo.lock").read_bytes()
            self.assertIn(b'fresh-benchmark-lock',lock)
            runner.ensure_lock(root,root/"again.log")
            self.assertEqual((root/"Cargo.lock").read_bytes(),lock)
    def test_cli_invalid_configuration_fails_without_publishing(self):
        with tempfile.TemporaryDirectory() as tmp:
            target=Path(tmp)/"new"
            for extra in [("--case","unknown"),("--cold-samples","1")]:
                result=subprocess.run([sys.executable,str(Path(__file__).with_name("run.py")),"--output",str(target),*extra],capture_output=True,text=True)
                self.assertNotEqual(result.returncode,0)
                self.assertFalse(target.exists())
    def test_cargo_artifact_selection_rejects_missing_or_ambiguous_binary(self):
        artifact=dict(reason="compiler-artifact",target=dict(name="shodo-probe",kind=["bin"]),executable="/actual/probe")
        irrelevant=dict(reason="compiler-artifact",target=dict(name="shodo-probe",kind=["lib"]),executable="/wrong/lib")
        self.assertEqual(runner.executable_from_cargo("\n".join(json.dumps(x) for x in [irrelevant,artifact]),"shodo-probe","bin"),Path("/actual/probe"))
        for rows in [[irrelevant],[artifact,artifact]]:
            with self.assertRaises(ValueError):runner.executable_from_cargo("\n".join(json.dumps(x) for x in rows),"shodo-probe","bin")
    def test_failed_real_child_retains_log_and_prevents_publish(self):
        with tempfile.TemporaryDirectory() as tmp:
            target=Path(tmp)/"new";log=Path(tmp)/"failure.log"
            def collect(stage):
                runner.execute([sys.executable,"-c","print('partial'); raise SystemExit(7)"],log,cwd=stage)
                return valid_report()
            with self.assertRaises(subprocess.CalledProcessError):runner.publish(target,collect)
            self.assertFalse(target.exists())
            self.assertIn("partial",log.read_text())
    def test_invalid_selection_rejected_before_output_or_commands(self):
        with tempfile.TemporaryDirectory() as tmp:
            target=Path(tmp)/"new"
            for case,samples in [("unknown",2),("",2),(None,0),(None,1)]:
                with self.subTest(case=case,samples=samples),self.assertRaises(ValueError):
                    runner.validate_config(target,case,samples)
            self.assertFalse(target.exists())
    def test_raw_criterion_samples_have_literal_per_iteration_median(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);settings=valid_report()["selected"][0]["settings"]
            operations={op:valid_report()["rows"][0]["timing"][op]["digest"] for op in OPS}
            (root/"digests.json").write_text(json.dumps([dict(settings=settings,operations=operations)]))
            for op in OPS:
                p=root/"latin-short"/op/"1"/"new";p.mkdir(parents=True)
                (p/"sample.json").write_text(json.dumps(dict(iters=[2]*10,times=[200]*10)))
            timings=runner.load_timings(root,[dict(key="latin-short/1",settings=settings)])
            self.assertEqual(timings["latin-short/1"]["build"]["median_ns"],100)
            (root/"latin-short"/"all_lines"/"1"/"new"/"sample.json").unlink()
            with self.assertRaises(FileNotFoundError):runner.load_timings(root,[dict(key="latin-short/1",settings=settings)])
    def test_compatible_revision_change_has_literal_ratios_and_memory_deltas(self):
        before=valid_report();after=copy.deepcopy(before);after["metadata"]["revision"]="two"
        for op in OPS:
            after["rows"][0]["timing"][op]["median_ns"]=200
            after["rows"][0]["timing"][op]["sample"]["times"]=[200]*10
        after["rows"][0]["memory"]["scopes"][0]["counts"].update(allocated_bytes=32,live_bytes=64,net_bytes=32,peak_extra_bytes=32)
        result=runner.compare(after,before)
        self.assertEqual(result["rows"][0]["timing_ratios"]["build"],2)
        self.assertEqual(result["rows"][0]["memory_net_deltas"]["font_context_init"],16)
        self.assertEqual(result["rows"][0]["cold_median_ratios"]["build"],1)
        self.assertEqual(result["rows"][0]["process_wall_median_ratio"],1)
    def test_changed_conditions_are_rejected(self):
        for field in ["rustc","cargo","cpu","os","features","profile","flags","font_hashes","input_hash","harness_hash","lock_hash","measurement_config"]:
            after=valid_report();after["metadata"]["conditions"][field]="different"
            with self.subTest(field=field),self.assertRaises(ValueError):runner.compare(after,valid_report())
    def test_missing_duplicate_or_empty_measurements_are_rejected(self):
        for mutate in [lambda x:x["rows"].clear(),lambda x:x["rows"].append(copy.deepcopy(x["rows"][0])),lambda x:x["selected"].clear(),lambda x:x["rows"][0]["timing"].pop("all_lines"),lambda x:x["rows"][0]["cold"].clear(),lambda x:x["rows"][0]["memory"]["scopes"].pop()]:
            v=valid_report();mutate(v)
            with self.assertRaises(ValueError):runner.validate_report(v)
    def test_nonfinite_negative_wrong_samples_and_inconsistent_counts_fail(self):
        for mutate in [lambda x:x["rows"][0]["timing"]["build"].update(median_ns=float("nan")),lambda x:x["rows"][0]["timing"]["build"]["sample"]["times"].__setitem__(0,-1),lambda x:x["rows"][0]["timing"]["build"]["sample"]["iters"].__setitem__(0,0),lambda x:x["rows"][0]["memory"]["scopes"][0]["counts"].update(net_bytes=0),lambda x:x["rows"][0]["memory"]["scopes"][0]["counts"].update(peak_extra_bytes=0),lambda x:x["rows"][0]["cold"][0]["durations_ns"].update(build=-1),lambda x:x["rows"][0]["memory"].update(instrumented=False)]:
            v=valid_report();mutate(v)
            with self.assertRaises(ValueError):runner.validate_report(v)
    def test_stale_summary_and_empty_output_are_rejected(self):
        after=valid_report();after["rows"][0]["timing"]["build"]["sample"]["times"]=[200]*10
        with self.assertRaises(ValueError):runner.validate_report(after)
        after=valid_report()
        for t in after["rows"][0]["timing"].values():t["digest"]["lines"]=0;t["digest"]["glyphs"]=0;t["digest"]["runs"]=0
        with self.assertRaises(ValueError):runner.validate_report(after)
    def test_changed_settings_or_output_digest_fail_before_comparison(self):
        after=valid_report();after["rows"][0]["settings"]["width"]=160
        with self.assertRaises(ValueError):runner.compare(after,valid_report())
        after=valid_report();after["rows"][0]["timing"]["next_line"]["digest"]["sha256"]="f"*64
        with self.assertRaises(ValueError):runner.compare(after,valid_report())
    def test_failed_run_preserves_existing_results_and_never_publishes_partial_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            old=Path(tmp)/"old";old.mkdir();(old/"results.json").write_text("existing")
            with self.assertRaises(FileExistsError):runner.publish(old,lambda stage:valid_report())
            self.assertEqual((old/"results.json").read_text(),"existing")
            new=Path(tmp)/"new"
            def fail(stage):
                (stage/"raw.json").write_text("partial")
                raise RuntimeError("last child failed")
            with self.assertRaises(RuntimeError):runner.publish(new,fail)
            self.assertFalse(new.exists())
    def test_validation_failure_never_publishes_partial_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            target=Path(tmp)/"results"
            with self.assertRaises(ValueError):runner.publish(target,lambda stage:{})
            self.assertFalse(target.exists())
    def test_valid_report_publishes_a_reviewable_json_artifact(self):
        with tempfile.TemporaryDirectory() as tmp:
            target=Path(tmp)/"results";runner.publish(target,lambda stage:valid_report());self.assertTrue((target/"results.json").is_file())

if __name__=="__main__":unittest.main()

"""Reports must be shareable without recording machine-local home paths."""
import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


HERE = Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ReportPathTests(unittest.TestCase):
    def test_saved_nested_paths_are_portable_without_changing_live_inputs(self):
        runner = load("raikiri_measure")
        home = Path.home()
        value = {
            "argv": ["cargo", "--manifest-path", str(runner.ROOT / "target/probe/Cargo.toml")],
            "artifact": {"executable": str(home / "tmp/probe"),
                         "profile": {"opt_level": "3", "test": False}},
            "diagnostic": f"cannot open {home}/tmp/日本語.json",
            "source_sha256": {str(runner.ROOT / "dev/source.rs"): "a" * 64},
            "rows": [{"duration_ns": 123, "ratio": None}],
            "unrelated": ["dev/source.rs", "https://example.invalid/source", "/usr/bin/perf"],
        }
        original = copy.deepcopy(value)
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report.json"
            runner.write_json(output, value)
            saved = json.loads(output.read_text())
        self.assertEqual(saved, {
            "argv": ["cargo", "--manifest-path", "./target/probe/Cargo.toml"],
            "artifact": {"executable": "~/tmp/probe", "profile": {"opt_level": "3", "test": False}},
            "diagnostic": "cannot open ~/tmp/日本語.json",
            "source_sha256": {"./dev/source.rs": "a" * 64},
            "rows": [{"duration_ns": 123, "ratio": None}],
            "unrelated": ["dev/source.rs", "https://example.invalid/source", "/usr/bin/perf"],
        })
        self.assertEqual(value, original)

    def test_jt1_failure_keeps_execution_paths_but_reports_portable_argv(self):
        runner = load("jt1_measure")
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "probe"
            binary.write_text("#!/bin/sh\nprintf 'failed: %s\\n' \"$3\" >&2\nexit 7\n")
            binary.chmod(0o755)
            output = Path(temporary) / "output.json"
            # Real process execution needs an absolute path, even though the
            # returned evidence must use portable names.
            with patch.object(runner, "ROOT", Path(temporary)):
                row = runner.run_probe(binary, "time", "native", "case.html", output, "pipeline")
        self.assertEqual(row["returncode"], 7)
        self.assertFalse(row["ok"])
        self.assertNotIn(str(Path.home()) + "/", json.dumps(row))
        self.assertEqual(row["argv"][0], "./probe")
        self.assertIn("case.html", row["argv"])

    def test_complete_directory_prefixes_and_bare_roots_are_normalized(self):
        paths = load("report_paths")
        home = Path("/") / "Users" / "fixture"
        root = home / "shodo"
        with patch.object(paths.Path, "home", return_value=home):
            self.assertEqual(paths.portable([
                str(root), str(home), f"error: '{root}'", str(root / "src/main.rs"),
                str(home / "shodo-other/main.rs"), str(home) + "-other/file",
                f"path+file://{root}/Cargo.toml",
            ], root), [
                ".", "~", "error: '.'", "./src/main.rs", "~/shodo-other/main.rs",
                str(home) + "-other/file", "path+file://./Cargo.toml",
            ])

    def test_nonfinite_measurements_are_still_rejected(self):
        runner = load("raikiri_measure")
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                runner.write_json(Path(temporary) / "report.json", {"duration_ns": float("nan")})

    def test_paths_with_spaces_and_embedded_home_names_keep_their_identity(self):
        paths = load("report_paths")
        home = Path("/") / "Users" / "fixture"
        root = home / "shodo"
        with patch.object(paths.Path, "home", return_value=home):
            self.assertEqual(paths.portable([
                str(Path("/") / "mnt" / "Users" / "fixture" / "other"),
                str(home / "shodo backup/src/main.rs"),
                str(home / "shodo.backup/src/main.rs"),
                str(home / "shodo:backup/src/main.rs"),
                str(home / "shodo" / "a b.rs"),
                f"failed to open '{home}/shodo backup/src/main.rs'",
            ], root), [
                str(Path("/") / "mnt" / "Users" / "fixture" / "other"),
                "~/shodo backup/src/main.rs", "~/shodo.backup/src/main.rs",
                "~/shodo:backup/src/main.rs", "./a b.rs",
                "failed to open '~/shodo backup/src/main.rs'",
            ])

    def test_diagnostic_punctuation_and_cargo_file_uri_are_portable(self):
        paths = load("report_paths")
        root = Path("/") / "work" / "shodo"
        self.assertEqual(paths.portable([
            f"checkout: {root}.", f"file://{root}#probe@0.1",
            f"path+file://{root}/probe#0.1",
        ], root), ["checkout: ..", "file://.#probe@0.1", "path+file://./probe#0.1"])

    def test_absolute_first_diagnostic_keeps_normalizing_later_paths(self):
        paths = load("report_paths")
        home = Path("/") / "Users" / "fixture"
        root = home / "shodo"
        with patch.object(paths.Path, "home", return_value=home):
            self.assertEqual(paths.portable({
                "diagnostic": f"{root}/src/main.rs: error\ncaused by: {home}/tmp/font.ttf",
                "stderr_tail": f"/opt/probe: cannot open {home}/tmp/font.ttf",
                "brackets": f"cwd=[{home}], checkout=({root})",
                "sibling": str(root) + ".",
            }, root), {
                "diagnostic": "./src/main.rs: error\ncaused by: ~/tmp/font.ttf",
                "stderr_tail": "/opt/probe: cannot open ~/tmp/font.ttf",
                "brackets": "cwd=[~], checkout=(.)",
                "sibling": "~/shodo.",
            })

    def test_portable_path_recipe_is_included_in_saved_source_fingerprint(self):
        runner = load("raikiri_measure")
        self.assertIn("tools/raikiri/report_paths.py", runner.source_hashes())

    def test_archive_manifest_paths_resolve_and_provenance_has_no_home_paths(self):
        runner = load("raikiri_overlay")
        import hashlib
        import subprocess
        import tomllib
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary) / "source"
            page = repo / "dev/raikiri/src/candidate_page.rs"
            page.parent.mkdir(parents=True)
            page.write_text("// immutable fixture\n")
            def git(*argv):
                return subprocess.check_output(["git", *argv], cwd=repo, stderr=subprocess.DEVNULL)
            git("init")
            git("add", ".")
            git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "fixture")
            revision = git("rev-parse", "HEAD").decode().strip()
            lock = repo / "Cargo.lock"
            lock.write_text("version = 4\n")
            destination = Path(temporary) / "archive"
            with patch.object(runner, "S4_REVISION", revision), \
                    patch.object(runner, "PAGE_SOURCE_SHA256", hashlib.sha256(page.read_bytes()).hexdigest()), \
                    patch.object(runner, "LOCK_SOURCE_SHA256", hashlib.sha256(lock.read_bytes()).hexdigest()):
                provenance = runner.prepare(repo, destination, expose_layout_boundary=False)
            manifest = tomllib.loads((destination / "Cargo.toml").read_text())
            for target in manifest["bin"]:
                with self.subTest(target=target["name"]):
                    path = Path(target["path"])
                    self.assertFalse(path.is_absolute())
                    self.assertTrue((destination / path).is_file())
            saved = (destination / "archive-provenance.json").read_text()
            self.assertNotIn(str(Path.home()) + "/", saved)
            self.assertEqual(json.loads(saved), provenance)


if __name__ == "__main__":
    unittest.main()

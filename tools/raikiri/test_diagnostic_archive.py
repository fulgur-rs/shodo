"""Verify collection provenance with real files, without building probes."""
import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

HERE = Path(__file__).resolve().parent


class StopCollection(RuntimeError):
    pass


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DiagnosticArchiveTests(unittest.TestCase):
    def setUp(self):
        self.measurement = load("raikiri_measure")
        self.library = load("raikiri_library_measure")
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "original"
        self.source.mkdir()
        self.raw = "native-initial model diagnostic\n日本語\n".encode("utf-8")
        self.log = self.source / "native.log"
        self.log.write_bytes(self.raw)
        self.selection = HERE / "fixtures/raikiri/selection.json"
        self.case = json.loads(self.selection.read_text())["cases"][0]["id"]
        self.inventory = {
            "source": str(self.log),
            "source_sha256": hashlib.sha256(self.raw).hexdigest(),
            "messages": [{"id": self.case, "phase": "native-initial"}],
            "diagnostic_count": 1,
            "documents": [self.case],
            "document_count": 1,
        }
        self.input = self.source / "inventory.json"
        self.input.write_text(json.dumps(self.inventory))
        self.input_bytes = self.input.read_bytes()

    def args(self, output, diagnostics=None):
        return SimpleNamespace(
            selection=self.selection, diagnostics=diagnostics or self.input,
            spike=self.source, raikiri=self.source, wpt=self.source,
            references=self.source, output=output, target_dir=self.root / "target",
            repetitions=1, operations=["layout"],
        )

    def whole_archive(self, output):
        # Only the expensive probe-preparation boundary is replaced. The real
        # collector validates inputs and writes its archive and source snapshots.
        with patch.object(self.measurement.overlay, "prepare", side_effect=StopCollection):
            with self.assertRaises(StopCollection):
                self.measurement.collect(self.args(output))
        return output / "native-diagnostic-inventory.json"

    def assert_archive(self, inventory_path):
        value = json.loads(inventory_path.read_text())
        source = Path(value["source"])
        self.assertFalse(source.is_absolute())
        saved = inventory_path.parent / source
        self.assertTrue(saved.resolve().is_relative_to(inventory_path.parent.resolve()))
        self.assertEqual(saved.read_bytes(), self.raw)
        self.assertEqual(value["original_source"], str(self.log))
        self.assertEqual(value["source_sha256"], hashlib.sha256(self.raw).hexdigest())
        for key in ["messages", "diagnostic_count", "documents", "document_count"]:
            self.assertEqual(value[key], self.inventory[key])
        self.assertEqual(self.measurement.diagnostic_inventory(inventory_path), {self.case})
        return saved

    def move_without_original(self, output):
        self.log.unlink()
        self.input.unlink()
        moved = self.root / (output.name + "-moved")
        output.rename(moved)
        return moved / "native-diagnostic-inventory.json"

    def test_whole_archive_survives_original_removal_and_collection_move(self):
        output = self.root / "whole"
        inventory_path = self.whole_archive(output)
        self.assertEqual(self.input.read_bytes(), self.input_bytes)
        self.assert_archive(inventory_path)
        saved = self.assert_archive(self.move_without_original(output))
        saved.write_bytes(b"changed diagnostic log")
        with self.assertRaisesRegex(ValueError, "native diagnostic source log changed"):
            self.measurement.diagnostic_inventory(saved.parent / "native-diagnostic-inventory.json")

    def test_metadata_distinguishes_input_and_archived_inventory_hashes(self):
        output = self.root / "metadata"
        # Exercise actual metadata emission, stopping at the first measurement.
        # Probe build/toolchain discovery are unrelated to this file-copy contract.
        with patch.object(self.measurement.overlay, "prepare", return_value={}), \
                patch.object(self.measurement.foundation, "build_configuration", return_value={}), \
                patch.object(self.measurement, "build", return_value=({"time": Path("unused")}, [])), \
                patch.object(self.measurement.foundation, "execute", return_value="test-version"), \
                patch.object(self.measurement, "invoke", side_effect=StopCollection):
            with self.assertRaises(StopCollection):
                self.measurement.collect(self.args(output))
        self.assert_archive(output / "native-diagnostic-inventory.json")
        metadata = json.loads((output / "metadata.json").read_text())
        original_hash = hashlib.sha256(self.input_bytes).hexdigest()
        archived_hash = hashlib.sha256((output / "native-diagnostic-inventory.json").read_bytes()).hexdigest()
        self.assertNotEqual(original_hash, archived_hash)
        self.assertEqual(metadata["native_diagnostic_inventory_sha256"], original_hash)
        self.assertEqual(metadata["archived_native_diagnostic_inventory_sha256"], archived_hash)
        self.assertEqual(metadata["native_segmentation_exclusions"], [self.case])

    def test_library_can_rearchive_a_moved_whole_inventory(self):
        whole = self.root / "whole"
        self.whole_archive(whole)
        input_path = self.move_without_original(whole)
        output = self.root / "library"
        with patch.object(self.library.recipe, "prepare_library", side_effect=StopCollection):
            with self.assertRaises(StopCollection):
                self.library.collect(self.args(output, input_path))
        self.assert_archive(output / "native-diagnostic-inventory.json")
        moved = self.root / "library-moved"
        output.rename(moved)
        self.assert_archive(moved / "native-diagnostic-inventory.json")


if __name__ == "__main__":
    unittest.main()

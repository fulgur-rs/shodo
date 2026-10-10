import importlib.util
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from . import run as runner

SPEC = importlib.util.spec_from_file_location("core_comparison_runner", Path(__file__).with_name("compare_core.py"))
comparison = importlib.util.module_from_spec(SPEC)
with patch.dict(sys.modules, {"run": runner}):
    SPEC.loader.exec_module(comparison)


class SystemMetadataTests(unittest.TestCase):
    def test_metadata_without_linux_interfaces(self):
        cpuinfo = Mock()
        cpuinfo.is_file.return_value = False
        with patch.object(comparison, "os", SimpleNamespace()), patch.object(comparison, "Path", return_value=cpuinfo):
            self.assertEqual(comparison.system_metadata(), {"affinity": None, "cpu_models": []})
        cpuinfo.read_text.assert_not_called()

    def test_linux_metadata_preserves_affinity_and_cpu_models(self):
        cpuinfo = Mock()
        cpuinfo.is_file.return_value = True
        cpuinfo.read_text.return_value = "model name : CPU B\nmodel name : CPU A\nmodel name : CPU B\nprocessor : 0\n"
        with patch.object(comparison.os, "sched_getaffinity", return_value={3, 1}, create=True), patch.object(comparison, "Path", return_value=cpuinfo):
            self.assertEqual(comparison.system_metadata(), {"affinity": [1, 3], "cpu_models": ["CPU A", "CPU B"]})

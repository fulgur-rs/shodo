"""Run the actual snapshots CLI; no browser or image-library dependency."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parents[3]


def files(directory):
    return {p.name: p.read_bytes() for p in directory.iterdir()}


def change_first_pixel(path):
    # Tiny-Skia's encoder writes RGBA8. Decode PNG filters independently so the
    # test changes exactly one pixel, not compression or metadata alone.
    data = path.read_bytes()
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    offset = 8
    compressed = b''
    while offset < len(data):
        size = struct.unpack('>I', data[offset:offset + 4])[0]
        kind = data[offset + 4:offset + 8]
        body = data[offset + 8:offset + 8 + size]
        if kind == b'IHDR':
            width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', body)
            assert (depth, color, interlace) == (8, 6, 0)
        if kind == b'IDAT':
            compressed += body
        offset += size + 12
    raw = zlib.decompress(compressed)
    stride = width * 4
    previous = bytearray(stride)
    rows = []
    for y in range(height):
        start = y * (stride + 1)
        method = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            left = row[i - 4] if i >= 4 else 0
            up = previous[i]
            corner = previous[i - 4] if i >= 4 else 0
            if method == 0:
                predictor = 0
            elif method == 1:
                predictor = left
            elif method == 2:
                predictor = up
            elif method == 3:
                predictor = (left + up) // 2
            elif method == 4:
                estimate = left + up - corner
                distances = [abs(estimate - v) for v in (left, up, corner)]
                predictor = (left, up, corner)[distances.index(min(distances))]
            else:
                raise AssertionError(f'unknown PNG filter {method}')
            row[i] = (row[i] + predictor) & 255
        rows.append(row)
        previous = row
    assert rows[0][:4] == b'\xff\xff\xff\xff'
    rows[0][0] = 0

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(b''.join(b'\0' + row for row in rows))) + chunk(b'IEND', b''))


class SnapshotCliTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(['cargo', '+stable', 'build', '--offline', '-p', 'shodo-fixtures', '--example', 'snapshots', '--message-format=json'], cwd=ROOT, text=True, capture_output=True)
        if result.returncode:
            raise AssertionError(f'snapshot CLI build failed:\n{result.stderr}')
        artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        binaries = [a['executable'] for a in artifacts if a.get('reason') == 'compiler-artifact' and a['target']['name'] == 'snapshots' and a.get('executable')]
        if len(binaries) != 1:
            raise AssertionError(f'expected exactly one snapshots compiler artifact, got {binaries}')
        cls.binary = binaries[0]
        cls.shared = tempfile.TemporaryDirectory(prefix='shodo-snapshot-cli-')
        cls.base = Path(cls.shared.name) / 'expected'
        update = subprocess.run([cls.binary, '--expected', str(cls.base), '--output', str(Path(cls.shared.name) / 'update'), '--update'], capture_output=True, text=True)
        if update.returncode:
            cls.shared.cleanup()
            raise AssertionError(update.stderr)
        if len(files(cls.base)) != 93:
            cls.shared.cleanup()
            raise AssertionError('full update did not produce 46 PNG/geometry pairs and manifest')

    @classmethod
    def tearDownClass(cls):
        cls.shared.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='shodo-snapshot-command-')
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.expected = self.directory / 'expected'
        shutil.copytree(self.base, self.expected)

    def command(self, output, *args, expected=None, cwd=None):
        return subprocess.run([self.binary, '--expected', str(expected or self.expected), '--output', str(output), *args], cwd=cwd or self.directory, capture_output=True, text=True)

    def test_full_check_passes_without_rewriting_any_expected_bytes(self):
        before = files(self.expected)
        output = self.directory / 'check'
        result = self.command(output)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads((output / 'report.json').read_text())
        self.assertTrue(report['passed'])
        self.assertFalse(report['partial'])
        self.assertEqual(len(report['cases']), 46)
        self.assertEqual(files(self.expected), before)

    def test_one_changed_pixel_fails_with_triples_and_preserves_expectations(self):
        change_first_pixel(self.expected / 'latin-short.png')
        before = files(self.expected)
        output = self.directory / 'changed'
        result = self.command(output, '--case', 'latin-short')
        self.assertNotEqual(result.returncode, 0)
        report = json.loads((output / 'report.json').read_text())
        self.assertFalse(report['passed'])
        self.assertTrue(report['partial'])
        self.assertEqual(report['cases'][0]['changed_pixels'], 1)
        self.assertTrue((output / 'index.html').is_file())
        for asset in ('expected.png', 'actual.png', 'diff.png'):
            self.assertTrue((output / 'latin-short' / asset).is_file())
        self.assertEqual(files(self.expected), before)

    def test_deleted_image_fails_and_remaining_bytes_are_immutable(self):
        (self.expected / 'latin-short.png').unlink()
        before = files(self.expected)
        output = self.directory / 'deleted'
        result = self.command(output, '--case', 'latin-short')
        self.assertNotEqual(result.returncode, 0)
        for asset in ('actual.png', 'diff.png'):
            self.assertTrue((output / 'latin-short' / asset).is_file())
        self.assertTrue((output / 'index.html').is_file())
        self.assertEqual(files(self.expected), before)

    def test_missing_baseline_is_never_created_by_normal_check(self):
        missing = self.directory / 'missing'
        output = self.directory / 'missing-report'
        result = self.command(output, '--case', 'latin-short', expected=missing)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(missing.exists())
        self.assertTrue((output / 'latin-short/actual.png').is_file())

    def test_invalid_flags_selection_and_partial_update_fail_before_writes(self):
        before = files(self.expected)
        for i, args in enumerate([('--case', 'unknown'), ('--unknown',), ('--update', '--case', 'latin-short')]):
            with self.subTest(args=args):
                output = self.directory / f'invalid-{i}'
                self.assertNotEqual(self.command(output, *args).returncode, 0)
                self.assertFalse(output.exists())
        self.assertEqual(files(self.expected), before)

    def test_existing_output_and_nested_or_symlink_alias_are_preserved(self):
        before = files(self.expected)
        output = self.directory / 'existing'
        output.mkdir()
        (output / 'keep').write_bytes(b'caller-owned')
        self.assertNotEqual(self.command(output).returncode, 0)
        self.assertEqual((output / 'keep').read_bytes(), b'caller-owned')
        nested = self.expected / 'report'
        self.assertNotEqual(self.command(nested, '--update').returncode, 0)
        self.assertFalse(nested.exists())
        if hasattr(os, 'symlink'):
            alias = self.directory / 'alias'
            alias.symlink_to(self.expected, target_is_directory=True)
            self.assertNotEqual(self.command(alias / 'report').returncode, 0)
            self.assertFalse((alias / 'report').exists())
        self.assertEqual(files(self.expected), before)

    def test_default_baseline_is_independent_of_current_directory(self):
        output = self.directory / 'default'
        result = subprocess.run([self.binary, '--output', str(output), '--case', 'latin-short'], cwd=self.directory, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(json.loads((output / 'report.json').read_text())['passed'])

    def test_identical_pixels_with_changed_case_settings_fail(self):
        manifest = self.expected / 'manifest.json'
        data = json.loads(manifest.read_text())
        data['cases']['latin-short']['width'] = 999
        manifest.write_text(json.dumps(data))
        before = files(self.expected)
        output = self.directory / 'settings'
        result = self.command(output, '--case', 'latin-short')
        self.assertNotEqual(result.returncode, 0)
        row = json.loads((output / 'report.json').read_text())['cases'][0]
        self.assertEqual(row['changed_pixels'], 0)
        self.assertIn('settings', row['reason'])
        self.assertEqual(files(self.expected), before)

    def test_unknown_expected_files_survive_refused_update(self):
        (self.expected / 'caller-note.txt').write_bytes(b'keep')
        before = files(self.expected)
        output = self.directory / 'refused-update'
        self.assertNotEqual(self.command(output, '--update').returncode, 0)
        self.assertFalse(output.exists())
        self.assertEqual(files(self.expected), before)


if __name__ == '__main__':
    unittest.main()

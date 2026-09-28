import importlib.util
import hashlib
import json
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest

import regenerate


class RegenerationTests(unittest.TestCase):
    def test_checked_in_artifacts_are_complete_and_bounded(self):
        root = Path(__file__).resolve().parents[1]
        regenerate.check_assets(root)
        manifest = json.loads((root / 'assets/manifest.json').read_text(encoding="utf-8"))
        self.assertEqual([font['id'] for font in manifest['fonts']], ['latin', 'cjk', 'arabic', 'emoji-color', 'emoji-mono'])
        self.assertLessEqual(sum(font['size'] for font in manifest['fonts']), 768 * 1024)
        for font in manifest['fonts']:
            self.assertEqual(font['face_index'], 0)
            self.assertEqual(len(font['source_sha256']), 64)
            self.assertTrue(font['source_url'].startswith('https://raw.githubusercontent.com/'))
            self.assertIn('OFL', (root / font['license']).read_text(encoding="utf-8"))

    @unittest.skipUnless(importlib.util.find_spec('fontTools'), 'FontTools required for font-name metadata check')
    def test_cff_internal_name_matches_renamed_family(self):
        from fontTools.ttLib import TTFont
        root = Path(__file__).resolve().parents[1]
        manifest = json.loads((root / 'assets/manifest.json').read_text(encoding="utf-8"))
        for entry in manifest['fonts']:
            font = TTFont(root / entry['path'])
            self.assertEqual(font['name'].getDebugName(1), entry['family'])
            if 'CFF ' in font:
                self.assertEqual(font['CFF '].cff.fontNames, [entry['family'].replace(' ', '') + '-Regular'])

    def test_non_utf8_locale_reads_corpus_before_verifying_sources(self):
        root = Path(__file__).resolve().parents[1]
        manifest = json.loads((root / 'assets/manifest.json').read_text(encoding='utf-8'))
        with tempfile.TemporaryDirectory() as directory:
            sources = Path(directory)
            (sources / manifest['fonts'][0]['source_file']).write_bytes(b'replaced source')
            env = os.environ.copy()
            env.update(PYTHONUTF8='0', PYTHONCOERCECLOCALE='0', LC_ALL='C', PYTHONDONTWRITEBYTECODE='1')
            result = subprocess.run([
                sys.executable, str(root / 'tools/regenerate.py'), '--rebuild',
                '--sources-dir', str(sources),
            ], env=env, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'source checksum mismatch', result.stderr)
            self.assertNotIn(b'UnicodeDecodeError', result.stderr)

    @unittest.skipUnless(importlib.util.find_spec('fontTools'), 'FontTools required')
    def test_rebuild_reads_each_fonts_own_corpus(self):
        # A wrong shared corpus would retain A and lose B in this actual font.
        from fontTools.ttLib import TTFont
        base = Path(__file__).resolve().parents[1]
        data = (base / 'assets/fonts/latin.ttf').read_bytes()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'assets').mkdir()
            (root / 'sources').mkdir()
            (root / 'sources/latin.ttf').write_bytes(data)
            entry = {
                'id': 'probe', 'path': 'assets/output.ttf', 'family': 'Corpus Probe',
                'source_file': 'latin.ttf', 'source_sha256': regenerate.digest(data),
                'extra_unicode_ranges': [], 'corpus': 'assets/emoji-cases.json',
            }
            (root / 'assets/cases.json').write_text(json.dumps([
                {'font_ids': ['probe'], 'text': 'A'}]), encoding='utf-8')
            (root / 'assets/emoji-cases.json').write_text(json.dumps([
                {'font_ids': ['probe'], 'text': 'B'}]), encoding='utf-8')
            (root / 'assets/manifest.json').write_text(json.dumps({
                'format_version': 1, 'fonts': [entry]}), encoding='utf-8')
            regenerate.rebuild(root, root / 'sources', update=True)
            font = TTFont(root / 'assets/output.ttf')
            self.assertIn(ord('B'), font.getBestCmap())
            self.assertNotIn(ord('A'), font.getBestCmap())

    def test_offline_check_rejects_total_budget_without_mutation(self):
        # Each real font is valid and small; only their sum exceeds the budget.
        base = Path(__file__).resolve().parents[1]
        data = (base / 'assets/fonts/latin.ttf').read_bytes()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'assets').mkdir()
            (root / 'assets/font.ttf').write_bytes(data)
            (root / 'assets/OFL.txt').write_text('OFL', encoding='utf-8')
            fonts = [dict(id=str(i), path='assets/font.ttf', sha256=regenerate.digest(data),
                          size=len(data), face_index=0, license='assets/OFL.txt') for i in range(9)]
            (root / 'assets/manifest.json').write_text(json.dumps({
                'format_version': 1, 'fonts': fonts}), encoding='utf-8')
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            with self.assertRaisesRegex(ValueError, 'font budget'):
                regenerate.check_assets(root)
            self.assertEqual(before, {p: p.read_bytes() for p in root.rglob('*') if p.is_file()})

    @unittest.skipUnless(importlib.util.find_spec('fontTools'), 'FontTools required')
    def test_over_budget_update_leaves_all_assets_untouched(self):
        base = Path(__file__).resolve().parents[1]
        data = (base / 'assets/fonts/latin.ttf').read_bytes()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'assets').mkdir()
            (root / 'sources').mkdir()
            (root / 'sources/latin.ttf').write_bytes(data)
            fonts = []
            for i in range(9):
                path = f'assets/font{i}.ttf'
                (root / path).write_bytes(b'unchanged asset')
                fonts.append(dict(id=str(i), path=path, family='Budget Probe',
                                  source_file='latin.ttf', source_sha256=regenerate.digest(data),
                                  extra_unicode_ranges=[[32,126], [160,383], [768,879]]))
            (root / 'assets/cases.json').write_text('[]', encoding='utf-8')
            (root / 'assets/manifest.json').write_text(json.dumps({
                'format_version': 1, 'fonts': fonts}), encoding='utf-8')
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            with self.assertRaisesRegex(ValueError, 'font budget'):
                regenerate.rebuild(root, root / 'sources', update=True)
            self.assertEqual(before, {p: p.read_bytes() for p in root.rglob('*') if p.is_file()})

    def test_replaced_source_is_rejected(self):
        original = b'original font'
        digest = hashlib.sha256(original).hexdigest()
        regenerate.validate_source(original, digest)
        with self.assertRaisesRegex(ValueError, 'source checksum'):
            regenerate.validate_source(b'replaced font', digest)

    def test_check_mode_leaves_mismatched_files_untouched(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'assets').mkdir()
            (root / 'assets/font.ttf').write_bytes(b'font')
            manifest = {'format_version': 1, 'fonts': [{
                'id': 'test', 'path': 'assets/font.ttf', 'sha256': '0' * 64,
                'size': 4, 'face_index': 0, 'license': 'assets/OFL.txt',
            }]}
            (root / 'assets/manifest.json').write_text(json.dumps(manifest), encoding="utf-8")
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            with self.assertRaisesRegex(ValueError, 'checksum'):
                regenerate.check_assets(root)
            self.assertEqual(before, {p: p.read_bytes() for p in root.rglob('*') if p.is_file()})


if __name__ == '__main__':
    unittest.main()

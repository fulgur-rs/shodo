import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import regenerate


class RegenerationTests(unittest.TestCase):
    def test_checked_in_artifacts_are_complete_and_bounded(self):
        root = Path(__file__).resolve().parents[1]
        regenerate.check_assets(root)
        manifest = json.loads((root / 'assets/manifest.json').read_text())
        self.assertEqual([font['id'] for font in manifest['fonts']], ['latin', 'cjk', 'arabic'])
        self.assertLessEqual(sum(font['size'] for font in manifest['fonts']), 512 * 1024)
        for font in manifest['fonts']:
            self.assertEqual(font['face_index'], 0)
            self.assertEqual(len(font['source_sha256']), 64)
            self.assertIn('raw.githubusercontent.com/notofonts/', font['source_url'])
            self.assertIn('OFL', (root / font['license']).read_text())

    @unittest.skipUnless(importlib.util.find_spec('fontTools'), 'FontTools required for font-name metadata check')
    def test_cff_internal_name_matches_renamed_family(self):
        from fontTools.ttLib import TTFont
        root = Path(__file__).resolve().parents[1]
        manifest = json.loads((root / 'assets/manifest.json').read_text())
        for entry in manifest['fonts']:
            font = TTFont(root / entry['path'])
            self.assertEqual(font['name'].getDebugName(1), entry['family'])
            if 'CFF ' in font:
                self.assertEqual(font['CFF '].cff.fontNames, [entry['family'].replace(' ', '') + '-Regular'])

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
            (root / 'assets/manifest.json').write_text(json.dumps(manifest))
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            with self.assertRaisesRegex(ValueError, 'checksum'):
                regenerate.check_assets(root)
            self.assertEqual(before, {p: p.read_bytes() for p in root.rglob('*') if p.is_file()})


if __name__ == '__main__':
    unittest.main()

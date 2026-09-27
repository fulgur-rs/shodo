import json
import tempfile
import unittest
from pathlib import Path
import collect_browser as collector

class CollectorValidationTests(unittest.TestCase):
    def test_unicode_endpoint_validation_rejects_surrogate_middle(self):
        for units, byte in [(0, 0), (1, 1), (3, 5), (4, 7)]:
            self.assertEqual(collector.utf16_to_utf8('A𠮷é', units), byte)
        with self.assertRaises(ValueError):
            collector.utf16_to_utf8('A𠮷é', 2)
        with self.assertRaises(ValueError):
            collector.utf16_to_utf8('A𠮷é', 5)

    def test_failed_or_incomplete_capture_preserves_previous_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'capture.json'
            path.write_text('previous', encoding='utf-8')
            for invalid in [{'error': 'font load failed'}, {}, {'format_version': 1, 'records': []}]:
                with self.assertRaises(ValueError):
                    collector.save_capture(path, invalid, {'cases': [{'id': 'one'}]}, [])
                self.assertEqual(path.read_text(encoding='utf-8'), 'previous')

    def test_dump_dom_result_is_decoded_and_missing_result_fails(self):
        self.assertEqual(collector.parse_dom('<pre id="result">{"text":"&lt;&amp;𠮷"}</pre>'), {'text': '<&𠮷'})
        with self.assertRaises(ValueError):
            collector.parse_dom('<html><body>loading</body></html>')

    def test_corrupt_atomic_geometry_is_rejected_before_replacing_data(self):
        original = json.loads((collector.ROOT/'assets/browser/chromium.json').read_text(encoding='utf-8'))
        inputs = json.loads((collector.ROOT/'assets/browser-inputs.json').read_text(encoding='utf-8'))
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'capture.json'
            path.write_text('previous', encoding='utf-8')
            for bad in [None, {'top': 0, 'bottom': 18, 'width': 25, 'height': 18},
                        {'top': 0, 'bottom': 18, 'width': 24, 'height': float('inf')}]:
                data = json.loads(json.dumps(original))
                record = next(r for r in data['records'] if r['id'] == 'atomic-baseline')
                record['samples'][0]['atomic'] = bad
                with self.assertRaises(ValueError):
                    collector.save_capture(path, data, inputs, original['fonts'])
                self.assertEqual(path.read_text(encoding='utf-8'), 'previous')

    def test_inconsistent_transition_and_missing_probes_preserve_previous_file(self):
        original = json.loads((collector.ROOT/'assets/browser/chromium.json').read_text(encoding='utf-8'))
        inputs = json.loads((collector.ROOT/'assets/browser-inputs.json').read_text(encoding='utf-8'))
        collector.validate_capture(original, inputs, original['fonts'])
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'capture.json'
            path.write_text('previous', encoding='utf-8')
            for defect in ['null', 'already-reached', 1, 7769, 7831, 7832, 7833, 7834, 7835, 7897, 8960]:
                with self.subTest(defect=defect):
                    path.write_text('previous', encoding='utf-8')
                    data = json.loads(json.dumps(original))
                    record = next(r for r in data['records'] if r['id'] == 'color-ffi')
                    if defect == 'null':
                        record['boundary_subpixels'] = None
                        record['samples'] = [record['samples'][0], record['initial']]
                    elif defect == 'already-reached':
                        record['samples'][0]['end_utf16'] = 19
                        record['samples'][0]['end_utf8'] = 19
                    else:
                        record['samples'] = [s for s in record['samples'] if s['width_subpixels'] != defect]
                    with self.assertRaises(ValueError):
                        collector.save_capture(path, data, inputs, original['fonts'])
                    self.assertEqual(path.read_text(encoding='utf-8'), 'previous')
                    self.assertEqual(list(Path(tmp).iterdir()), [path])

if __name__ == '__main__':
    unittest.main()

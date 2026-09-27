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

if __name__ == '__main__':
    unittest.main()

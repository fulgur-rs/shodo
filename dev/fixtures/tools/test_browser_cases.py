import unittest
import browser_cases as cases

class BrowserCasesTests(unittest.TestCase):
    def test_seed_changes_input_reproducibly(self):
        self.assertEqual(cases.xorshift(1), 270369)
        self.assertEqual(cases.generate(), cases.generate())
        generated = cases.generate()['cases']
        self.assertGreaterEqual(len(generated), 54)
        self.assertEqual(len({c['id'] for c in generated}), len(generated))
        self.assertNotEqual(generated[0]['parts'], generated[1]['parts'])

    def test_all_priority_inputs_share_existing_fonts(self):
        generated = cases.generate()['cases']
        ids = {c['id'] for c in generated}
        self.assertTrue({'nested-inline', 'color-ffi', 'arabic-wrap', 'pre-wrap-tab',
                         'japanese-punctuation', 'atomic-baseline', 'supplementary'} <= ids)
        for c in generated:
            self.assertTrue(c['font_ids'])
            self.assertTrue(set(c['font_ids']) <= {'latin', 'cjk', 'arabic'})
            self.assertGreater(c['width_subpixels'], 0)
            self.assertTrue(''.join(p['text'] for p in c['parts']))

if __name__ == '__main__':
    unittest.main()

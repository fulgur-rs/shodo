"""Versioned fixed-font inputs; no system fonts or external random library."""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def xorshift(state):
    state ^= (state << 13) & 0xffffffff
    state ^= state >> 17
    state ^= (state << 5) & 0xffffffff
    return state & 0xffffffff


def generate():
    corpus = {c['id']: c for c in json.loads((ROOT / 'assets/cases.json').read_text(encoding='utf-8'))}
    result = []
    def add(id, seed, text, font, lang, direction='ltr', size=16, width=140, white_space='normal', parts=None):
        result.append(dict(id=id, seed=seed, font_ids=[font], lang=lang, direction=direction,
                           font_size=size, width_subpixels=width*64, white_space=white_space,
                           parts=parts or [dict(text=text, depth=0)]))
    for family, case_id, font, lang, direction in [
        ('latin', 'latin-short', 'latin', 'en', 'ltr'),
        ('japanese', 'japanese-short', 'cjk', 'ja', 'ltr'),
        ('arabic', 'arabic-short', 'arabic', 'ar', 'rtl')]:
        original = corpus[case_id]['text']
        for seed in range(1, 17):
            state = seed
            if family == 'japanese':
                state = xorshift(state)
                start = state % len(original)
                text = (original + original)[start:start+30]
            else:
                words = original.split(' ')
                selected = []
                for _ in range(10):
                    state = xorshift(state)
                    selected.append(words[state % len(words)])
                text = ' '.join(selected)
            add(f'{family}-{seed:02}', seed, text, font, lang, direction,
                size=16+(seed % 4)/4, width=90+(seed % 5)*20)
    add('nested-inline', 1001, '', 'latin', 'en', parts=[
        dict(text='Readers ', depth=0), dict(text='compare ', depth=1),
        dict(text='office ', depth=2), dict(text='words ', depth=1), dict(text='and lines.', depth=0)])
    add('color-ffi', 1002, '', 'latin', 'en', parts=[
        dict(text='f', depth=1, color='#ff0000'), dict(text='f', depth=2, color='#0000ff'),
        dict(text='i office readers compare words.', depth=1, color='#008000')])
    add('arabic-wrap', 1003, corpus['arabic-short']['text'], 'arabic', 'ar', 'rtl', width=130)
    add('pre-wrap-tab', 1004, 'One  two\tthree\nFour five.', 'latin', 'en', width=90, white_space='pre-wrap')
    add('japanese-punctuation', 1005, corpus['japanese-short']['text'], 'cjk', 'ja', width=112)
    add('atomic-baseline', 1006, '', 'latin', 'en', width=90, parts=[
        dict(text='One ', depth=0), dict(text='\ufffc', depth=1, atomic_width=24, atomic_height=18),
        dict(text=' two three.', depth=0)])
    add('supplementary', 1007, corpus['japanese-supplementary']['text'], 'cjk', 'ja', width=80)
    return dict(format_version=1, generator='xorshift32-v1; seeds 1..16; corpus words/30-scalar slices; named structural seeds1001..1007', cases=result)


def check_materialized(path=None):
    path = Path(path) if path is not None else ROOT / 'assets/browser-inputs.json'
    output = json.dumps(generate(), ensure_ascii=False, indent=2)+'\n'
    if path.read_text(encoding='utf-8') != output:
        raise ValueError('browser-inputs.json is stale; run browser_cases.py to regenerate')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    output = json.dumps(generate(), ensure_ascii=False, indent=2)+'\n'
    path = ROOT / 'assets/browser-inputs.json'
    if args.check:
        check_materialized(path)
    else:
        path.write_text(output, encoding='utf-8')

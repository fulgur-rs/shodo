"""Explicit fixed-font headless Chromium recollection; standard library only."""
import argparse
import hashlib
import json
import math
import platform
import shutil
import subprocess
import tempfile
import threading
import browser_cases
from html.parser import HTMLParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def utf16_to_utf8(text, units):
    used = byte = 0
    for ch in text:
        if used == units:
            return byte
        used += 2 if ord(ch) > 0xffff else 1
        byte += len(ch.encode('utf-8'))
        if used > units:
            raise ValueError('UTF-16 endpoint splits a surrogate pair')
    if used != units:
        raise ValueError('UTF-16 endpoint out of range')
    return byte


def parse_dom(dom):
    class Parser(HTMLParser):
        def __init__(self):
            super().__init__(); self.active = False; self.data = []
        def handle_starttag(self, tag, attrs):
            if tag == 'pre' and dict(attrs).get('id') == 'result': self.active = True
        def handle_endtag(self, tag):
            if tag == 'pre': self.active = False
        def handle_data(self, value):
            if self.active: self.data.append(value)
    parser = Parser(); parser.feed(dom)
    if not parser.data:
        raise ValueError('browser did not produce a completed result')
    return json.loads(''.join(parser.data))


def validate_capture(result, inputs, fonts):
    if result.get('error'):
        raise ValueError(result['error'])
    if result.get('format_version') != 1 or not result.get('user_agent') or result.get('fonts') != fonts:
        raise ValueError('missing/wrong capture version, browser or font metadata')
    records = result.get('records', [])
    if [r['id'] for r in records] != [c['id'] for c in inputs['cases']]:
        raise ValueError('incomplete or reordered capture')
    for record, case in zip(records, inputs['cases']):
        text = ''.join(p['text'] for p in case['parts'])
        if record['text'] != text or record['seed'] != case['seed']:
            raise ValueError('capture input differs')
        samples = record['samples']
        if not samples or record['initial']['width_subpixels'] != case['width_subpixels']:
            raise ValueError('missing initial measurement')
        if record['initial'] not in samples:
            raise ValueError('initial sample missing')
        widths = [s['width_subpixels'] for s in samples]
        if widths != sorted(set(widths)) or widths[0] != 1:
            raise ValueError('invalid probe widths')
        for sample in samples:
            if type(sample['width_subpixels']) is not int or sample['width_subpixels'] <= 0:
                raise ValueError('invalid probe width')
            if type(sample['end_utf16']) is not int or sample['end_utf16'] < 0:
                raise ValueError('invalid endpoint')
            if utf16_to_utf8(text, sample['end_utf16']) != sample['end_utf8']:
                raise ValueError('UTF-16/UTF-8 endpoint mismatch')
            atomic_part = next((p for p in case['parts'] if 'atomic_width' in p), None)
            atomic = sample.get('atomic')
            if (atomic_part is not None) != (atomic is not None):
                raise ValueError('missing/unexpected atomic geometry')
            if atomic is not None:
                values = [atomic.get(k) for k in ['top', 'bottom', 'width', 'height']]
                if not all(type(v) in (int, float) and math.isfinite(v) for v in values):
                    raise ValueError('invalid atomic geometry')
                if atomic['width'] != atomic_part['atomic_width'] or atomic['height'] != atomic_part['atomic_height'] or abs(atomic['bottom']-atomic['top']-atomic['height']) > 0.001:
                    raise ValueError('atomic dimensions differ')
        boundary = record['boundary_subpixels']
        target = record['initial']['end_utf16']
        minimum = samples[0]['end_utf16']
        if boundary is None:
            if minimum < target:
                raise ValueError('missing transition for unreached initial target')
        else:
            if type(boundary) is not int or minimum >= target:
                raise ValueError('invalid transition for minimum endpoint')
            required = {1, case['width_subpixels']}
            required.update(boundary + delta for delta in [-64, -2, -1, 0, 1, 2, 64] if boundary + delta > 0)
            if not required.issubset(widths):
                raise ValueError('missing required boundary probes')
            by_width = {s['width_subpixels']: s for s in samples}
            if boundary <= 1 or boundary > case['width_subpixels'] or boundary-1 not in by_width or boundary not in by_width:
                raise ValueError('boundary has no adjacent probes')
            target = record['initial']['end_utf16']
            if not (by_width[boundary-1]['end_utf16'] < target <= by_width[boundary]['end_utf16']):
                raise ValueError('boundary does not bracket target')


def save_capture(path, result, inputs, fonts):
    validate_capture(result, inputs, fonts)
    path = Path(path); path.parent.mkdir(parents=True, exist_ok=True)
    f = tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=path.parent, delete=False)
    tmp = Path(f.name)
    try:
        with f:
            json.dump(result, f, ensure_ascii=False, indent=2, allow_nan=False); f.write('\n')
        tmp.replace(path)
    finally:
        tmp.unlink(missing_ok=True)


def capture(browser, timeout):
    browser_cases.check_materialized()
    inputs_path = ROOT / 'assets/browser-inputs.json'
    inputs = json.loads(inputs_path.read_text(encoding='utf-8'))
    manifest = json.loads((ROOT/'assets/manifest.json').read_text(encoding='utf-8'))
    fonts = manifest['fonts']
    payloads = {}
    for font in fonts:
        data = (ROOT/font['path']).read_bytes()
        if hashlib.sha256(data).hexdigest() != font['sha256'] or font['face_index'] != 0:
            raise ValueError('fixture font hash/index differs')
        payloads['/fonts/'+font['id']] = ('font/otf', data)
    recorder = Path(__file__).with_name('browser_recorder.js').read_bytes()
    config = json.dumps(dict(inputs=inputs, fonts=fonts), ensure_ascii=True).replace('<', '\\u003c')
    payloads['/'] = ('text/html; charset=utf-8', ('<!doctype html><meta charset="utf-8"><script>window.shodoCapture='+config+'</script><script src="/recorder.js"></script>').encode())
    payloads['/recorder.js'] = ('text/javascript', recorder)
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path not in payloads:
                self.send_error(404); return
            content_type, data = payloads[self.path]
            self.send_response(200); self.send_header('Content-Type', content_type)
            self.send_header('Content-Length', str(len(data))); self.end_headers(); self.wfile.write(data)
        def log_message(self, *args): pass
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
    flags = ['--headless','--disable-gpu','--disable-background-networking','--no-first-run',
             '--no-default-browser-check','--dump-dom','--virtual-time-budget=30000']
    try:
        version = subprocess.run([browser, '--version'], check=True, capture_output=True, text=True, timeout=10).stdout.strip()
        with tempfile.TemporaryDirectory(prefix='shodo-browser-') as profile:
            run = subprocess.run([browser, *flags, '--user-data-dir='+profile, f'http://127.0.0.1:{server.server_port}/'],
                                 check=True, capture_output=True, text=True, encoding='utf-8', timeout=timeout)
        result = parse_dom(run.stdout)
    finally:
        server.shutdown(); server.server_close(); thread.join()
    expected_fonts = [dict(id=f['id'], sha256=f['sha256'], face_index=f['face_index']) for f in fonts]
    validate_capture(result, inputs, expected_fonts)
    result['metadata'] = dict(browser_version=version, platform=platform.platform(),
        inputs_sha256=hashlib.sha256(inputs_path.read_bytes()).hexdigest(),
        corpus_sha256=hashlib.sha256((ROOT/'assets/cases.json').read_bytes()).hexdigest(),
        recorder_sha256=hashlib.sha256(recorder).hexdigest(), generator=inputs['generator'],
        subpixels_per_px=64, line_height=40, flags=flags,
        source_endpoint='first later-line scalar; normal collapsible whitespace assigned to preceding line; UTF-16 and UTF-8 original source')
    return result, inputs, expected_fonts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--browser', default=shutil.which('chromium') or shutil.which('google-chrome'))
    parser.add_argument('--output', type=Path, default=ROOT/'assets/browser/chromium.json')
    parser.add_argument('--timeout', type=int, default=90)
    args = parser.parse_args()
    if not args.browser:
        parser.error('Chrome/Chromium executable required for recollection')
    result, inputs, fonts = capture(args.browser, args.timeout)
    save_capture(args.output, result, inputs, fonts)
    print(f"Saved {len(result['records'])} measured cases: {args.output}")

if __name__ == '__main__':
    main()

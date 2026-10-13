#!/usr/bin/env python3
"""Verify offline assets or explicitly reproduce/update the pinned subsets."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, build_opener

FONTTOOLS_VERSION = '4.61.1'
FONT_BUDGET = 768 * 1024
SOURCE_MAX_BYTES = 32 * 1024 * 1024
SOURCE_TRANSFER_SECONDS = 60
SOURCE_CHUNK_BYTES = 64 * 1024

# A fixed worker program, with all source data passed as argv, never shell/code.
# Import this same module without invoking its maintainer CLI.
_SOURCE_WORKER = '''
import importlib.util
import sys
spec = importlib.util.spec_from_file_location('_fixture_source', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
try:
    module._copy_source(sys.argv[2], sys.argv[3] == 'remote', int(sys.argv[4]), sys.stdout.buffer)
except module._SourceTooLarge:
    sys.exit(2)
except Exception:
    sys.exit(1)
'''


class _SourceTooLarge(ValueError):
    pass


class _ClosedRedirectBody:
    def __init__(self, response):
        self.response = response

    def read(self, *args):
        return b''

    def __getattr__(self, name):
        return getattr(self.response, name)


class _RedirectWithoutBody(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        _require_http_source(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)

    def http_error_302(self, req, fp, code, msg, headers):
        # urllib drains redirect bodies with an unbounded read(). This request
        # will use a new connection, so discard/close that body and retain the
        # standard redirect URL, credential, method and loop validation.
        fp.close()
        return super().http_error_302(req, _ClosedRedirectBody(fp), code, msg, headers)

    http_error_301 = http_error_303 = http_error_307 = http_error_308 = http_error_302


def _require_http_source(location):
    # FTP welcome/control replies can accumulate before a bounded body reader
    # runs. Keep remote sources and every redirect on HTTP(S); offline originals
    # remain available through --sources-dir.
    if urlsplit(location).scheme not in ('http', 'https'):
        raise ValueError('remote font sources require HTTP or HTTPS')


def _copy_bounded(stream, output, max_bytes):
    total = 0
    while True:
        chunk = stream.read(min(SOURCE_CHUNK_BYTES, max_bytes - total + 1))
        if not chunk:
            return total
        total += len(chunk)
        if total > max_bytes:
            raise _SourceTooLarge('font source exceeds byte ceiling')
        output.write(chunk)


def _copy_source(location, remote, max_bytes, output):
    if remote:
        _require_http_source(location)
        opener = build_opener(_RedirectWithoutBody())
        with opener.open(location, timeout=SOURCE_TRANSFER_SECONDS) as response:
            copied = _copy_bounded(response, output, max_bytes)
            # HTTPResponse.read(n) can silently return EOF on a truncated
            # Content-Length body, unlike the former read(). Preserve rejection
            # before hashing. Chunked responses use their framing instead.
            if response.headers.get('Transfer-Encoding', '').lower() != 'chunked':
                try:
                    expected = int(response.headers.get('Content-Length'))
                except (TypeError, ValueError):
                    expected = None
                if expected is not None and expected >= 0 and copied != expected:
                    raise ValueError('incomplete font source response')
    else:
        with Path(location).open('rb') as source:
            _copy_bounded(source, output, max_bytes)


def _read_source(location, remote=False):
    # Per-socket timeouts cannot bound DNS/open, slow-drip headers, or a single
    # read's chunk metadata/trailers. Supervise the whole source operation in a
    # disposable process; only a successful, bounded payload reaches hashing.
    deadline = time.monotonic() + SOURCE_TRANSFER_SECONDS
    command = [sys.executable, '-B', '-c', _SOURCE_WORKER, str(Path(__file__).resolve()),
               str(location), 'remote' if remote else 'local', str(SOURCE_MAX_BYTES)]
    with subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL) as worker:
        try:
            data, _ = worker.communicate(timeout=max(0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as error:
            raise ValueError('font source transfer deadline exceeded') from error
        finally:
            # Also recover the worker after KeyboardInterrupt/unexpected errors.
            # Process creation and OS kill/wait cannot be a real-time guarantee.
            if worker.poll() is None:
                worker.kill()
            worker.wait()
        if time.monotonic() > deadline:
            raise ValueError('font source transfer deadline exceeded')
        if worker.returncode == 2 or len(data) > SOURCE_MAX_BYTES:
            raise ValueError(f'font source exceeds {SOURCE_MAX_BYTES} bytes')
        if worker.returncode != 0:
            raise ValueError('font source read failed')
        return data


def digest(data):
    return hashlib.sha256(data).hexdigest()


def validate_source(data, expected):
    if digest(data) != expected:
        raise ValueError('source checksum mismatch')


def read_manifest(root):
    manifest = json.loads((root / 'assets/manifest.json').read_text(encoding="utf-8"))
    if manifest['format_version'] != 1:
        raise ValueError('unsupported manifest version')
    return manifest


def check_assets(root):
    manifest = read_manifest(root)
    total = 0
    for entry in manifest['fonts']:
        data = (root / entry['path']).read_bytes()
        total += len(data)
        if digest(data) != entry['sha256']:
            raise ValueError(f"{entry['id']}: asset checksum mismatch")
        if len(data) != entry['size'] or entry['face_index'] != 0:
            raise ValueError(f"{entry['id']}: size or face index mismatch")
        if data[:4] not in (b'OTTO', b'\x00\x01\x00\x00'):
            raise ValueError(f"{entry['id']}: not a standalone sfnt")
        if 'OFL' not in (root / entry['license']).read_text(encoding="utf-8"):
            raise ValueError(f"{entry['id']}: license notice missing")
    if total > FONT_BUDGET:
        raise ValueError('fixture font budget exceeds 768KiB')
    print('Pinned fixture checksums, sizes, indices, and licenses verified.')


def subset_font(data, entry, cases):
    import fontTools
    from fontTools import subset
    from fontTools.ttLib import TTFont
    if fontTools.__version__ != FONTTOOLS_VERSION:
        raise ValueError(f'FontTools {FONTTOOLS_VERSION} required')
    required = {ch for start, end in entry['extra_unicode_ranges'] for ch in range(start, end + 1)}
    for case in cases:
        if entry['id'] in case['font_ids']:
            required.update(ord(ch) for ch in case['text'])
    font = TTFont(io.BytesIO(data), recalcTimestamp=False)
    options = subset.Options()
    options.layout_features = ['*']
    options.name_IDs = ['*']
    options.name_languages = ['*']
    options.name_legacy = True
    options.notdef_outline = True
    options.recommended_glyphs = True
    sub = subset.Subsetter(options=options)
    sub.populate(unicodes=required)
    sub.subset(font)
    # Rename derived fonts while retaining original copyright/license records.
    names = {
        1: entry['family'], 2: 'Regular', 3: entry['family'] + ';Fixture1',
        4: entry['family'] + ' Regular',
        6: entry['family'].replace(' ', '') + '-Regular',
        16: entry['family'], 17: 'Regular',
    }
    for record in font['name'].names:
        if record.nameID in names:
            record.string = names[record.nameID].encode(record.getEncoding())
    if 'CFF ' in font:
        cff = font['CFF '].cff
        cff.fontNames = [names[6]]
        cff.topDictIndex[0].FullName = names[4]
        cff.topDictIndex[0].FamilyName = names[1]
    out = io.BytesIO()
    font.save(out, reorderTables=True)
    return out.getvalue()


def rebuild(root, sources_dir=None, update=False):
    manifest = read_manifest(root)
    outputs = []
    # Stage every source and result before touching any checked-in file.
    for entry in manifest['fonts']:
        if sources_dir is None:
            data = _read_source(entry['source_url'], remote=True)
        else:
            data = _read_source(sources_dir / entry['source_file'])
        validate_source(data, entry['source_sha256'])
        cases = json.loads((root / entry.get('corpus', 'assets/cases.json')).read_text(encoding='utf-8'))
        result = subset_font(data, entry, cases)
        if not update and digest(result) != entry['sha256']:
            raise ValueError(f"{entry['id']}: rebuilt checksum mismatch")
        outputs.append((entry, result))
    if sum(len(data) for _, data in outputs) > FONT_BUDGET:
        raise ValueError('fixture font budget exceeds 768KiB')
    if update:
        for entry, data in outputs:
            destination = root / entry['path']
            with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as staged:
                staged.write(data)
                name = staged.name
            os.replace(name, destination)
            entry['sha256'], entry['size'] = digest(data), len(data)
        (root / 'assets/manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    print('All pinned subsets reproduced' + (' and explicitly updated.' if update else ' without modifying assets.'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--rebuild', action='store_true')
    mode.add_argument('--update', action='store_true')
    parser.add_argument('--sources-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2] / 'dev' / 'fixtures'
    if args.check:
        check_assets(root)
    else:
        rebuild(root, args.sources_dir, update=args.update)


if __name__ == '__main__':
    main()

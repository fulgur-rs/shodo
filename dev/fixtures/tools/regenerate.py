#!/usr/bin/env python3
"""Verify offline assets or explicitly reproduce/update the pinned subsets."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
from urllib.request import urlopen

FONTTOOLS_VERSION = '4.61.1'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def validate_source(data, expected):
    if digest(data) != expected:
        raise ValueError('source checksum mismatch')


def read_manifest(root):
    manifest = json.loads((root / 'assets/manifest.json').read_text())
    if manifest['format_version'] != 1:
        raise ValueError('unsupported manifest version')
    return manifest


def check_assets(root):
    manifest = read_manifest(root)
    for entry in manifest['fonts']:
        data = (root / entry['path']).read_bytes()
        if digest(data) != entry['sha256']:
            raise ValueError(f"{entry['id']}: asset checksum mismatch")
        if len(data) != entry['size'] or entry['face_index'] != 0:
            raise ValueError(f"{entry['id']}: size or face index mismatch")
        if data[:4] not in (b'OTTO', b'\x00\x01\x00\x00'):
            raise ValueError(f"{entry['id']}: not a standalone sfnt")
        if 'OFL' not in (root / entry['license']).read_text():
            raise ValueError(f"{entry['id']}: license notice missing")
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
    cases = json.loads((root / 'assets/cases.json').read_text())
    outputs = []
    # Stage every source and result before touching any checked-in file.
    for entry in manifest['fonts']:
        if sources_dir is None:
            data = urlopen(entry['source_url'], timeout=60).read()
        else:
            data = (sources_dir / entry['source_file']).read_bytes()
        validate_source(data, entry['source_sha256'])
        result = subset_font(data, entry, cases)
        if not update and digest(result) != entry['sha256']:
            raise ValueError(f"{entry['id']}: rebuilt checksum mismatch")
        outputs.append((entry, result))
    if sum(len(data) for _, data in outputs) > 512 * 1024:
        raise ValueError('fixture font budget exceeds 512KiB')
    if update:
        for entry, data in outputs:
            destination = root / entry['path']
            with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as staged:
                staged.write(data)
                name = staged.name
            os.replace(name, destination)
            entry['sha256'], entry['size'] = digest(data), len(data)
        (root / 'assets/manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print('All pinned subsets reproduced' + (' and explicitly updated.' if update else ' without modifying assets.'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--rebuild', action='store_true')
    mode.add_argument('--update', action='store_true')
    parser.add_argument('--sources-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.check:
        check_assets(root)
    else:
        rebuild(root, args.sources_dir, update=args.update)


if __name__ == '__main__':
    main()

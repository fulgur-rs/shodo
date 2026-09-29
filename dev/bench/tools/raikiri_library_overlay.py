"""Observe original library construction only in separate disposable archives.

This recipe borrows immutable source; no upstream algorithm is checked in here.
The main whole-caller archive/binaries are never modified by this extra mode.
"""
import hashlib
import importlib.util
import io
import json
import subprocess
import tarfile
import tomllib
from pathlib import Path

spec = importlib.util.spec_from_file_location("layout_overlay", Path(__file__).with_name("raikiri_overlay.py"))
layout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(layout)
BUILDER_SHA = "24d99f970db0fdcd788a606ea779620dd1853c61ce3da4d6b3d175ae1bae966b"
PROJECTION_SHA = "5a7f53ec8a10f3d3ba6b5456f5c28f7d8658c7c205204f1d98885fc30b5d5427"
NATIVE_SHA = "f1ac6a3b5cdb9541218e692e994cfaa628423988c09c867f61c11e1e3a8dd383"


def guarded(path, expected):
    original = path.read_bytes()
    if layout.sha(original) != expected:
        raise ValueError(f"original library boundary source differs: {path.name}")
    return original.decode()


def dependency(path, value):
    path.write_text(layout.replace_once(path.read_text(), "[dependencies]\n", "[dependencies]\n" + value + "\n"))


def prepare_library(spike, raikiri, destination):
    destination = Path(destination).resolve()
    raikiri = Path(raikiri).resolve(strict=True)
    if destination == raikiri or raikiri in destination.parents:
        raise ValueError("library output must be outside the upstream checkout")
    provenance = layout.prepare(spike, destination)
    archive = subprocess.check_output(["git", "archive", layout.RAIKIRI_REVISION], cwd=raikiri)
    upstream = destination / "raikiri"
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        if any(not (m.isfile() or m.isdir()) for m in tar.getmembers()):
            raise ValueError("upstream library archive contains links/special files")
        tar.extractall(upstream, filter="data")
    observer = destination / "observer"
    observer.mkdir()
    root = Path(__file__).resolve().parents[3]
    observer_source = root / "dev/raikiri/probe/observer.rs"
    (observer / "lib.rs").write_bytes(observer_source.read_bytes())
    (observer / "Cargo.toml").write_text('''[package]
name = "shodo-benchmark-observer"
version = "0.0.0"
edition = "2024"
publish = false
[workspace]
[lib]
path = "lib.rs"
''')
    builder = destination / "s4/src/builder.rs"
    original = guarded(builder, BUILDER_SHA)
    old = '''        self.close_unbalanced();
        if let Some(e) = self.error {
            return Err(e);
        }
        Paragraph::from_builder(self, cx, fonts)
'''
    observed = '''        shodo_benchmark_observer::start("candidate", shodo_benchmark_observer::current_node(), &self.text);
        let result = (|| {
''' + old + '''        })();
        shodo_benchmark_observer::end();
        result
'''
    builder.write_text(layout.replace_once(original, old, observed))
    projection = destination / "s4/dev/raikiri/src/projection.rs"
    original = guarded(projection, PROJECTION_SHA)
    projection.write_text(layout.replace_once(original,
        '''        paragraph: builder
            .build(context, fonts)
            .map_err(IntegrationError::Limit)?,''',
        '''        paragraph: shodo_benchmark_observer::with_node(root, || builder.build(context, fonts))
            .map_err(IntegrationError::Limit)?,'''))
    native = upstream / "crates/raikiri-dom/src/layout/inline_text.rs"
    original = guarded(native, NATIVE_SHA)
    # Only the existing sequential final job construction is observed. Probe
    # input rejects >=32 original text nodes before the scheduler can parallelize.
    observed = layout.replace_once(original,
        '            let mut builder = layout_cx.ranged_builder(fonts, &job.text, 1.0, quantize_metrics);',
        '            shodo_benchmark_observer::start("native", job.idx, &job.text);\n'
        '            let mut builder = layout_cx.ranged_builder(fonts, &job.text, 1.0, quantize_metrics);')
    observed = layout.replace_once(observed,
        '\n            let mut layout: Layout<()> = builder.build(&job.text);',
        '\n            let mut layout: Layout<()> = builder.build(&job.text);\n'
        '            shodo_benchmark_observer::end();')
    native.write_text(observed)
    dependency(destination / "s4/Cargo.toml", 'shodo-benchmark-observer = { path = "../observer" }')
    dependency(destination / "s4/dev/raikiri/Cargo.toml", 'shodo-benchmark-observer = { path = "../../../observer" }')
    dependency(upstream / "crates/raikiri-dom/Cargo.toml", 'shodo-benchmark-observer = { path = "../../../observer" }')
    manifest = destination / "Cargo.toml"
    dependency(manifest, 'shodo-benchmark-observer = { path = "observer" }')
    value = manifest.read_text().replace('exclude = ["s4"]', 'exclude = ["s4", "raikiri", "observer"]')
    value += '\n[[bin]]\nname = "library-probe"\npath = ' + json.dumps(str(root / "dev/raikiri/probe/library.rs")) + '\n'
    value += '\n[patch."https://github.com/fulgur-rs/raikiri.git"]\n'
    # Patch all internal packages together so original DOM/style/trait types
    # share one identity, retaining original upstream workspace dependency specs.
    members = tomllib.loads((upstream / "Cargo.toml").read_text())["workspace"]["members"]
    packages = {}
    for member in members:
        package = tomllib.loads((upstream / member / "Cargo.toml").read_text())["package"]["name"]
        packages[package] = "raikiri/" + member
    for name, path in sorted(packages.items()):
        value += f'{name} = {{ path = {json.dumps(path)} }}\n'
    manifest.write_text(value)
    provenance.update(library_observed=True, upstream_archive_sha256=layout.sha(archive),
        original_library_sources_sha256={"s4/src/builder.rs":BUILDER_SHA,"s4/dev/raikiri/src/projection.rs":PROJECTION_SHA,
            "raikiri/crates/raikiri-dom/src/layout/inline_text.rs":NATIVE_SHA},
        observed_library_sources_sha256={str(p.relative_to(destination)):layout.sha(p.read_bytes()) for p in [builder,projection,native]},
        observer_sha256=layout.sha(observer_source.read_bytes()), recipe_sha256=layout.sha(Path(__file__).read_bytes()))
    (destination / "library-provenance.json").write_text(json.dumps(provenance,indent=2)+'\n')
    return provenance

#!/usr/bin/env python3
"""Borrow a frozen S4 layout boundary only inside a new disposable archive.

No original source is edited. The generated layout routine is deliberately not
checked into main: its exact original bytes and extraction recipe are recorded.
"""
import hashlib
import io
import json
import subprocess
import tarfile
from pathlib import Path

S4_REVISION = "fe67a281210fbc52d22032911ac3584405aa8198"
RAIKIRI_REVISION = "ab7e619a8f321f03de8b8c8b9342954868e044c8"
PAGE_SOURCE_SHA256 = "a7c1c4b8a0d1f4b11b28cf8e69cee23e11b29abdb5c7cd9605b3fd7e7ab29707"
LOCK_SOURCE_SHA256 = "d41af8a11a1799cd990fd256b9f11e3592ecd07a787ed50e529d4f52374c1d71"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def replace_once(source, old, new):
    if source.count(old) != 1:
        raise ValueError("nonunique or missing original layout extraction anchor")
    return source.replace(old, new, 1)


def expose_layout(source):
    """Add a layout-only entry point, refusing any version of unreviewed code."""
    if sha(source) != PAGE_SOURCE_SHA256:
        raise ValueError("preserved candidate page source SHA differs")
    text = source.decode("utf-8")
    start = text.index("fn paint(\n")
    end = text.index("fn draw_borders(\n", start)
    body = text[start:end]
    body = replace_once(body, "fn paint(\n", "fn benchmark_layout(\n")
    body = replace_once(body, "    bytes: &[&[u8]],\n", "")
    body = replace_once(body, ") -> Result<CandidatePaintedPage, IntegrationError> {", ") -> Result<Vec<CandidateBlock>, IntegrationError> {")
    body = replace_once(body, '''    let mut image = tiny_skia::Pixmap::new(width, height)
        .ok_or(unsupported(body, "candidate page allocation failed"))?;
    image.fill(tiny_skia::Color::WHITE);
''', "")
    paint_start = body.index("        draw_borders(&mut image, cv, id, origin, geometry)?;")
    paint_end = body.index("        bottom = origin[1] + geometry.border_block_size;", paint_start)
    removed = body[paint_start:paint_end]
    for required in ("snapshot_candidate", "paint_glyph_layer", "image.draw_pixmap"):
        if required not in removed:
            raise ValueError("original raster boundary differs")
    body = body[:paint_start] + body[paint_end:]
    body = replace_once(body, "    Ok(CandidatePaintedPage { image, blocks })", "    Ok(blocks)")
    if any(word in body for word in ("Pixmap", "snapshot_candidate", "paint_glyph_layer", "draw_borders")):
        raise ValueError("raster or snapshot work remains in layout-only scope")
    entry = '''
/// Disposable benchmark entry point: original leaf layout, no raster output.
/// Original whole-page support classification must be checked separately.
pub fn layout_candidate_screen_page(
    document: &crate::WptScreenDocument,
    fonts: &FontCollection,
    viewport: [u32; 2],
    limits: &Limits,
) -> Result<Vec<CandidateBlock>, IntegrationError> {
    reject_line_pseudos(&document.parsed.stylesheet_sources)?;
    benchmark_layout(
        DocumentInput::screen(document),
        DocumentProjection::new_screen(document),
        fonts,
        viewport,
        limits,
    )
}
'''
    return (text + entry + body).encode("utf-8")


def expose_page_inputs(source):
    """Borrow original CSS preflight/sizing, stopping before paragraph shaping."""
    if sha(source) != PAGE_SOURCE_SHA256:
        raise ValueError("preserved candidate page source SHA differs")
    text = source.decode("utf-8")
    start = text.index("fn paint(\n")
    end = text.index("        let prepared = projection.build_ifc_in_block(", start)
    # The exact pinned call currently sits on one line. Refuse altered anchors.
    prefix = text[start:end]
    prefix = replace_once(prefix, "fn paint(\n", "fn benchmark_page_inputs(\n")
    for argument in ("    projection: DocumentProjection<'_>,\n", "    bytes: &[&[u8]],\n", "    limits: &Limits,\n"):
        prefix = replace_once(prefix, argument, "")
    prefix = replace_once(prefix, ") -> Result<CandidatePaintedPage, IntegrationError> {", ") -> Result<CandidatePageInputs, IntegrationError> {")
    prefix = replace_once(prefix, '''    let mut image = tiny_skia::Pixmap::new(width, height)
        .ok_or(unsupported(body, "candidate page allocation failed"))?;
    image.fill(tiny_skia::Color::WHITE);
''', "")
    for setup in ("    let mut cx = LayoutContext::new();\n", "    let mut bottom = 0.0f32;\n", "    let mut pending_margin = body_edges.margin.block_start;\n"):
        prefix = replace_once(prefix, setup, "")
    prefix = replace_once(prefix, "        let mut geometry = MeasuredBlock {", "        let geometry = MeasuredBlock {")
    if any(word in prefix for word in ("Pixmap", "Paragraph", "build_ifc", "snapshot_candidate", "paint_glyph_layer")):
        raise ValueError("shaping/raster work remains in CSS-input boundary")
    tail = '''        blocks.push(CandidateBlockInput { node: id, geometry });
    }
    if blocks.is_empty() {
        return Err(unsupported(body, "candidate static page has no supported blocks"));
    }
    Ok(CandidatePageInputs { body_edges, blocks })
}
'''
    entry = '''
/// Disposable observation of the original static caller's CSS input boundary.
pub struct CandidateBlockInput {
    pub node: usize,
    pub geometry: MeasuredBlock,
}
pub struct CandidatePageInputs {
    pub body_edges: shodo::node::InlineEdges,
    pub blocks: Vec<CandidateBlockInput>,
}
pub fn candidate_page_inputs(
    document: &crate::WptScreenDocument,
    fonts: &FontCollection,
    viewport: [u32; 2],
) -> Result<CandidatePageInputs, IntegrationError> {
    reject_line_pseudos(&document.parsed.stylesheet_sources)?;
    benchmark_page_inputs(DocumentInput::screen(document), fonts, viewport)
}
'''
    return (entry + prefix + tail).encode("utf-8")


def prepare(spike, destination, *, expose_layout_boundary=True):
    """Create a new external probe tree, never writing into the supplied spike."""
    spike = Path(spike).resolve(strict=True)
    destination = Path(destination).resolve()
    if destination.exists():
        raise FileExistsError(destination)
    if destination == spike or spike in destination.parents:
        raise ValueError("benchmark output must be outside the protected spike")
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=spike)
    def fingerprint():
        return (git("rev-parse", "HEAD").decode().strip(), sha(git("status", "--porcelain")),
                sha(git("diff", "--binary", "HEAD")))
    checkout_before = fingerprint()
    # A shared checkout may have advanced for another task. Borrow the exact
    # immutable object, never its current HEAD, working source, or index.
    original_page = git("show", f"{S4_REVISION}:dev/raikiri/src/candidate_page.rs")
    if sha(original_page) != PAGE_SOURCE_SHA256:
        raise ValueError("original candidate source differs")
    original_lock = (spike / "Cargo.lock").read_bytes()
    if sha(original_lock) != LOCK_SOURCE_SHA256:
        raise ValueError("preserved ignored S4 lock SHA differs")
    archive = git("archive", S4_REVISION)
    destination.mkdir(parents=True)
    archived = destination / "s4"
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        # Exact git archive, but still disallow filesystem links and special files.
        if any(not (m.isfile() or m.isdir()) for m in tar.getmembers()):
            raise ValueError("archive contains a link or special file")
        tar.extractall(archived, filter="data")
    (archived / "Cargo.lock").write_bytes(original_lock)
    page = archived / "dev/raikiri/src/candidate_page.rs"
    if page.read_bytes() != original_page:
        raise ValueError("archived page differs from the protected source")
    if expose_layout_boundary:
        page.write_bytes(expose_layout(original_page) + expose_page_inputs(original_page))
        lib = archived / "dev/raikiri/src/lib.rs"
        lib.write_bytes(lib.read_bytes() + b"\npub use candidate_page::{layout_candidate_screen_page, candidate_page_inputs, CandidatePageInputs, CandidateBlockInput};\n")
    root = Path(__file__).resolve().parents[2]
    main = root / "dev/raikiri/probe/layout_check.rs"
    manifest = f'''[package]
name = "shodo-raikiri-measurement"
version = "0.0.0"
edition = "2024"
rust-version = "1.89"
publish = false
[workspace]
exclude = ["s4"]
[features]
allocation-counting = []
[dependencies]
shodo = {{ path = "s4", default-features = false, features = ["complex-scripts"] }}
shodo-raikiri-integration = {{ path = "s4/dev/raikiri" }}
raikiri-html = {{ git = "https://github.com/fulgur-rs/raikiri.git", rev = "{RAIKIRI_REVISION}" }}
raikiri-dom = {{ git = "https://github.com/fulgur-rs/raikiri.git", rev = "{RAIKIRI_REVISION}" }}
raikiri-style = {{ git = "https://github.com/fulgur-rs/raikiri.git", rev = "{RAIKIRI_REVISION}" }}
raikiri-traits = {{ git = "https://github.com/fulgur-rs/raikiri.git", rev = "{RAIKIRI_REVISION}" }}
parley = {{ version = "0.11", default-features = false, features = ["std"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
sha2 = "0.10.9"
[[bin]]
name = "layout-check"
path = {json.dumps(str(main))}
[[bin]]
name = "core-contract-check"
path = {json.dumps(str(root / "dev/raikiri/probe/core_contract_check.rs"))}
[[bin]]
name = "measurement-probe"
path = {json.dumps(str(root / "dev/raikiri/probe/main.rs"))}
'''
    (destination / "Cargo.toml").write_text(manifest)
    (destination / "Cargo.lock").write_bytes(original_lock)
    provenance = dict(spike=str(spike), stage=str(destination), revision=S4_REVISION,
        archive_sha256=sha(archive), original_lock_sha256=sha(original_lock),
        layout_exposed=expose_layout_boundary, original_page_sha256=sha(original_page),
        generated_page_sha256=sha(page.read_bytes()), recipe_sha256=sha(Path(__file__).read_bytes()),
        acceptance_cli_sha256=sha(main.read_bytes()))
    provenance["checkout_observed"] = dict(head=checkout_before[0], status_sha256=checkout_before[1], diff_sha256=checkout_before[2])
    provenance["probe_sources_sha256"] = {
        str(path.relative_to(root)): sha(path.read_bytes())
        for path in sorted((root / "dev/raikiri/probe").glob("*.rs"))
    }
    provenance["allocator_source_sha256"] = sha((root / "dev/bench/src/allocator.rs").read_bytes())
    (destination / "archive-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    if fingerprint() != checkout_before or (spike / "Cargo.lock").read_bytes() != original_lock:
        raise ValueError("protected checkout changed during archive preparation")
    return provenance

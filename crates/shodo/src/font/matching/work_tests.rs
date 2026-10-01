//! Observe actual work on oversized clusters, without copying them per query.
use super::*;
use crate::font::FontOptions;
use crate::node::{InlineEdges, NodeId, TextSource};
use crate::style::{InlineStyle, ParagraphStyle};
use crate::{LayoutContext, Paragraph, ParagraphBuilder};
use std::cell::Cell;

thread_local! {
    static HASH_BYTES: Cell<usize> = const { Cell::new(0) };
    static SCALAR_VISITS: Cell<usize> = const { Cell::new(0) };
    static CMAP_PROBES: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_hash(bytes: usize) {
    if bytes > 4096 {
        HASH_BYTES.with(|n| n.set(n.get() + bytes));
    }
}

pub(super) fn record_scalar(bytes: usize) {
    if bytes > 4096 {
        SCALAR_VISITS.with(|n| n.set(n.get() + 1));
    }
}

pub(super) fn record_cmap(bytes: usize) {
    if bytes > 4096 {
        CMAP_PROBES.with(|n| n.set(n.get() + 1));
    }
}

fn reset_work() {
    HASH_BYTES.with(|n| n.set(0));
    SCALAR_VISITS.with(|n| n.set(0));
    CMAP_PROBES.with(|n| n.set(0));
}

fn fonts(cap: usize) -> FontCollection {
    FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: cap,
            ..Default::default()
        },
    )
}

fn marks() -> String {
    (0x300..0x310).map(|n| char::from_u32(n).unwrap()).collect()
}

fn split_grapheme(fonts: &FontCollection, queries: usize, first_line: bool) -> Paragraph {
    let normal = InlineStyle {
        font_families: vec![FontFamily::Named("Web".into())],
        ..Default::default()
    };
    let style = ParagraphStyle {
        root: normal.clone(),
        first_line: first_line.then(|| InlineStyle {
            font_weight: 700.,
            ..normal.clone()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    let part = marks().repeat(256 / queries);
    for i in 0..queries {
        let style = InlineStyle {
            font_width: 50. + i as f32,
            ..normal.clone()
        };
        let node = NodeId(2 + i as u64);
        b.open_inline(node, &style, InlineEdges::default());
        b.push_text(TextSource::Generated { node }, &part);
        b.close_inline();
    }
    b.build(&mut LayoutContext::new(), fonts).unwrap()
}

#[test]
fn long_grapheme_queries_do_not_hash_uncacheable_text() {
    for cap in [0, 64] {
        for queries in [8, 32] {
            reset_work();
            let p = split_grapheme(&fonts(cap), queries, false);
            assert_eq!(p.text(), format!("a{}", marks().repeat(256)));
            assert_eq!(HASH_BYTES.with(Cell::get), 0, "cap={cap}/queries={queries}");
        }
    }
}

#[test]
fn long_grapheme_auto_presentation_is_summarized_once() {
    for queries in [8, 32] {
        reset_work();
        let p = split_grapheme(&fonts(0), queries, false);
        assert_eq!(p.text().chars().count(), 4097);
        let visits = SCALAR_VISITS.with(Cell::get);
        assert!(
            visits <= 4097,
            "{visits} scalar visits for {queries} queries"
        );
    }
}

#[test]
fn long_grapheme_coverage_is_shared_across_queries_and_first_line() {
    for cap in [0, 64] {
        for queries in [8, 32] {
            for first_line in [false, true] {
                let fonts = fonts(cap);
                let chars: Vec<_> = std::iter::once('a').chain(marks().chars()).collect();
                let id = fonts
                    .register_face(
                        super::super::browser_tests::test_font("Web", &chars, 600),
                        0,
                        FontFaceDescriptor {
                            family: "Web".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                reset_work();
                let p = split_grapheme(&fonts, queries, first_line);
                assert!(
                    p.data
                        .shape_items
                        .iter()
                        .all(|item| { item.font.as_ref().is_some_and(|font| font.id == id) })
                );
                let builds = if first_line { 2 } else { 1 };
                let probes = CMAP_PROBES.with(Cell::get);
                let visits = SCALAR_VISITS.with(Cell::get);
                assert!(
                    probes <= 17 * builds,
                    "{probes} cmap probes: cap={cap}/queries={queries}/first_line={first_line}"
                );
                assert!(visits <= 4097 * builds, "{visits} source scalar visits");
                assert_eq!(p.text(), format!("a{}", marks().repeat(256)));
            }
        }
    }
}

fn temporary(data: FontData, ranges: Vec<(u32, u32)>) -> Candidate {
    let info = face_info(&data).unwrap();
    Candidate {
        id: FontId {
            layer: 0,
            index: u32::MAX,
        },
        data,
        descriptor: FontFaceDescriptor {
            family: "Web".into(),
            unicode_ranges: ranges,
            ..Default::default()
        },
        info,
        color: false,
    }
}

#[test]
fn prepared_coverage_keeps_native_blobs_faces_and_unicode_ranges_distinct() {
    // Both TTC faces share a blob and temporary id, but only the first covers
    // the combining mark. A third blob has the same face index as the first.
    let first = super::super::browser_tests::test_font("Full", &['a', '\u{301}'], 600);
    let second = super::super::browser_tests::test_font("Partial", &['a'], 600);
    let offsets = [20, 20 + first.len()];
    let mut ttc = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
    for offset in offsets {
        ttc.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    for (mut face, offset) in [(first, offsets[0]), (second.clone(), offsets[1])] {
        let count = u16::from_be_bytes(face[4..6].try_into().unwrap()) as usize;
        for table in 0..count {
            let at = 12 + table * 16 + 8;
            let value = u32::from_be_bytes(face[at..at + 4].try_into().unwrap());
            face[at..at + 4].copy_from_slice(&(value + offset as u32).to_be_bytes());
        }
        ttc.extend(face);
    }
    let blob = super::super::Blob::from(ttc);
    let full = temporary(FontData::new(blob.clone(), 0), vec![]);
    let restricted = temporary(FontData::new(blob.clone(), 0), vec![(0x61, 0x61)]);
    let partial = temporary(FontData::new(blob, 1), vec![]);
    let other = temporary(FontData::new(super::super::Blob::from(second), 0), vec![]);
    let text = format!("a{}", "\u{301}\u{200d}\u{fe0f}\u{e0100}".repeat(1024));
    let mut cluster = FontCluster::new(&text);
    for (candidate, expected) in [
        (&full, true),
        (&restricted, false),
        (&partial, false),
        (&other, false),
        (&full, true),
    ] {
        let font = FontRef::from_index(candidate.data.data.as_ref(), candidate.data.index).unwrap();
        assert_eq!(cluster.covers(candidate, &font), expected);
    }
}

#[test]
fn long_cluster_presentation_keeps_last_selector_and_explicit_preference() {
    for (base, suffix, expected) in [
        ("a", "", false),
        ("😀", "", true),
        ("a", "\u{fe0e}\u{fe0f}", true),
        ("😀", "\u{fe0f}\u{fe0e}", false),
    ] {
        let text = format!("{base}{}{suffix}", "\u{301}".repeat(4096));
        let cluster = FontCluster::new(&text);
        assert_eq!(cluster.prefer_color(FontPresentation::Auto), expected);
        assert!(cluster.prefer_color(FontPresentation::Emoji));
        assert!(!cluster.prefer_color(FontPresentation::Text));
    }
}

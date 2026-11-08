use super::*;
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::style::{FontFamily, ParagraphStyle};
use std::sync::atomic::Ordering;

/// The fixture Latin face with GPOS kerning pairs: each `(first, second,
/// kern)` moves `first` by `kern` font units before `second`, which makes the
/// start of `second` unsafe to break after `first`. Nothing else kerns.
fn pair_kerned_latin(pairs: &[(char, char, i16)]) -> Vec<u8> {
    use skrifa::MetadataProvider;
    let bytes = crate::test_support::fonts::LATIN;
    let charmap = skrifa::FontRef::from_index(bytes, 0).unwrap();
    let glyph = |c| charmap.charmap().map(c).unwrap().to_u32() as u16;
    let mut firsts: Vec<u16> = pairs.iter().map(|p| glyph(p.0)).collect();
    firsts.sort_unstable();
    firsts.dedup();
    let mut gpos = Vec::new();
    // Header, ScriptList (latn, default LangSys using feature 0).
    for value in [1u16, 0, 10, 30, 44, 1] {
        gpos.extend(value.to_be_bytes());
    }
    gpos.extend(b"latn");
    for value in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
        gpos.extend(value.to_be_bytes());
    }
    // FeatureList (kern -> lookup 0), LookupList with one PairPos lookup.
    gpos.extend(b"kern");
    for value in [8u16, 0, 1, 0, 1, 4, 2, 0, 1, 8] {
        gpos.extend(value.to_be_bytes());
    }
    // PairPosFormat1: XAdvance on the first glyph, one pair set per first
    // glyph in coverage order, then the coverage.
    let sets: Vec<Vec<u16>> = firsts
        .iter()
        .map(|first| {
            let mut records: Vec<(u16, i16)> = pairs
                .iter()
                .filter(|p| glyph(p.0) == *first)
                .map(|p| (glyph(p.1), p.2))
                .collect();
            records.sort_unstable();
            let mut set = vec![records.len() as u16];
            for (second, kern) in records {
                set.extend([second, kern as u16]);
            }
            set
        })
        .collect();
    let header = 5 + firsts.len();
    let sets_len: usize = sets.iter().map(Vec::len).sum();
    let mut subtable = vec![
        1u16,
        ((header + sets_len) * 2) as u16,
        4,
        0,
        firsts.len() as u16,
    ];
    let mut at = header;
    for set in &sets {
        subtable.push((at * 2) as u16);
        at += set.len();
    }
    subtable.extend(sets.concat());
    subtable.extend([1, firsts.len() as u16]);
    subtable.extend(&firsts);
    for value in subtable {
        gpos.extend(value.to_be_bytes());
    }
    let mut tables: Vec<_> = crate::font::sfnt::read_sfnt_tables(bytes)
        .into_iter()
        .filter(|(tag, _)| tag != b"GPOS")
        .collect();
    tables.push((*b"GPOS", gpos));
    tables.sort_by_key(|(tag, _)| *tag);
    crate::font::sfnt::build_sfnt(&tables)
}

fn paragraph(font: Vec<u8>, text: &str) -> crate::Paragraph {
    paragraph_spaced(font, text, 0.0)
}

fn paragraph_spaced(font: Vec<u8>, text: &str, letter_spacing: f32) -> crate::Paragraph {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            font,
            0,
            FontFaceDescriptor {
                family: "Pair".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Pair".into())];
    style.root.letter_spacing = letter_spacing;
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

/// The word after the kerned space starts a min-content line. Its first
/// cluster is reshaped alone, and the probe that adds the next cluster finds
/// the boundary safe, so the probe's prefix is the window.
#[test]
fn safe_probe_prefix_is_the_window_shape() {
    let p = paragraph(pair_kerned_latin(&[(' ', '1', -200)]), "x 12");
    let data = &*p.data;
    let start = data
        .units
        .iter()
        .position(|u| &data.text[u.text.start as usize..u.text.end as usize] == "1")
        .unwrap();
    assert!(data.units[start - 1].unsafe_to_break, "the pair must apply");
    let mut cx = LayoutContext::new();
    let before = data.edge_shape_calls.load(Ordering::Relaxed);
    let delta = delta(
        data,
        start,
        data.units.len(),
        &mut cx,
        &mut Saturation::default(),
    );
    assert_eq!(data.edge_shape_calls.load(Ordering::Relaxed) - before, 1);
    // The same window measured with the window shaped on its own.
    PROBE_PREFIX_DISABLED.with(|d| d.set(true));
    let reference = delta_fresh(data, start);
    PROBE_PREFIX_DISABLED.with(|d| d.set(false));
    assert_eq!(delta, reference);
}

fn delta_fresh(data: &ParagraphData, start: usize) -> LayoutUnit {
    delta(
        data,
        start,
        data.units.len(),
        &mut LayoutContext::new(),
        &mut Saturation::default(),
    )
}

fn layout(p: &crate::Paragraph, widths: &[f32]) -> Vec<String> {
    let mut cx = LayoutContext::new();
    let options = crate::style::LineOptions::default();
    let sizes = p.intrinsic_sizes(&mut cx, &options, &crate::AtomicIntrinsics::EMPTY);
    let mut out = vec![format!("{sizes:?}")];
    for width in widths
        .iter()
        .copied()
        .chain([sizes.min_content, sizes.max_content])
    {
        for line in p.break_all(&mut cx, &options, width, &crate::AtomicSizes::EMPTY) {
            out.push(format!(
                "{line:?} {:?}",
                line.fragments().collect::<Vec<_>>()
            ));
        }
    }
    out
}

/// Intrinsic sizes and lines at many widths are the same whether windows
/// take a safe probe's prefix or are shaped on their own, and no paragraph
/// shapes more windows for it: a probe whose prefix is unusable, or whose
/// window later grows, still serves the loop's own probe. In test builds
/// every prefix taken is also compared with its own shaping.
#[test]
fn probe_prefixes_leave_layout_unchanged() {
    let fonts = [
        pair_kerned_latin(&[(' ', '1', -200)]),
        pair_kerned_latin(&[(' ', 'o', 150)]),
        pair_kerned_latin(&[('a', 'b', -300), (' ', 'a', 100)]),
    ];
    let texts = [
        "x 12 1ab 123 1",
        "of office of 1o oo",
        "ab cab abab a b",
        "x 12\u{a0}34 1-2 (1) 11",
        "1 1 1 1 11 111",
    ];
    let (mut fast_calls, mut reference_calls) = (0, 0);
    for (font, letter_spacing) in fonts.iter().flat_map(|f| [(f, 0.0), (f, 1.5)]) {
        for text in texts {
            let p = paragraph_spaced(font.clone(), text, letter_spacing);
            let calls = || p.data.edge_shape_calls.load(Ordering::Relaxed);
            let widths = [0.0, 8.0, 20.0, 40.0];
            let before = calls();
            let fast = layout(&p, &widths);
            let fast_case = calls() - before;
            PROBE_PREFIX_DISABLED.with(|d| d.set(true));
            let before = calls();
            let reference = layout(&p, &widths);
            let reference_case = calls() - before;
            PROBE_PREFIX_DISABLED.with(|d| d.set(false));
            assert_eq!(fast, reference, "{text:?}");
            assert!(fast_case <= reference_case, "{text:?}");
            fast_calls += fast_case;
            reference_calls += reference_case;
        }
    }
    // The cases above must exercise prefixes.
    assert!(
        fast_calls < reference_calls,
        "{fast_calls} >= {reference_calls}"
    );
}

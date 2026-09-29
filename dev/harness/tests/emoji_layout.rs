//! Mutations caught: component font fallback, lost GSUB, interior emoji breaks
//! or editing stops, wrong retained font instance, and dropped UTF8 source bytes.
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::hit::{CaretDirection, LineLayout, NavigationOrder, TextPosition};
use shodo::limits::{Limits, WarningKind};
use shodo::mapping::{Affinity, TextOrigin};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, OverflowWrap, ParagraphStyle};
use shodo::{
    AtomicSizes, Fragment, GlyphRunView, LayoutContext, Line, Paragraph, ParagraphBuilder,
};
use shodo_fixtures::{EMOJI_FONTS, load_emoji_fonts, load_fonts};

const SCOTLAND: &str = "🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}";
// Literal IDs and hmtx widths from the pinned subset cmap/ordered GSUB tables,
// independently inspected with FontTools (not shodo or harfrust output).
const SEQUENCES: &[(&str, usize, u32, u32)] = &[
    ("😀", 4, 16, 20),
    ("☺︎", 6, 5, 9),
    ("☺️", 6, 5, 9),
    ("👍🏽", 8, 27, 14),
    ("👩‍💻", 11, 58, 21),
    ("👨‍👩‍👧‍👦", 25, 43, 45),
    ("🇯🇵", 8, 25, 69),
    ("1️⃣", 7, 24, 63),
    (SCOTLAND, 28, 26, 70),
];
fn style() -> ParagraphStyle {
    ParagraphStyle {
        root: InlineStyle {
            font_size: 24.,
            font_families: vec![FontFamily::Named(EMOJI_FONTS[0].family.into())],
            ..Default::default()
        },
        ..Default::default()
    }
}
fn build(text: &str, style: &ParagraphStyle, fonts: &FontCollection) -> Paragraph {
    let limits = Limits::default();
    let mut b = ParagraphBuilder::new(style, &limits);
    b.with_offset_mapping(true).push_text(
        TextSource::Dom {
            node: NodeId(42),
            offset: 10,
        },
        text,
    );
    b.build(&mut LayoutContext::new(), fonts).unwrap()
}
fn lines(p: &Paragraph, width: f32) -> Vec<Line> {
    p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        width,
        &AtomicSizes::EMPTY,
    )
}
fn runs(line: &Line) -> impl Iterator<Item = GlyphRunView<'_>> {
    line.fragments().filter_map(|f| {
        if let Fragment::GlyphRun(r) = f {
            Some(r)
        } else {
            None
        }
    })
}
fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.04, "{actual} != {expected}");
}
fn position(line: usize, offset: usize, affinity: Affinity) -> TextPosition {
    TextPosition {
        line,
        offset: offset as u32,
        affinity,
    }
}
fn indivisible(line: &Line, start: usize, end: usize) {
    let ls = std::slice::from_ref(line);
    let layout = LineLayout::new(ls);
    for byte in start + 1..end {
        for (affinity, want) in [(Affinity::Upstream, start), (Affinity::Downstream, end)] {
            assert_eq!(
                layout
                    .caret(position(0, byte, affinity))
                    .unwrap()
                    .position
                    .offset as usize,
                want
            );
        }
    }
    for order in [NavigationOrder::Logical, NavigationOrder::Visual] {
        assert_eq!(
            layout
                .move_caret(
                    position(0, start, Affinity::Downstream),
                    CaretDirection::Forward,
                    order
                )
                .unwrap()
                .offset as usize,
            end
        );
    }
    let a = position(0, start, Affinity::Downstream);
    let b = position(0, end, Affinity::Upstream);
    let selected = layout.selection_rects(a, b);
    assert_eq!(selected.len(), 1);
    let rect = selected[0];
    assert!(rect.inline_size > 0. && rect.block_size > 0.);
    for t in [0.1, 0.3, 0.7, 0.9] {
        let hit = layout
            .hit_test(
                rect.inline_start + rect.inline_size * t,
                rect.block_start + rect.block_size * 0.5,
            )
            .unwrap();
        assert!([start, end].contains(&(hit.position.offset as usize)));
    }
    if end - start > 2 {
        assert_eq!(
            layout.selection_rects(
                position(0, start + 1, Affinity::Upstream),
                position(0, end - 1, Affinity::Downstream)
            ),
            selected
        );
    }
}

#[test]
fn real_sequences_shape_table_derived_glyphs_and_instances() {
    let fonts = load_emoji_fonts(&Limits::default()).unwrap();
    for &(text, len, color, mono) in SEQUENCES {
        assert_eq!(text.len(), len);
        let p = build(text, &style(), &fonts.base.collection);
        assert!(p.warnings().is_empty(), "{text:?} {:?}", p.warnings());
        let ls = lines(&p, 1000.);
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0].text_range(), 0..len);
        let expected_mono = text == "☺︎";
        let expected_id = fonts.emoji_ids[usize::from(expected_mono)];
        let positive: Vec<_> = runs(&ls[0])
            .flat_map(|r| r.glyphs())
            .filter(|g| g.advance > 0.)
            .collect();
        assert_eq!(positive.len(), 1, "{text:?}");
        assert_eq!(
            positive[0].id,
            if expected_mono { mono } else { color },
            "{text:?}"
        );
        close(
            ls[0].inline_size(),
            (if expected_mono { 2600. } else { 2550. }) * 24. / 2048.,
        );
        for r in runs(&ls[0]) {
            assert_eq!(r.font(), expected_id);
            assert_eq!(
                r.font_data().unwrap().data.as_ref(),
                EMOJI_FONTS[usize::from(expected_mono)].bytes
            );
            assert_eq!(r.font_size(), 24.);
            close(r.metrics().ascent, 1900. * 24. / 2048.);
            close(r.metrics().descent, 500. * 24. / 2048.);
        }
        indivisible(&ls[0], 0, len);
    }
    // A mono-only collection tests all real monochrome sequences independently
    // of the color preference; it is still whole-grapheme font selection.
    let mono = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let id = mono
        .register_face(
            EMOJI_FONTS[1].bytes.to_vec(),
            0,
            FontFaceDescriptor {
                family: EMOJI_FONTS[1].family.into(),
                ..Default::default()
            },
        )
        .unwrap();
    for &(text, _, _, want) in SEQUENCES {
        let p = build(text, &style(), &mono);
        assert!(p.warnings().is_empty());
        let ls = lines(&p, 1000.);
        let positive: Vec<_> = runs(&ls[0])
            .flat_map(|r| r.glyphs())
            .filter(|g| g.advance > 0.)
            .collect();
        assert_eq!(
            positive.iter().map(|g| g.id).collect::<Vec<_>>(),
            [want],
            "mono {text:?}"
        );
        assert!(runs(&ls[0]).all(|r| r.font() == id));
        close(ls[0].inline_size(), 2600. * 24. / 2048.);
    }
}

#[test]
fn compound_emoji_preserve_break_and_edit_boundaries() {
    let fonts = load_emoji_fonts(&Limits::default()).unwrap();
    for &(text, len, _, _) in SEQUENCES {
        for wrap in [
            OverflowWrap::Normal,
            OverflowWrap::Anywhere,
            OverflowWrap::BreakWord,
        ] {
            let mut s = style();
            s.root.overflow_wrap = wrap;
            let p = build(&text.repeat(2), &s, &fonts.base.collection);
            let ls = lines(&p, 1.);
            let ranges: Vec<_> = ls.iter().map(Line::text_range).collect();
            if wrap != OverflowWrap::Normal {
                assert_eq!(ranges, [0..len, len..len * 2], "{text:?} {wrap:?}");
            }
            // Normal line breaking may prohibit a break between numeric keycaps;
            // overflowing a whole pair is valid, splitting either keycap is not.
            assert_eq!(ranges.first().unwrap().start, 0);
            assert_eq!(ranges.last().unwrap().end, len * 2);
            for pair in ranges.windows(2) {
                assert_eq!(pair[0].end, pair[1].start);
            }
            for line in &ls {
                assert!(line.inline_size().is_finite() && line.block_size() > 0.);
                let range = line.text_range();
                assert_eq!(range.start % len, 0);
                assert_eq!(range.end % len, 0);
                for start in (range.start..range.end).step_by(len) {
                    indivisible(line, start, start + len);
                }
            }
        }
    }
}

#[test]
fn mixed_japanese_latin_metrics_and_utf8_offsets_are_preserved() {
    let fonts = load_emoji_fonts(&Limits::default()).unwrap();
    let text = "日本語A👩‍💻B";
    let p = build(text, &style(), &fonts.base.collection);
    let ls = lines(&p, 1000.);
    assert_eq!(ls.len(), 1);
    let line = &ls[0];
    assert_eq!(line.text_range(), 0..22);
    // CJK1000upem x3; Latin A639/B650upem1000; emoji2550upem2048.
    close(
        line.inline_size(),
        72. + (639. + 650.) * 24. / 1000. + 2550. * 24. / 2048. + 3.,
    );
    close(line.metrics().ascent, 1160. * 24. / 1000.);
    close(line.metrics().descent, 293. * 24. / 1000.);
    close(line.block_size(), 1160. * 24. / 1000. + 293. * 24. / 1000.);
    indivisible(line, 10, 21);
    let mapping = line.offset_mapping().unwrap();
    for offset in [0, 3, 6, 9, 10, 21, 22] {
        assert_eq!(
            mapping.text_to_dom(offset, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(42),
                offset: 10 + offset
            })
        );
    }
    let narrow = lines(&p, 35.);
    for l in &narrow {
        assert!(!(11..21).contains(&l.text_range().start));
        assert!(!(11..21).contains(&l.text_range().end));
    }
}

#[test]
fn split_dom_components_keep_one_grapheme_and_both_sources() {
    let fonts = load_emoji_fonts(&Limits::default()).unwrap();
    for (a, b, expected) in [
        ("👩", "\u{200d}💻", 58),
        ("☺", "\u{fe0f}", 5),
        ("🇯", "🇵", 25),
    ] {
        let mut builder = ParagraphBuilder::new(&style(), &Limits::default());
        builder
            .with_offset_mapping(true)
            .push_text(
                TextSource::Dom {
                    node: NodeId(1),
                    offset: 7,
                },
                a,
            )
            .push_text(
                TextSource::Dom {
                    node: NodeId(2),
                    offset: 19,
                },
                b,
            );
        let p = builder
            .build(&mut LayoutContext::new(), &fonts.base.collection)
            .unwrap();
        let ls = lines(&p, 1.);
        assert_eq!(ls.len(), 1);
        let positive: Vec<_> = runs(&ls[0])
            .flat_map(|r| r.glyphs())
            .filter(|g| g.advance > 0.)
            .collect();
        assert_eq!(
            positive.iter().map(|g| g.id).collect::<Vec<_>>(),
            [expected]
        );
        indivisible(&ls[0], 0, a.len() + b.len());
        let m = ls[0].offset_mapping().unwrap();
        assert_eq!(
            m.text_to_dom(0, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 7
            })
        );
        assert_eq!(
            m.text_to_dom(a.len() as u32, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(2),
                offset: 19
            })
        );
        assert_eq!(
            m.text_to_dom((a.len() + b.len()) as u32, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(2),
                offset: 19 + b.len() as u32
            })
        );
    }
}

#[test]
fn unsupported_sequence_keeps_one_grapheme() {
    let fonts = load_emoji_fonts(&Limits::default()).unwrap();
    let p = build("😀\u{200d}😀", &style(), &fonts.base.collection);
    let ls = lines(&p, 1.);
    assert_eq!(ls.len(), 1);
    let positive: Vec<_> = runs(&ls[0])
        .flat_map(|r| r.glyphs())
        .filter(|g| g.advance > 0.)
        .collect();
    assert_eq!(positive.iter().map(|g| g.id).collect::<Vec<_>>(), [16, 16]);
    assert!(runs(&ls[0]).all(|r| r.font() == fonts.emoji_ids[0]));
    indivisible(&ls[0], 0, 11);

    // Hide only the real GSUB directory record, leaving cmap/metrics/bitmaps.
    // Actual shaping of the same face must yield the two nominal components.
    let mut bytes = EMOJI_FONTS[0].bytes.to_vec();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let record = (12..12 + count * 16)
        .step_by(16)
        .find(|i| &bytes[*i..*i + 4] == b"GSUB")
        .unwrap();
    bytes[record..record + 4].copy_from_slice(b"TEST");
    let collection = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let id = collection
        .register_face(
            bytes,
            0,
            FontFaceDescriptor {
                family: EMOJI_FONTS[0].family.into(),
                ..Default::default()
            },
        )
        .unwrap();
    let p = build("👩‍💻", &style(), &collection);
    let ls = lines(&p, 1.);
    assert_eq!(ls.len(), 1);
    let positive: Vec<_> = runs(&ls[0])
        .flat_map(|r| r.glyphs())
        .filter(|g| g.advance > 0.)
        .collect();
    assert_eq!(positive.iter().map(|g| g.id).collect::<Vec<_>>(), [14, 15]);
    assert!(runs(&ls[0]).all(|r| r.font() == id));
    indivisible(&ls[0], 0, 11);
}

#[test]
fn missing_emoji_warns_and_retains_sources() {
    let base = load_fonts(&Limits::default()).unwrap();
    let emoji = load_emoji_fonts(&Limits::default()).unwrap();
    for (fonts, text) in [
        (&base.collection, "👩‍💻"),
        (&emoji.base.collection, "🫠"),
        (&emoji.base.collection, "👩\u{200d}🫠"),
    ] {
        let p = build(text, &style(), fonts);
        assert!(
            p.warnings()
                .iter()
                .any(|w| w.kind == WarningKind::Unsupported)
        );
        let ls = lines(&p, 1.);
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0].text_range(), 0..text.len());
        assert!(runs(&ls[0]).flat_map(|r| r.glyphs()).all(|g| g.id == 0));
        indivisible(&ls[0], 0, text.len());
        let m = ls[0].offset_mapping().unwrap();
        assert_eq!(
            m.text_to_dom(text.len() as u32, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(42),
                offset: 10 + text.len() as u32
            })
        );
    }
}

#[test]
fn monochrome_variable_instance_survives_to_output() {
    let fonts = load_emoji_fonts(&Limits::default()).unwrap();
    for (weight, coord) in [(400., 0.), (700., 1.)] {
        let mut s = style();
        s.root.font_weight = weight;
        let p = build("☺︎", &s, &fonts.base.collection);
        let ls = lines(&p, 1000.);
        for run in runs(&ls[0]) {
            assert_eq!(run.font(), fonts.emoji_ids[1]);
            assert_eq!(run.font_data().unwrap().data.as_ref(), EMOJI_FONTS[1].bytes);
            close(
                run.normalized_coords().first().map_or(0., |c| c.to_f32()),
                coord,
            );
            assert!(
                run.variations()
                    .iter()
                    .any(|v| v.tag == *b"wght" && v.value == weight)
            );
            assert!(!run.embolden());
        }
    }
}

//! Real shaping must constrain the correspondence, including source fragments.
use crate::analysis::units::{BreakClass, UnitKind};
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{
    FontFamily, InlineStyle, LineBreak, ParagraphStyle, TextTransform, TextWrapMode,
};
use crate::{LayoutContext, Paragraph, ParagraphBuilder};

fn fonts() -> FontCollection {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, bytes) in [
        ("Shodo Fixture CJK", crate::test_support::fonts::CJK),
        ("Shodo Fixture Latin", crate::test_support::fonts::LATIN),
        (
            "Shodo Fixture Emoji",
            crate::test_support::fonts::EMOJI_COLOR,
        ),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    fonts
}

fn style(family: &str) -> InlineStyle {
    InlineStyle {
        font_size: 20.0,
        font_families: vec![FontFamily::Named(family.into())],
        line_break: LineBreak::Anywhere,
        ..Default::default()
    }
}

fn content(node: u64, text: &str, style: &InlineStyle) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 30,
        },
        text,
        style,
        &Limits::default(),
    )
}

fn pair(base: RubyContent, reading: RubyContent) -> Ruby {
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: base,
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: reading,
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap()
}

fn build(ruby: Ruby) -> Paragraph {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &style("Shodo Fixture CJK"), ruby);
    b.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

fn cursor_storage_fixture(count: usize, full_lanes: usize, first_line: bool) -> ParagraphBuilder {
    let limits = Limits::default();
    let mut root = style("Shodo Fixture CJK");
    root.font_size = 16.0;
    root.line_break = LineBreak::Normal;
    let mut small = root.clone();
    small.font_size = 8.0;
    small.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
    let mut first = root.clone();
    first.font_size = 20.0;
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        first_line: first_line.then_some(first),
        ..Default::default()
    };
    let bases = (0..if full_lanes == 0 { count } else { 1 })
        .map(|i| RubyBase {
            node: NodeId(10000 + i as u64),
            content: content(
                10000 + i as u64,
                &"日".repeat(if full_lanes == 0 { 1 } else { count }),
                &root,
            ),
            align: RubyAlign::Start,
        })
        .collect();
    let levels = (0..full_lanes.max(1))
        .map(|level| RubyLevel {
            annotations: (0..if full_lanes == 0 { count } else { 1 })
                .map(|i| RubyAnnotation {
                    node: NodeId(20000 + (level * count + i) as u64),
                    content: content(
                        20000 + (level * count + i) as u64,
                        &if full_lanes == 0 {
                            "aaa".into()
                        } else {
                            "に".repeat(count)
                        },
                        &if full_lanes == 0 {
                            small.clone()
                        } else {
                            let mut s = root.clone();
                            s.font_size = 8.0;
                            s
                        },
                    ),
                    span: RubySpan::Columns(if full_lanes == 0 { i..i + 1 } else { 0..1 }),
                    visibility: RubyVisibility::Visible,
                })
                .collect(),
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        })
        .collect();
    let ruby = Ruby::new(bases, levels).unwrap();
    let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
    builder.push_ruby(NodeId(1), &root, ruby);
    builder
}

fn cursor_table_bytes(ruby: &crate::ruby::prepare::PreparedRuby) -> usize {
    ruby.cuts.capacity() * std::mem::size_of::<crate::ruby::cuts::PairedCut>()
        + ruby
            .cuts
            .first()
            .map_or(0, |cut| cut.lanes.table_payload_bytes())
}

#[test]
fn short_spanned_lanes_retain_changes_instead_of_every_row() {
    let paragraph = cursor_storage_fixture(64, 0, false)
        .build(&mut LayoutContext::new(), &fonts())
        .unwrap();
    let ruby = &paragraph.data.ruby.containers[0];
    assert_eq!(ruby.cuts.len(), 65);
    for (row, cut) in ruby.cuts.iter().enumerate() {
        for lane in 0..64 {
            let end = ruby.lanes[lane].paragraph.data.units.len();
            assert_eq!(cut.lanes[lane], if row <= lane { 0 } else { end });
        }
    }
    assert!(
        cursor_table_bytes(ruby) < 16_384,
        "64 short lanes must retain cursor changes within 16KiB, actual {} bytes",
        cursor_table_bytes(ruby)
    );
}

#[test]
fn many_dense_lanes_do_not_retain_column_metadata() {
    for first_line in [false, true] {
        let paragraph = cursor_storage_fixture(7, 64, first_line)
            .build(&mut LayoutContext::new(), &fonts())
            .unwrap();
        let tables = std::iter::once(&paragraph.data)
            .chain(paragraph.data.first_line.iter().map(|first| &first.data));
        for data in tables {
            let ruby = &data.ruby.containers[0];
            assert_eq!((ruby.cuts.len(), ruby.lanes.len()), (8, 64));
            for (lane, prepared) in ruby.lanes.iter().enumerate() {
                let child = &prepared.paragraph.data;
                let cluster_ends: Vec<_> = child
                    .units
                    .iter()
                    .enumerate()
                    .filter(|(_, unit)| matches!(unit.kind, UnitKind::Cluster { .. }))
                    .map(|(index, _)| index + 1)
                    .collect();
                assert_eq!(cluster_ends.len(), 7);
                for (row, cut) in ruby.cuts.iter().enumerate() {
                    let expected = match row {
                        0 => 0,
                        7 => child.units.len(),
                        _ => cluster_ends[row - 1],
                    };
                    assert_eq!(cut.lanes[lane], expected);
                }
            }
            // Original 8 rows * (40-byte metadata + 64 full usize cursors).
            assert!(
                cursor_table_bytes(ruby) < 4_416,
                "dense64 must fit the original table payload; actual {} bytes",
                cursor_table_bytes(ruby)
            );
        }
    }
}

#[test]
#[ignore = "prints actual cursor storage diagnostics for the performance record"]
fn cursor_storage_diagnostic() {
    use std::hash::{Hash, Hasher};
    let fonts = fonts();
    for (count, full_lanes, first_line) in [
        (16, 0, false),
        (64, 0, false),
        (256, 0, false),
        (512, 0, false),
        (512, 0, true),
        (64, 1, false),
        (512, 1, false),
        (512, 4, false),
    ] {
        let mut context = LayoutContext::new();
        let paragraph = cursor_storage_fixture(count, full_lanes, first_line)
            .build(&mut context, &fonts)
            .unwrap();
        assert!(context.take_warnings().is_empty());
        let mut tables = vec![("normal", &paragraph.data)];
        if let Some(first) = &paragraph.data.first_line {
            tables.push(("first-line", &first.data));
        }
        for (kind, data) in tables {
            let ruby = &data.ruby.containers[0];
            let rows = ruby.cuts.len();
            let lanes = ruby.lanes.len();
            let mut changes = 0;
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            for (row, cut) in ruby.cuts.iter().enumerate() {
                cut.unit.hash(&mut hash);
                (cut.class as u8).hash(&mut hash);
                for lane in 0..lanes {
                    cut.lanes[lane].hash(&mut hash);
                    if row > 0 && cut.lanes[lane] != ruby.cuts[row - 1].lanes[lane] {
                        changes += 1;
                    }
                }
            }
            let cells: usize = ruby.cuts.iter().map(|cut| cut.lanes.len()).sum();
            let metadata_bytes =
                ruby.cuts.capacity() * std::mem::size_of::<crate::ruby::cuts::PairedCut>();
            let table_payload = ruby
                .cuts
                .first()
                .map_or(0, |cut| cut.lanes.table_payload_bytes());
            let dense_lanes = ruby.cuts.first().map_or(0, |cut| cut.lanes.dense_columns());
            assert_eq!(cells, rows * lanes);
            println!(
                "CURSOR_STORAGE columns={count} full_lanes={full_lanes} kind={kind} rows={rows} lanes={lanes} logical_cells={cells} changes={changes} metadata_bytes={metadata_bytes} table_payload_bytes={table_payload} retained_payload_bytes={} dense_lanes={dense_lanes} fingerprint={:016x}",
                cursor_table_bytes(ruby),
                hash.finish()
            );
        }
    }
    let error = cursor_storage_fixture(1024, 0, false)
        .build(&mut LayoutContext::new(), &fonts)
        .unwrap_err();
    println!("CURSOR_STORAGE default_C1024_refusal={error:?}");
    assert_eq!(error.kind, crate::limits::LimitKind::Items);
    assert_eq!(error.limit, 1 << 20);
    assert_eq!(error.actual, 1064969);
}

#[test]
fn shared_cluster_source_slices_keep_one_ligature_unbroken() {
    let mut base = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style("Shodo Fixture Latin"),
            ..Default::default()
        },
        &Limits::default(),
    );
    base.push_text(
        TextSource::Dom {
            node: NodeId(11),
            offset: 40,
        },
        "f",
    )
    .push_text(
        TextSource::Dom {
            node: NodeId(12),
            offset: 70,
        },
        "fi",
    );
    let p = build(pair(
        RubyContent::from_builder(base),
        content(20, "にほん", &style("Shodo Fixture CJK")),
    ));
    assert_eq!(p.data.glyphs.len(), 1, "fixture must really form ffi");
    let slices: Vec<_> = p
        .data
        .units
        .iter()
        .filter_map(|u| u.shared_cluster.as_ref().map(|c| c.slices.len()))
        .collect();
    // Anywhere exposes three selectable characters sharing the one glyph;
    // DOM source ownership is independently checked below for both nodes.
    assert_eq!(slices, [3, 3, 3]);
    assert_eq!(p.data.ruby.containers[0].cuts.len(), 2);
    for (node, offset) in [(NodeId(11), 41), (NodeId(12), 72)] {
        assert!(
            p.offset_mapping()
                .unwrap()
                .dom_to_text(node, offset)
                .is_some()
        );
    }
}

#[test]
fn shared_cluster_source_slices_keep_zwj_reading_unbroken() {
    let p = build(pair(
        content(10, "日本語", &style("Shodo Fixture CJK")),
        content(20, "👩‍💻", &style("Shodo Fixture Emoji")),
    ));
    let ruby = &p.data.ruby.containers[0];
    let child = &ruby.lanes[0].paragraph;
    assert_eq!(
        child.data.glyphs.len(),
        1,
        "fixture must really form the ZWJ glyph"
    );
    assert_ne!(child.data.glyphs.id[0], 0);
    assert_eq!(
        ruby.cuts.len(),
        2,
        "an indivisible reading prevents interior base cuts"
    );
    assert_eq!(ruby.cuts.last().unwrap().lanes, [child.data.units.len()]);
}

#[test]
fn transformed_source_expansion_is_indivisible_in_prepared_cuts() {
    let s = InlineStyle {
        text_transform: TextTransform::Uppercase,
        ..style("Shodo Fixture Latin")
    };
    let p = build(pair(
        content(10, "ßa", &s),
        content(20, "にほん", &style("Shodo Fixture CJK")),
    ));
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(
        ruby.cuts.len(),
        3,
        "one original ß plus a permits one interior cut"
    );
    let internal = ruby.cuts[1].unit;
    assert!(matches!(
        p.data.units[internal - 1].kind,
        UnitKind::Cluster { .. }
    ));
    let text = &p.data.units[internal - 1].text;
    assert_eq!(&p.text()[text.start as usize..text.end as usize], "S");
    assert_eq!(text.end as usize, p.text().find("SSA").unwrap() + 2);
}

#[test]
fn prepared_multi_level_cuts_advance_every_parallel_reading() {
    let mut ruby = pair(
        content(10, "日本語", &style("Shodo Fixture CJK")),
        content(20, "にほんご", &style("Shodo Fixture CJK")),
    );
    ruby.levels.push(RubyLevel {
        annotations: vec![RubyAnnotation {
            node: NodeId(21),
            content: content(21, "かな", &style("Shodo Fixture CJK")),
            span: RubySpan::All,
            visibility: RubyVisibility::Hidden,
        }],
        style: RubyStyle::default(),
    });
    let p = build(ruby);
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(ruby.cuts.len(), 3);
    for cuts in ruby.cuts.windows(2) {
        assert!(cuts[0].unit < cuts[1].unit);
        assert!(
            cuts[0]
                .lanes
                .iter()
                .zip(cuts[1].lanes.iter())
                .all(|(a, b)| a < b)
        );
    }
}

#[test]
fn nowrap_partial_annotation_does_not_block_the_next_base() {
    let s = style("Shodo Fixture CJK");
    let ruby = Ruby::new(
        vec![
            RubyBase {
                node: NodeId(10),
                content: content(10, "日", &s),
                align: RubyAlign::default(),
            },
            RubyBase {
                node: NodeId(11),
                content: content(11, "本語", &s),
                align: RubyAlign::default(),
            },
        ],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: content(
                    20,
                    "にほん",
                    &InlineStyle {
                        text_wrap_mode: TextWrapMode::NoWrap,
                        ..s.clone()
                    },
                ),
                span: RubySpan::Columns(0..1),
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let p = build(ruby);
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(
        ruby.cuts.len(),
        4,
        "start, between bases, inside second base, end"
    );
    assert_eq!(ruby.cuts[1].unit, ruby.columns[0].units.end);
    assert_eq!(ruby.cuts[1].lanes, ruby.cuts.last().unwrap().lanes);
    assert_eq!(ruby.cuts[1].class, BreakClass::Allowed);
}

#[test]
fn baseless_annotation_has_distinct_complete_endpoint_cursors() {
    let ruby = Ruby::new(
        Vec::new(),
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: content(20, "にほん", &style("Shodo Fixture CJK")),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let p = build(ruby);
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(ruby.columns[0].node, None);
    assert_eq!(ruby.cuts.len(), 2);
    assert!(ruby.cuts[0].unit < ruby.cuts[1].unit);
    assert_eq!(
        ruby.cuts[1].lanes,
        [ruby.lanes[0].paragraph.data.units.len()]
    );
}

#[test]
fn aggregate_nested_limits_include_annotations_in_base_snapshots() {
    let s = style("Shodo Fixture CJK");
    let inner = pair(content(10, "日", &s), content(20, "に", &s));
    let mut base = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    base.push_ruby(NodeId(18), &s, inner);
    let ruby = pair(RubyContent::from_builder(base), content(20, "にほん", &s));
    // Nested base stream:27 bytes/25 items/4 styles. Readings:9+15 bytes,
    // 5+5 items and2+2 styles. Two pairings and cut tables add16 items; the two-container interval index adds4 cells.
    for (kind, total) in [
        (crate::limits::LimitKind::TextBytes, 51),
        (crate::limits::LimitKind::Items, 55),
        (crate::limits::LimitKind::Styles, 8),
        (crate::limits::LimitKind::ShapedGlyphs, 5),
    ] {
        for cap in [total - 1, total] {
            let mut limits = Limits::default();
            match kind {
                crate::limits::LimitKind::TextBytes => limits.max_text_bytes = Some(cap),
                crate::limits::LimitKind::Items => limits.max_items = Some(cap),
                crate::limits::LimitKind::Styles => limits.max_styles = Some(cap),
                crate::limits::LimitKind::ShapedGlyphs => limits.max_shaped_glyphs = Some(cap),
                _ => unreachable!(),
            }
            let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            b.push_ruby(NodeId(8), &s, ruby.clone());
            let result = b.build(&mut LayoutContext::new(), &fonts());
            if cap < total {
                let e = result.expect_err("nested readings share every aggregate resource cap");
                assert_eq!((e.kind, e.limit), (kind, cap));
            } else {
                let p = result.unwrap();
                assert_eq!(p.data.ruby.containers.len(), 2);
                let outer = &p.data.ruby.containers[0];
                let inner = &p.data.ruby.containers[1];
                assert!(outer.units.start < inner.units.start && inner.units.end < outer.units.end);
                assert_eq!(outer.cuts.len(), 2);
            }
        }
    }
}

#[test]
fn aggregate_limits_include_text_styles_metadata_and_cut_cells() {
    let s = style("Shodo Fixture CJK");
    let ruby = pair(content(10, "日", &s), content(20, "にほん", &s));
    // Parent: 3 source bytes + 4 isolate controls =15, annotation 9+6=15.
    // Items: 13 parent + 5 annotation + 4 pairing metadata + 4 cut cells + 2 interval-index cells=28.
    // Styles: parent root, base text and isolated box; annotation root and
    // isolated box=5. Glyphs: 1+3=4.
    for (kind, total) in [
        (crate::limits::LimitKind::TextBytes, 30),
        (crate::limits::LimitKind::Items, 28),
        (crate::limits::LimitKind::Styles, 5),
        (crate::limits::LimitKind::ShapedGlyphs, 4),
    ] {
        for cap in [total - 1, total] {
            let mut limits = Limits::default();
            match kind {
                crate::limits::LimitKind::TextBytes => limits.max_text_bytes = Some(cap),
                crate::limits::LimitKind::Items => limits.max_items = Some(cap),
                crate::limits::LimitKind::Styles => limits.max_styles = Some(cap),
                crate::limits::LimitKind::ShapedGlyphs => limits.max_shaped_glyphs = Some(cap),
                _ => unreachable!(),
            }
            let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            b.push_ruby(NodeId(8), &s, ruby.clone());
            let result = b.build(&mut LayoutContext::new(), &fonts());
            if cap < total {
                let e = result.expect_err("each kind counts the complete retained graph");
                assert_eq!((e.kind, e.limit), (kind, cap));
            } else {
                result.unwrap_or_else(|e| panic!("exact {kind:?} cap {total} should fit: {e:?}"));
            }
        }
    }
}

#[test]
fn annotation_keeps_its_own_glyph_limit_under_a_larger_parent_cap() {
    let s = style("Shodo Fixture CJK");
    let reading = RubyContent::text(
        TextSource::Dom {
            node: NodeId(20),
            offset: 0,
        },
        "にほん",
        &s,
        &Limits {
            max_shaped_glyphs: Some(2),
            ..Default::default()
        },
    );
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &s, pair(content(10, "日", &s), reading));
    let e = b
        .build(&mut LayoutContext::new(), &fonts())
        .expect_err("own annotation cap still applies");
    assert_eq!(
        (e.kind, e.limit),
        (crate::limits::LimitKind::ShapedGlyphs, 2)
    );
}

#[test]
fn mandatory_base_break_coordinates_a_nowrap_annotation() {
    let s = style("Shodo Fixture CJK");
    let mut base = ParagraphBuilder::new(
        &ParagraphStyle {
            root: s.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    base.push_text(
        TextSource::Dom {
            node: NodeId(10),
            offset: 0,
        },
        "日",
    )
    .push_forced_break(NodeId(17))
    .push_text(
        TextSource::Dom {
            node: NodeId(11),
            offset: 0,
        },
        "本",
    );
    let p = build(pair(
        RubyContent::from_builder(base),
        content(
            20,
            "にほん",
            &InlineStyle {
                text_wrap_mode: TextWrapMode::NoWrap,
                ..s
            },
        ),
    ));
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(ruby.cuts.len(), 3);
    assert_eq!(ruby.cuts[1].class, BreakClass::Mandatory);
    assert!(matches!(
        p.data.units[ruby.cuts[1].unit - 1].kind,
        UnitKind::ForcedBreak
    ));
    assert_eq!(ruby.cuts[1].lanes, ruby.cuts[2].lanes);
}

#[test]
fn spanning_annotation_forbids_the_between_base_cut() {
    let s = style("Shodo Fixture CJK");
    let ruby = Ruby::new(
        vec![
            RubyBase {
                node: NodeId(10),
                content: content(10, "日本", &s),
                align: RubyAlign::default(),
            },
            RubyBase {
                node: NodeId(11),
                content: content(11, "語日", &s),
                align: RubyAlign::default(),
            },
        ],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: content(20, "にほんご", &s),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let p = build(ruby);
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(ruby.cuts.len(), 4, "both base interiors remain legal");
    assert!(
        !ruby
            .cuts
            .iter()
            .any(|cut| cut.unit == ruby.columns[0].units.end)
    );
}

#[test]
fn merged_annotations_retain_pairings_across_base_breaks() {
    let s = style("Shodo Fixture CJK");
    let ruby = Ruby::new(
        vec![
            RubyBase {
                node: NodeId(10),
                content: content(10, "日", &s),
                align: RubyAlign::default(),
            },
            RubyBase {
                node: NodeId(11),
                content: content(11, "本", &s),
                align: RubyAlign::default(),
            },
        ],
        vec![RubyLevel {
            annotations: vec![
                RubyAnnotation {
                    node: NodeId(20),
                    content: content(20, "に", &s),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                },
                RubyAnnotation {
                    node: NodeId(21),
                    content: content(21, "ほん", &s),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                },
            ],
            style: RubyStyle {
                merge: RubyMerge::Merge,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let p = build(ruby);
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(ruby.levels[0].merge, RubyMerge::Merge);
    assert_eq!(ruby.lanes[0].columns, 0..1);
    assert_eq!(ruby.lanes[1].columns, 1..2);
    assert_eq!(ruby.cuts.len(), 3);
    assert_eq!(
        ruby.cuts[1].lanes,
        [ruby.lanes[0].paragraph.data.units.len(), 0]
    );
}

#[test]
fn first_line_prepared_cuts_map_to_normal_source_cursors() {
    let s = style("Shodo Fixture Latin");
    let first = InlineStyle {
        text_transform: TextTransform::Uppercase,
        font_size: 28.0,
        ..s.clone()
    };
    let mut base = ParagraphBuilder::new(
        &ParagraphStyle {
            root: s.clone(),
            first_line: Some(first.clone()),
            ..Default::default()
        },
        &Limits::default(),
    );
    base.push_text(
        TextSource::Dom {
            node: NodeId(10),
            offset: 30,
        },
        "ßa",
    );
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: s,
            first_line: Some(first),
            ..Default::default()
        },
        &Limits::default(),
    );
    reading.push_text(
        TextSource::Dom {
            node: NodeId(20),
            offset: 70,
        },
        "ßa",
    );
    let mut ruby = pair(
        RubyContent::from_builder(base),
        RubyContent::from_builder(reading),
    );
    // Same-original annotations are explicitly retained under Merge.
    ruby.levels[0].style.merge = RubyMerge::Merge;
    let p = build(ruby);
    let normal = &p.data.ruby.containers[0];
    let first = p.data.first_line.as_ref().unwrap();
    let alternate = &first.data.ruby.containers[0];
    assert_eq!((normal.cuts.len(), alternate.cuts.len()), (3, 3));
    let child_first = normal.lanes[0].paragraph.data.first_line.as_ref().unwrap();
    for (normal, alt) in normal.cuts.iter().zip(&alternate.cuts) {
        assert_eq!(first.normal_cursors[alt.unit], Some(normal.unit as u32));
        assert_eq!(
            child_first.normal_cursors[alt.lanes[0]],
            Some(normal.lanes[0] as u32)
        );
    }
}

#[test]
fn long_prepared_ruby_cut_index_is_linear() {
    fn measured(count: usize) -> usize {
        let s = style("Shodo Fixture CJK");
        crate::ruby::index::take_visits();
        crate::ruby::cuts::take_visits();
        let p = build(pair(
            content(10, &"日".repeat(count), &s),
            content(20, &"に".repeat(count), &s),
        ));
        assert_eq!(p.data.ruby.containers[0].cuts.len(), count + 1);
        crate::ruby::index::take_visits() + crate::ruby::cuts::take_visits()
    }
    let small = measured(64);
    let large = measured(128);
    assert!(small > 0);
    assert!(
        large <= 3 * small,
        "doubling real prepared input took {small}→{large} index visits"
    );
}

#[test]
fn text_combine_in_a_vertical_base_remains_indivisible() {
    let s = InlineStyle {
        text_combine_upright: crate::style::TextCombineUpright::All,
        ..style("Shodo Fixture Latin")
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode: crate::geometry::WritingMode::VerticalRl,
            ..Default::default()
        },
        &Limits::default(),
    );
    b.push_ruby(
        NodeId(8),
        &s,
        pair(
            content(10, "123", &s),
            content(20, "にほん", &style("Shodo Fixture CJK")),
        ),
    );
    let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert_eq!(p.data.combine_spans.len(), 1);
    assert_eq!(p.data.ruby.containers[0].cuts.len(), 2);
}

#[test]
fn inter_character_reading_is_upright_vertical_and_has_no_interior_cuts() {
    let s = style("Shodo Fixture CJK");
    let mut ruby = pair(content(10, "日本語", &s), content(20, "にほん", &s));
    ruby.levels[0].style.position = RubyPosition::InterCharacter;
    let p = build(ruby);
    let ruby = &p.data.ruby.containers[0];
    let child = &ruby.lanes[0].paragraph;
    assert_eq!(
        child.data.style.writing_mode,
        crate::geometry::WritingMode::VerticalRl
    );
    assert!(
        child
            .data
            .styles
            .iter()
            .all(|s| s.text_orientation == crate::style::TextOrientation::Upright)
    );
    assert_eq!(ruby.cuts.len(), 2);
}

#[test]
fn nested_ruby_in_annotation_respects_its_parallel_cuts_and_global_budget() {
    let s = style("Shodo Fixture CJK");
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: s.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    reading.push_ruby(
        NodeId(18),
        &s,
        pair(
            content(11, "日本", &s),
            content(21, "👩‍💻", &style("Shodo Fixture Emoji")),
        ),
    );
    let ruby = pair(
        content(10, "日本語", &s),
        RubyContent::from_builder(reading),
    );
    // Three outer bases + two child bases + one nested annotation glyph.
    for cap in [5, 6] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle::default(),
            &Limits {
                max_shaped_glyphs: Some(cap),
                ..Default::default()
            },
        );
        b.push_ruby(NodeId(8), &s, ruby.clone());
        let result = b.build(&mut LayoutContext::new(), &fonts());
        if cap == 5 {
            let e = result.expect_err("nested reading glyphs share the outer budget");
            assert_eq!(
                (e.kind, e.limit),
                (crate::limits::LimitKind::ShapedGlyphs, 5)
            );
        } else {
            let p = result.unwrap();
            let outer = &p.data.ruby.containers[0];
            assert_eq!(
                outer.lanes[0].paragraph.data.ruby.containers[0].cuts.len(),
                2
            );
            assert_eq!(
                outer.cuts.len(),
                2,
                "a nested indivisible reading constrains the parent"
            );
        }
    }
}

#[test]
fn annotation_unicode_forced_breaks_collapse_without_losing_dom_endpoints() {
    let s = InlineStyle {
        white_space_collapse: crate::style::WhiteSpaceCollapse::Preserve,
        ..style("Shodo Fixture Latin")
    };
    let p = build(pair(
        content(10, "日本", &style("Shodo Fixture CJK")),
        content(20, "a\u{2028}b\u{2029}a\u{85}b", &s),
    ));
    let child = &p.data.ruby.containers[0].lanes[0].paragraph;
    let logical: String = child
        .text()
        .chars()
        .filter(|ch| !matches!(*ch, '\u{2066}'..='\u{2069}'))
        .collect();
    assert_eq!(logical, "a b a b");
    assert!(
        !child
            .data
            .units
            .iter()
            .any(|u| matches!(u.kind, UnitKind::ForcedBreak))
    );
    let mapping = child.offset_mapping().unwrap();
    for offset in [30, 31, 34, 35, 38, 39, 41, 42] {
        assert!(
            mapping.dom_to_text(NodeId(20), offset).is_some(),
            "source endpoint {offset}"
        );
    }
}

#[test]
fn annotation_wrapper_counts_against_its_own_nesting_limit() {
    let s = style("Shodo Fixture CJK");
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: s.clone(),
            ..Default::default()
        },
        &Limits {
            max_nesting_depth: Some(1),
            ..Default::default()
        },
    );
    reading
        .open_inline(NodeId(21), &s, crate::node::InlineEdges::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(20),
                offset: 0,
            },
            "にほん",
        )
        .close_inline();
    assert_eq!(reading.error(), None, "the snapshot itself has one level");
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(
        NodeId(8),
        &s,
        pair(content(10, "日", &s), RubyContent::from_builder(reading)),
    );
    let e = b
        .build(&mut LayoutContext::new(), &fonts())
        .expect_err("annotation isolation adds a second level");
    assert_eq!(
        (e.kind, e.limit, e.actual),
        (crate::limits::LimitKind::NestingDepth, 1, 2)
    );
}

#[test]
fn prepared_parallel_cuts_replace_ordinary_base_break_classes() {
    let p = build(pair(
        content(10, "日本語", &style("Shodo Fixture CJK")),
        content(20, "👩‍💻", &style("Shodo Fixture Emoji")),
    ));
    let ruby = &p.data.ruby.containers[0];
    assert_eq!(ruby.cuts.len(), 2);
    assert!(
        p.data.units[ruby.units.clone()]
            .iter()
            .all(|u| u.break_after == BreakClass::Prohibited),
        "the ordinary scanner must not split a base while its actual reading is indivisible"
    );
}

#[test]
fn nested_ruby_preserves_the_normal_break_after_its_outer_container() {
    let s = style("Shodo Fixture CJK");
    let mut base = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    base.push_ruby(
        NodeId(18),
        &s,
        pair(content(11, "日", &s), content(21, "に", &s)),
    );
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(
        NodeId(8),
        &s,
        pair(RubyContent::from_builder(base), content(20, "にほん", &s)),
    )
    .push_text(
        TextSource::Dom {
            node: NodeId(30),
            offset: 0,
        },
        "本",
    );
    let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    let end = p.data.ruby.containers[0].units.end;
    assert_eq!(
        p.data.units[end - 1].break_after,
        BreakClass::Allowed,
        "restricting the nested base must not erase the outer normal-CSS boundary"
    );
}

#[test]
fn consuming_ruby_builder_prepares_both_sets_without_metadata_clones() {
    // Restoring the whole-input clone in paragraph construction fails the
    // actual Clone count, even if source and geometry happen to stay equal.
    for first_line in [false, true] {
        let builder = cursor_storage_fixture(16, 2, first_line);
        let fonts = fonts();
        crate::ruby::builder::input_clone_probe::reset();
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(
            crate::ruby::builder::input_clone_probe::count(),
            0,
            "consuming owned input must not duplicate ruby metadata"
        );
        assert_eq!(p.data.ruby.containers.len(), 1);
        assert_eq!(p.data.ruby.containers[0].lanes.len(), 2);
        if first_line {
            let alternate = &p.data.first_line.as_ref().unwrap().data;
            assert_eq!(alternate.ruby.containers.len(), 1);
            assert_eq!(alternate.ruby.containers[0].lanes.len(), 2);
        }
    }
}

use super::index;
use super::*;
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{FontFamily, InlineStyle, LineBreak, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder};
thread_local! {
    static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
pub(super) fn visit() {
    VISITS.with(|n| n.set(n.get() + 1));
}
fn paragraph(count: usize) -> Paragraph {
    paragraph_with_text(count, "a", WritingMode::HorizontalTb)
}
fn paragraph_with_text(count: usize, text: &str, mode: WritingMode) -> Paragraph {
    paragraph_with_emphasis(count, text, mode, false)
}
fn paragraph_with_emphasis(
    count: usize,
    text: &str,
    mode: WritingMode,
    emphasis: bool,
) -> Paragraph {
    let limits = Limits::default();
    let style = InlineStyle {
        text_emphasis: emphasis.then_some(crate::style::TextEmphasis {
            shape: crate::style::TextEmphasisShape::Dot,
            filled: true,
            position: crate::style::TextEmphasisPosition::OverRight,
        }),
        line_break: LineBreak::Anywhere,
        font_families: vec![FontFamily::Named("Geometry Test".into())],
        ..Default::default()
    };
    let content = |text| {
        RubyContent::text(
            TextSource::Generated { node: NodeId(1) },
            text,
            &style,
            &limits,
        )
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode: mode,
            ..Default::default()
        },
        &limits,
    );
    for i in 0..count {
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(2),
                content: content(text),
                align: RubyAlign::Start,
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(3),
                    content: content("b"),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle {
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        b.push_ruby(NodeId(10 + i as u64), &style, ruby);
    }
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Geometry Test".into(),
                ..Default::default()
            },
        )
        .unwrap();
    b.build(&mut LayoutContext::new(), &fonts).unwrap()
}
#[test]
fn sibling_geometry_does_not_rescan_all_records_or_prior_fragments() {
    for count in [64, 128] {
        let p = paragraph(count);
        VISITS.with(|n| n.set(0));
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1_000_000.0,
            &AtomicSizes::EMPTY,
        );
        let visits = VISITS.with(|n| n.get());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].ruby_annotations().count(), count);
        let records = lines[0].fragments.len();
        assert!(
            visits <= 16 * (records + count),
            "count={count}, records={records}, visits={visits}"
        );
    }
}

#[test]
fn indexed_bounds_match_record_membership_for_bidi_and_continuations() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        let p = paragraph_with_text(4, "ab אב cd ef", mode);
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            110.0,
            &AtomicSizes::EMPTY,
        );
        assert!(lines.len() > 1, "{mode:?}: {} lines", lines.len());
        for line in lines {
            let units = line.units.start as usize..line.units.end as usize;
            // Unequal shifts make a missing visual fragment observable in the
            // union even when the fixed font gives all glyphs equal metrics.
            let shifts: Vec<_> = line
                .block_shifts
                .iter()
                .enumerate()
                .map(|(i, shift)| *shift + LayoutUnit::from_raw((i as i32 % 7 - 3) * 64))
                .collect();
            let frame = Frame::new(
                &p.data,
                units.clone(),
                &line.fragments,
                &line.overlay_runs,
                line.baseline,
                &shifts,
                line.block_size,
            );
            let mut sat = Saturation::default();
            let index = index::RecordBounds::new(&frame, &mut sat);
            let measure = crate::ruby::measure::candidate(
                &p.data,
                units.start,
                units.end,
                &AtomicSizes::EMPTY,
                &mut LayoutContext::new(),
                &mut sat,
            );
            for fragment in &measure.fragments {
                let ruby = &p.data.ruby.containers[fragment.container];
                for (i, units) in fragment
                    .bases
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| !u.is_empty())
                {
                    let mut got = frame.column_content(fragment, i, &mut sat);
                    if let Some(extra) = index.column(
                        &frame,
                        ruby.columns[fragment.column_start + i].box_index,
                        units,
                    ) {
                        got = got.union(extra);
                    }
                    let want = frame.column_area_reference(fragment, i, &mut sat);
                    assert_eq!((got.top, got.bottom), (want.top, want.bottom));
                }
            }
            // Also exercise the anonymous-column rule at every source boundary,
            // including controls with zero-length text and clipped clusters.
            for start in units.clone() {
                for end in [start, start + 1, units.end] {
                    let selected = start..end;
                    let mut want: Option<Bounds> = None;
                    for (i, record) in line.fragments.iter().enumerate() {
                        if frame.belongs(record, None, &selected)
                            && let Some(b) = frame.record_bounds(i, &mut sat)
                        {
                            want = Some(want.map_or(b, |old| old.union(b)));
                        }
                    }
                    let got = index.column(&frame, None, &selected);
                    assert_eq!(
                        got.map(|b| (b.top, b.bottom)),
                        want.map(|b| (b.top, b.bottom)),
                        "{mode:?}/{selected:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn completed_bounds_preserve_nested_equal_starts_and_skip_siblings() {
    let mut indexed = index::CompletedBounds::default();
    let mut reference = Vec::<(std::ops::Range<usize>, Bounds)>::new();
    // Reverse structural order, including clipped ancestors sharing start=5.
    for (i, (units, top, bottom)) in [
        (20..30, -2, 3),
        (8..10, -4, 5),
        (5..7, -6, 7),
        (5..10, -1, 1),
        (5..15, -3, 4),
        (0..30, -1, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let base = Bounds {
            top: LayoutUnit::from_raw(top),
            bottom: LayoutUnit::from_raw(bottom),
        };
        let mut want = base;
        for (range, bounds) in &reference {
            if units.start <= range.start && range.end <= units.end {
                want = want.union(*bounds);
            }
        }
        let got = indexed.include_children(&units, base);
        assert_eq!((got.top, got.bottom), (want.top, want.bottom));
        indexed.insert(units.clone(), i, got);
        reference.push((units, want));
    }
}

// Original membership rule retained as an independent compatibility oracle.
impl Frame<'_> {
    fn belongs(
        &self,
        record: &FragmentRecord,
        box_index: Option<u32>,
        units: &Range<usize>,
    ) -> bool {
        if let Some(box_index) = box_index {
            let mut owner = self.owner(record);
            while let Some(index) = owner {
                if index == box_index {
                    return true;
                }
                owner = self.data.boxes[index as usize].parent;
            }
            false
        } else {
            match &record.kind {
                RecordKind::Glyphs { text, .. } => self.data.units[units.clone()]
                    .iter()
                    .any(|u| u.text.start < text.end && text.start < u.text.end),
                RecordKind::Atomic { unit, .. } => units.contains(&(*unit as usize)),
                _ => false,
            }
        }
    }

    fn column_area_reference(
        &self,
        fragment: &RubyFragmentMeasure,
        column: usize,
        sat: &mut Saturation,
    ) -> Bounds {
        let ruby = &self.data.ruby.containers[fragment.container];
        let mut bounds = self.column_content(fragment, column, sat);
        for (i, record) in self.records.iter().enumerate() {
            bounds_tests::visit();
            if self.belongs(
                record,
                ruby.columns[fragment.column_start + column].box_index,
                &fragment.bases[column],
            ) && let Some(other) = self.record_bounds(i, sat)
            {
                bounds = bounds.union(other);
            }
        }
        bounds
    }
}

#[test]
fn indexed_bounds_keep_atomic_overlay_and_saturated_extents() {
    let p = paragraph(2);
    let line = p
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        )
        .remove(0);
    let mut records = line.fragments.to_vec();
    let glyph = records
        .iter()
        .position(|r| matches!(r.kind, RecordKind::Glyphs { .. }))
        .unwrap();
    let (run, item) = match records[glyph].kind {
        RecordKind::Glyphs { run, item, .. } => (run, item),
        _ => unreachable!(),
    };
    let mut overlay = p.data.runs[run as usize].clone();
    // Edge reshaping may select different metrics from the paragraph run.
    let metrics = std::sync::Arc::make_mut(&mut overlay.instance)
        .metrics
        .as_mut()
        .unwrap();
    metrics.ascent = 91.0;
    metrics.descent = 37.0;
    let overlays = [overlay];
    if let RecordKind::Glyphs { source, .. } = &mut records[glyph].kind {
        *source = GlyphSource::Overlay {
            glyphs: (0, 0),
            clusters: (0, 0),
            run: Some(0),
        };
    }
    let unit = p.data.units.iter().position(|u| u.item == item).unwrap();
    records.push(FragmentRecord {
        kind: RecordKind::Atomic {
            node: NodeId(99),
            size: crate::AtomicSize {
                inline_size: 10.0,
                block_size: 200.0,
                baseline: Some(33.0),
                ..Default::default()
            },
            unit: unit as u32,
        },
        inline_start: LayoutUnit::ZERO,
        inline_size: LayoutUnit::ZERO,
        level: 0,
    });
    for baseline in [LayoutUnit::ZERO, LayoutUnit::MIN, LayoutUnit::MAX] {
        let shifts = vec![LayoutUnit::ZERO; records.len()];
        let frame = Frame::new(
            &p.data,
            0..p.data.units.len(),
            &records,
            &overlays,
            baseline,
            &shifts,
            line.block_size,
        );
        let mut index_sat = Saturation::default();
        let indexed = index::RecordBounds::new(&frame, &mut index_sat);
        let owners = [None, frame.owner(&records[glyph])];
        for owner in owners {
            let mut want: Option<Bounds> = None;
            let mut reference_sat = Saturation::default();
            let selected = unit..unit + 1;
            for (i, r) in records.iter().enumerate() {
                if frame.belongs(r, owner, &selected)
                    && let Some(b) = frame.record_bounds(i, &mut reference_sat)
                {
                    want = Some(want.map_or(b, |old| old.union(b)));
                }
            }
            let got = indexed.column(&frame, owner, &selected);
            assert_eq!(
                got.map(|b| (b.top, b.bottom)),
                want.map(|b| (b.top, b.bottom))
            );
            assert_eq!(index_sat.is_clean(), reference_sat.is_clean());
            if baseline == LayoutUnit::ZERO {
                let b = got.unwrap();
                assert_eq!(b.top.to_f32(), -91.0);
                assert_eq!(b.bottom.to_f32(), 167.0);
            }
        }
    }
}

#[test]
fn emphasized_sibling_geometry_uses_bounded_interval_queries() {
    for count in [64, 128] {
        let paragraph = paragraph_with_emphasis(count, "a", WritingMode::VerticalRl, true);
        VISITS.with(|n| n.set(0));
        let lines = paragraph.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1_000_000.0,
            &AtomicSizes::EMPTY,
        );
        let visits = VISITS.with(|n| n.get());
        assert_eq!(lines.len(), 1);
        let records = lines[0].fragments.len();
        assert_eq!(lines[0].emphasis_offsets.len(), records);
        assert!(
            visits <= 32 * (records + count),
            "count={count}, records={records}, visits={visits}"
        );
        for fragment in lines[0].fragments() {
            if let crate::Fragment::GlyphRun(run) = fragment {
                assert!(run.emphasis_mark().unwrap().offset > run.font_size() / 2.0);
            }
        }
    }
}

use super::*;
use crate::analysis::units::UnitKind;
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{FontFamily, InlineStyle, ParagraphStyle};
use crate::{LayoutContext, Paragraph, ParagraphBuilder};

fn style() -> InlineStyle {
    InlineStyle {
        font_size: 20.0,
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    }
}

fn text(node: u64, value: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        },
        value,
        style,
        limits,
    )
}

fn pair(id: u64, base: RubyContent, reading: RubyContent) -> Ruby {
    Ruby::new(
        vec![RubyBase {
            node: NodeId(id),
            content: base,
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(id + 1),
                content: reading,
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap()
}

fn reference_groups(
    data: &ParagraphData,
    range: std::ops::Range<usize>,
) -> Vec<(usize, usize, u8, bool)> {
    let mut nested = std::collections::HashMap::new();
    let mut through = range.end;
    data.ruby.intervals.intersecting(
        &data.ruby.containers,
        range.start,
        &mut through,
        |index, _| {
            let ruby = &data.ruby.containers[index];
            if range.start <= ruby.units.start && ruby.units.end <= range.end {
                nested.insert(ruby.units.start, ruby.units.end);
            }
        },
    );

    let mut result = Vec::new();
    let mut i = range.start;
    while i < range.end {
        let unit = &data.units[i];
        if let Some(end) = nested.get(&i) {
            result.push((i, *end - 1, unit.level, true));
            i = *end;
            continue;
        }
        if matches!(
            unit.kind,
            UnitKind::Cluster { .. } | UnitKind::Atomic { .. }
        ) || unit.combine.is_some()
        {
            let tail = if let Some(combine) = unit.combine {
                data.combine_spans[combine as usize]
                    .units
                    .end
                    .min(range.end)
                    - 1
            } else if let Some(shared) = &unit.shared_cluster {
                shared.units.end.min(range.end) - 1
            } else {
                data.unit_spacing[i].tail.min(range.end - 1)
            };
            result.push((i, tail, unit.level, false));
            i = tail + 1;
        } else {
            i += 1;
        }
    }
    result
}

fn nested_ruby_paragraph() -> Paragraph {
    let limits = Limits::default();
    let style = style();
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
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();

    let inner = pair(
        20,
        text(22, "日月", &style, &limits),
        text(23, "にほん", &style, &limits),
    );
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style.clone(),
            ..Default::default()
        },
        &limits,
    );
    reading.push_ruby(NodeId(24), &style, inner);
    let outer = pair(
        10,
        text(12, "天地", &style, &limits),
        RubyContent::from_builder(reading),
    );
    let mut root = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style.clone(),
            ..Default::default()
        },
        &limits,
    );
    root.push_ruby(NodeId(14), &style, outer);
    root.build(&mut LayoutContext::new(), &fonts).unwrap()
}

fn assert_count_matches_materialized_groups(data: &ParagraphData, path: &str) {
    for start in 0..=data.units.len() {
        for end in start..=data.units.len() {
            let range = start..end;
            let expected_groups = reference_groups(data, range.clone());
            let actual_groups: Vec<_> = groups(data, range.clone())
                .into_iter()
                .map(|group| (group.head, group.tail, group.level, group.ruby))
                .collect();
            assert_eq!(actual_groups, expected_groups, "{path} {range:?}");
            let expected = expected_groups.len();
            GROUP_VECTORS.with(|vectors| vectors.set(0));
            let actual = count(data, range.clone());
            assert_eq!(actual, expected, "{path} {range:?}");
            assert_eq!(
                GROUP_VECTORS.with(std::cell::Cell::get),
                0,
                "count materialized Group storage for {path} {range:?}"
            );
        }
    }
    for (container_index, container) in data.ruby.containers.iter().enumerate() {
        for (lane_index, lane) in container.lanes.iter().enumerate() {
            assert_count_matches_materialized_groups(
                &lane.paragraph.data,
                &format!("{path}/container-{container_index}/lane-{lane_index}"),
            );
        }
    }
}

#[test]
fn count_matches_groups_without_materializing_group_vectors() {
    let paragraph = nested_ruby_paragraph();
    assert_count_matches_materialized_groups(&paragraph.data, "root");
}

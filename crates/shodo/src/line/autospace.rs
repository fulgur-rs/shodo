//! CSS Text4 classes and indexed ownership/edge tests for visual boundaries.
use super::spacing_summary::Edge;
use crate::geometry::{Direction, LayoutUnit, Saturation, WritingMode};
use crate::paragraph::ParagraphData;
use crate::shape::orientation::{RunOrientation, resolve};
use crate::style::{TextAutospace, TextOrientation};
use icu_properties::{
    CodePointMapData,
    props::{EastAsianWidth, GeneralCategory, Script},
    script::ScriptWithExtensions,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Class {
    #[default]
    Other,
    Ideograph,
    Letter,
    Digit,
}

/// Whether a BCP 47 tag is a Chinese language: the `zh` macrolanguage or
/// one of its ISO 639-3 member languages (UTR #59 Conditional resolution).
pub(super) fn chinese(language: Option<&str>) -> bool {
    const MEMBERS: [&str; 17] = [
        "zh", "cdo", "cjy", "cmn", "cnp", "cpx", "csp", "czh", "czo", "gan", "hak", "hsn", "lzh",
        "mnp", "nan", "wuu", "yue",
    ];
    language
        .and_then(|tag| tag.split('-').next())
        .is_some_and(|primary| MEMBERS.iter().any(|m| primary.eq_ignore_ascii_case(m)))
}

/// UTR #59 East_Asian_Spacing=Conditional: Other_Punctuation that is not
/// East Asian Fullwidth, Halfwidth or Wide, minus a few marks that never
/// take inter-script spacing.
fn conditional(ch: char, gc: GeneralCategory) -> bool {
    gc == GeneralCategory::OtherPunctuation
        && !matches!(
            CodePointMapData::<EastAsianWidth>::new().get(ch),
            EastAsianWidth::Fullwidth | EastAsianWidth::Halfwidth | EastAsianWidth::Wide
        )
        && !matches!(
            ch,
            '"' | '\'' | '*' | '/' | '\u{b7}' | '\u{2020}' | '\u{2021}' | '\u{2026}'
        )
}

/// `chinese` is the character's own content language; under it UTR #59
/// Conditional punctuation is spaced like a non-ideographic letter.
pub(super) fn classify(
    ch: char,
    mode: WritingMode,
    orientation: TextOrientation,
    chinese: bool,
) -> Class {
    let class = classify_character(ch, chinese);
    // CSS Text 4 §8.4.1 excludes non-ideographic letters and numerals
    // typeset upright in vertical text. The shaping resolver is the source
    // of the used orientation, including `mixed` and sideways writing modes.
    if matches!(class, Class::Letter | Class::Digit)
        && resolve(mode, orientation, ch) == RunOrientation::Upright
    {
        Class::Other
    } else {
        class
    }
}

fn classify_character(ch: char, chinese: bool) -> Class {
    let gc = CodePointMapData::<GeneralCategory>::new().get(ch);
    if chinese && conditional(ch, gc) {
        return Class::Letter;
    }
    // Punctuation and separators interrupt the boundary even when Han is
    // among Script_Extensions (e.g. CJK punctuation shared by several scripts).
    if matches!(
        gc,
        GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
            | GeneralCategory::SpaceSeparator
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    ) {
        return Class::Other;
    }
    if ('\u{3041}'..='\u{30ff}').contains(&ch)
        || ('\u{31c0}'..='\u{31ff}').contains(&ch)
        || ScriptWithExtensions::new().has_script(ch, Script::Han)
    {
        return Class::Ideograph;
    }
    let width = CodePointMapData::<EastAsianWidth>::new().get(ch);
    if gc == GeneralCategory::DecimalNumber && width != EastAsianWidth::Fullwidth {
        return Class::Digit;
    }
    if !matches!(width, EastAsianWidth::Wide | EastAsianWidth::Fullwidth)
        && matches!(
            gc,
            GeneralCategory::UppercaseLetter
                | GeneralCategory::LowercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::NonspacingMark
                | GeneralCategory::SpacingMark
                | GeneralCategory::EnclosingMark
        )
    {
        Class::Letter
    } else {
        Class::Other
    }
}

#[derive(Default)]
pub(crate) struct Tree {
    depth: Vec<u32>,
    jumps: Vec<Vec<usize>>,
    left: Vec<u32>,
    right: Vec<u32>,
    hang_left: Vec<u32>,
    hang_right: Vec<u32>,
    pub(super) item_nodes: Vec<u32>,
    pub(super) has_content: Vec<bool>,
}

impl Tree {
    /// Clone edges exist on every fragment. Slice edges are represented by
    /// their actual Open/Close markers in the candidate's visual summary.
    pub(super) fn cloned_outer_clear(&self, node: usize, left: bool) -> bool {
        if left {
            self.hang_left[node] == 0
        } else {
            self.hang_right[node] == 0
        }
    }
    pub(super) fn unobstructed(&self, left: usize, right: usize) -> bool {
        let common = self.common(left, right);
        self.right[left] == self.right[common] && self.left[right] == self.left[common]
    }
    pub(crate) fn build(data: &ParagraphData) -> Self {
        let n = data.boxes.len() + 1;
        let mut tree = Self {
            depth: vec![0; n],
            jumps: vec![vec![0; n]],
            left: vec![0; n],
            right: vec![0; n],
            hang_left: vec![0; n],
            hang_right: vec![0; n],
            item_nodes: vec![0; data.items.len()],
            has_content: vec![false; n],
        };
        for (i, b) in data.boxes.iter().enumerate() {
            let node = i + 1;
            let parent = b.parent.map_or(0, |p| p as usize + 1);
            tree.jumps[0][node] = parent;
            tree.depth[node] = tree.depth[parent] + 1;
            let e = b.edges;
            let start = [
                e.margin.inline_start,
                e.border.inline_start,
                e.padding.inline_start,
            ]
            .iter()
            .any(|v| *v != 0.0);
            let end = [
                e.margin.inline_end,
                e.border.inline_end,
                e.padding.inline_end,
            ]
            .iter()
            .any(|v| *v != 0.0);
            let rtl = data.styles[b.style as usize].direction == Direction::Rtl;
            tree.left[node] = tree.left[parent] + u32::from(if rtl { end } else { start });
            tree.right[node] = tree.right[parent] + u32::from(if rtl { start } else { end });
            let cloned = data.styles[b.style as usize].box_decoration_break
                == crate::style::BoxDecorationBreak::Clone;
            let hang_start =
                cloned && (e.border.inline_start != 0.0 || e.padding.inline_start != 0.0);
            let hang_end = cloned && (e.border.inline_end != 0.0 || e.padding.inline_end != 0.0);
            tree.hang_left[node] =
                tree.hang_left[parent] + u32::from(if rtl { hang_end } else { hang_start });
            tree.hang_right[node] =
                tree.hang_right[parent] + u32::from(if rtl { hang_start } else { hang_end });
        }
        let mut current = 0u32;
        let mut next_box = 1u32;
        for (index, item) in data.items.iter().enumerate() {
            use crate::analysis::ItemKind;
            match item.kind {
                ItemKind::OpenInline { .. } => {
                    current = next_box;
                    next_box += 1;
                }
                ItemKind::CloseInline => {
                    current = tree.jumps[0][current as usize] as u32;
                }
                _ => {}
            }
            tree.item_nodes[index] = current;
            let content = match item.kind {
                ItemKind::Text => data.text[item.text.start as usize..item.text.end as usize]
                    .chars()
                    .any(|ch| {
                        !matches!(
                            CodePointMapData::<GeneralCategory>::new().get(ch),
                            GeneralCategory::Format | GeneralCategory::Control
                        )
                    }),
                ItemKind::Atomic { .. } | ItemKind::Tab => true,
                _ => false,
            };
            tree.has_content[current as usize] |= content;
        }
        for node in (1..n).rev() {
            tree.has_content[tree.jumps[0][node]] |= tree.has_content[node];
        }
        let max = tree.depth.iter().copied().max().unwrap_or(0);
        while (1u64 << tree.jumps.len()) <= u64::from(max) {
            let previous = tree.jumps.last().unwrap();
            tree.jumps
                .push(previous.iter().map(|p| previous[*p]).collect());
        }
        tree
    }

    pub(super) fn common(&self, mut a: usize, mut b: usize) -> usize {
        if self.depth[a] < self.depth[b] {
            std::mem::swap(&mut a, &mut b);
        }
        let difference = self.depth[a] - self.depth[b];
        for (level, jumps) in self.jumps.iter().enumerate() {
            if difference & (1 << level) != 0 {
                a = jumps[a];
            }
        }
        if a == b {
            return a;
        }
        for jumps in self.jumps.iter().rev() {
            if jumps[a] != jumps[b] {
                a = jumps[a];
                b = jumps[b];
            }
        }
        self.jumps[0][a]
    }
}

/// Arguments are in physical left→right visual order, independent of block
/// direction. Prefix edge counts prevent a repeated ancestor walk per gap.
pub(super) fn gap(data: &ParagraphData, a: Edge, b: Edge, blocked: bool) -> i64 {
    if blocked
        || !((a.class == Class::Ideograph && matches!(b.class, Class::Letter | Class::Digit))
            || (b.class == Class::Ideograph && matches!(a.class, Class::Letter | Class::Digit)))
    {
        return 0;
    }
    let left = a.box_node as usize;
    let right = b.box_node as usize;
    let tree = &data.spacing_tree;
    let common = tree.common(left, right);
    if tree.right[left] != tree.right[common] || tree.left[right] != tree.left[common] {
        return 0;
    }
    let style = if common == 0 {
        0
    } else {
        data.boxes[common - 1].style as usize
    };
    if data.styles[style].text_autospace == TextAutospace::NoAutospace {
        return 0;
    }
    i64::from(
        LayoutUnit::from_f32_round(
            data.style_metrics[style].ic * 0.125,
            &mut Saturation::default(),
        )
        .raw(),
    )
}

/// Space reserved after a visual unit, owned by its common ancestor with
/// the following unit. Descendant borders must exclude this reservation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gap {
    pub(crate) assigned: u32,
    pub(crate) owner: usize,
    pub(crate) amount: LayoutUnit,
}

pub(super) fn owner(data: &ParagraphData, a: Edge, b: Edge) -> usize {
    data.spacing_tree
        .common(a.box_node as usize, b.box_node as usize)
}

pub(crate) fn exclude_from_boxes(
    data: &ParagraphData,
    records: &mut [super::fragments::FragmentRecord],
    gaps: &[Gap],
    sat: &mut Saturation,
) {
    use super::fragments::RecordKind;
    use std::collections::HashMap;
    let mut deltas = HashMap::<usize, LayoutUnit>::new();
    for gap in gaps {
        let assigned = data.units[gap.assigned as usize]
            .parent_box
            .map_or(0, |b| b as usize + 1);
        if assigned == gap.owner {
            continue;
        }
        let value = deltas.entry(assigned).or_default();
        *value = value.add(gap.amount, sat);
        let value = deltas.entry(gap.owner).or_default();
        *value = value.sub(gap.amount, sat);
    }
    let mut boxes: Vec<_> = records
        .iter()
        .filter_map(|r| {
            if let RecordKind::InlineBox { box_index, .. } = r.kind {
                Some(box_index as usize + 1)
            } else {
                None
            }
        })
        .collect();
    boxes.sort_unstable();
    boxes.dedup();
    for &node in boxes.iter().rev() {
        if let Some(delta) = deltas.get(&node).copied() {
            let parent = data.boxes[node - 1].parent.map_or(0, |p| p as usize + 1);
            let value = deltas.entry(parent).or_default();
            *value = value.add(delta, sat);
        }
    }
    for record in records {
        if let RecordKind::InlineBox { box_index, .. } = record.kind
            && let Some(delta) = deltas.get(&(box_index as usize + 1))
        {
            record.inline_size = record.inline_size.sub(*delta, sat);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::spacing_summary::{Cursor, Summary};
    use super::*;
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle};
    use unicode_bidi::{BidiInfo, Level};

    fn oracle(data: &ParagraphData, a: Edge, b: Edge) -> i64 {
        if a.class == b.class {
            return 0;
        }
        let path = |mut node: usize| {
            let mut result = vec![node];
            while node != 0 {
                node = data.boxes[node - 1].parent.map_or(0, |p| p as usize + 1);
                result.push(node);
            }
            result
        };
        let left = path(a.box_node as usize);
        let right = path(b.box_node as usize);
        let common = *left.iter().find(|p| right.contains(p)).unwrap();
        for (nodes, physical_right) in [(&left, true), (&right, false)] {
            for &node in nodes.iter().take_while(|n| **n != common) {
                let b = &data.boxes[node - 1];
                let rtl = data.styles[b.style as usize].direction == Direction::Rtl;
                let e = b.edges;
                let side = if physical_right != rtl {
                    [
                        e.margin.inline_end,
                        e.border.inline_end,
                        e.padding.inline_end,
                    ]
                } else {
                    [
                        e.margin.inline_start,
                        e.border.inline_start,
                        e.padding.inline_start,
                    ]
                };
                if side.iter().any(|v| *v != 0.0) {
                    return 0;
                }
            }
        }
        let style = if common == 0 {
            0
        } else {
            data.boxes[common - 1].style as usize
        };
        if data.styles[style].text_autospace == TextAutospace::NoAutospace {
            0
        } else {
            LayoutUnit::from_f32_round(
                data.styles[style].font_size / 8.0,
                &mut Saturation::default(),
            )
            .raw() as i64
        }
    }

    #[test]
    fn autospace_candidates_match_reorder_and_independent_ancestor_walk() {
        let limits = crate::limits::Limits::default();
        let root = InlineStyle {
            font_size: 20.0,
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &limits,
        );
        let group = InlineStyle {
            font_size: 32.0,
            ..root.clone()
        };
        b.open_inline(NodeId(10), &group, InlineEdges::default());
        for i in 0..6 {
            if i == 3 {
                b.close_inline();
            }
            let child = InlineStyle {
                direction: if i % 2 == 0 {
                    Direction::Ltr
                } else {
                    Direction::Rtl
                },
                ..root.clone()
            };
            let mut edges = InlineEdges::default();
            if i == 1 {
                edges.padding.inline_start = 2.0;
            }
            if i == 4 {
                edges.padding.inline_end = 2.0;
            }
            b.open_inline(NodeId(i * 2 + 20), &child, edges)
                .push_text(
                    TextSource::Generated {
                        node: NodeId(i * 2 + 21),
                    },
                    if i % 2 == 0 { "水" } else { "a" },
                )
                .close_inline();
        }
        let p = b
            .build(
                &mut crate::LayoutContext::new(),
                &crate::font::FontCollection::new(&limits),
            )
            .unwrap();
        let edges: Vec<_> = p
            .data
            .unit_spacing
            .iter()
            .filter_map(|u| u.summary.first)
            .collect();
        assert_eq!(edges.len(), 6);
        for profile in 0..4096usize {
            let levels: Vec<_> = (0..6)
                .map(|i| Level::new(((profile >> (i * 2)) & 3) as u8).unwrap())
                .collect();
            let mut cursor = Cursor::default();
            for end in 1..=6 {
                cursor.push(
                    levels[end - 1].number(),
                    Summary::leaf(edges[end - 1], Some(&p.data)),
                    Some(&p.data),
                );
                let order = BidiInfo::reorder_visual(&levels[..end]);
                let expected: i64 = order
                    .windows(2)
                    .map(|pair| oracle(&p.data, edges[pair[0]], edges[pair[1]]))
                    .sum();
                assert_eq!(
                    cursor.summary(Some(&p.data)).cost,
                    expected,
                    "profile {profile}, prefix {end}"
                );
            }
        }
    }

    #[test]
    fn deep_inline_index_keeps_logarithmic_ancestor_queries() {
        let limits = crate::limits::Limits {
            max_nesting_depth: None,
            ..Default::default()
        };
        let style = ParagraphStyle::default();
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        for i in 0..4096 {
            b.open_inline(NodeId(i), &style.root, InlineEdges::default());
        }
        b.push_text(TextSource::Generated { node: NodeId(4096) }, "水a");
        for _ in 0..4096 {
            b.close_inline();
        }
        b.push_text(TextSource::Generated { node: NodeId(4097) }, "a");
        let p = b
            .build(
                &mut crate::LayoutContext::new(),
                &crate::font::FontCollection::new(&limits),
            )
            .unwrap();
        let tree = &p.data.spacing_tree;
        assert_eq!(tree.jumps.len(), 13);
        assert_eq!(tree.jumps.iter().map(Vec::len).sum::<usize>(), 4097 * 13);
        for a in 0..=4096 {
            for b in [0, 1, 2048, 4096] {
                assert_eq!(tree.common(a, b), a.min(b));
            }
        }
        let lines = p.break_all(
            &mut crate::LayoutContext::new(),
            &Default::default(),
            1000.0,
            &crate::AtomicSizes::EMPTY,
        );
        assert_eq!(lines.len(), 1);
    }
}

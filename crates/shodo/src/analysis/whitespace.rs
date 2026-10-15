//! White-space processing (CSS Text 3 §4.1.1 phase I) and bidi control
//! insertion (CSS Writing Modes 4 §2.4), producing the processed text,
//! the item list and the offset mapping.
//!
//! Segment breaks use whole-IFC neighbor context. The input is DOM text:
//! LF is a segment break and CR is a CSS space, not an HTML source newline.

use super::whitespace_context::{REMOVE, ignorable};
use super::{Item, ItemKind};
use crate::builder::RawItem;
use crate::geometry::Direction;
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::mapping::{MappingCost, MappingKind, MappingUnit, OffsetMapping, TRANSFORM_SPAN_BYTES};
use crate::node::{NodeId, TextSource};
use crate::style::{InlineStyle, UnicodeBidi, WhiteSpaceCollapse};

const OBJECT_REPLACEMENT: char = '\u{FFFC}';
const PARAGRAPH_SEPARATOR: char = '\u{2029}';

#[cfg(test)]
std::thread_local! {
    static BIDI_SCOPE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone)]
pub(crate) struct Processed {
    pub(crate) mapping_cost: MappingCost,
    pub(crate) text: String,
    pub(crate) items: Vec<Item>,
    pub(crate) mapping: Option<OffsetMapping>,
    pub(crate) indivisible: Vec<std::ops::Range<u32>>,
    /// Correspondence with the common pre-transform text, independent of DOM mapping.
    pub(crate) source_spans: Vec<crate::mapping::TransformSpan>,
    pub(crate) width_origins: Vec<super::transform::WidthOrigin>,
}

#[derive(Clone, Copy)]
pub(crate) struct ProcessInput<'a> {
    pub(crate) raw_text: &'a str,
    pub(crate) raw: &'a [RawItem],
    pub(crate) styles: &'a [InlineStyle],
    pub(crate) with_mapping: bool,
    pub(crate) limits: &'a Limits,
    pub(crate) annotation: bool,
}

#[cfg(test)]
pub(crate) fn process(
    raw_text: &str,
    raw: &[RawItem],
    styles: &[InlineStyle],
    with_mapping: bool,
    limits: &Limits,
) -> Result<Processed, LimitExceeded> {
    let mut ledger = crate::limits::ledger::LimitsLedger::new(limits);
    process_input(
        ProcessInput {
            raw_text,
            raw,
            styles,
            with_mapping,
            limits,
            annotation: false,
        },
        &mut ledger,
    )
}

pub(crate) fn process_input(
    input: ProcessInput<'_>,
    ledger: &mut crate::limits::ledger::LimitsLedger,
) -> Result<Processed, LimitExceeded> {
    let flags = prepare_whitespace_flags(&input);
    process_input_with_flags(input, &flags, ledger)
}

pub(crate) fn prepare_whitespace_flags(input: &ProcessInput<'_>) -> Vec<u8> {
    super::whitespace_context::flags_in_context(
        input.raw_text,
        input.raw,
        input.styles,
        input.annotation,
    )
}

pub(crate) fn process_input_with_flags(
    input: ProcessInput<'_>,
    flags: &[u8],
    ledger: &mut crate::limits::ledger::LimitsLedger,
) -> Result<Processed, LimitExceeded> {
    let ProcessInput {
        raw_text,
        raw,
        styles,
        with_mapping,
        limits,
        annotation,
    } = input;
    debug_assert_eq!(flags.len(), raw_text.len());
    let mut p = Processor {
        out: String::with_capacity(
            raw_text.len().min(
                limits
                    .max_text_bytes
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(usize::MAX),
            ),
        ),
        // Scope admission precedes each push, including exhausted alternate
        // and ancestor budgets. Raw count alone cannot authorize capacity.
        items: Vec::new(),
        mapping: with_mapping.then(OffsetMapping::default),
        mapping_cost: MappingCost::default(),
        // Collapsible spaces at the start of the paragraph are removed.
        after_space: true,
        open: Vec::new(),
        bidi_scopes: Vec::new(),
        styles,
        annotation,
        ledger,
    };
    for item in raw {
        p.ledger.before_raw(item);
        match item {
            RawItem::RubyBoundary {
                ruby,
                boundary,
                node,
                style,
            } => {
                p.check_item()?;
                let at = p.pos();
                p.push_item(Item {
                    kind: ItemKind::RubyBoundary {
                        ruby: *ruby,
                        boundary: *boundary,
                    },
                    text: at..at,
                    style: *style,
                    node: *node,
                    own_break_style: false,
                })?;
            }
            RawItem::Text {
                source,
                range,
                style,
            } => {
                let text = &raw_text[range.start as usize..range.end as usize];
                p.text(
                    text,
                    *source,
                    *style,
                    &flags[range.start as usize..range.end as usize],
                )?;
            }
            RawItem::Open { node, style, edges } => {
                p.marker(ItemKind::OpenInline { edges: *edges }, *style, *node)?;
                for &c in bidi_open(&styles[*style as usize]) {
                    p.generated(ItemKind::BidiControl, c, *style, *node)?;
                }
                if !bidi_open(&styles[*style as usize]).is_empty() {
                    p.bidi_scopes.push(p.open.len());
                }
                p.open.push((*node, *style));
            }
            RawItem::Close => {
                if let Some((node, style)) = p.open.pop() {
                    if p.bidi_scopes.last() == Some(&p.open.len()) {
                        p.bidi_scopes.pop();
                    }
                    for &c in bidi_close(&styles[style as usize]) {
                        p.generated(ItemKind::BidiControl, c, style, node)?;
                    }
                    p.marker(ItemKind::CloseInline, style, node)?;
                }
            }
            RawItem::Atomic {
                node,
                style,
                parent_style,
                edges,
            } => {
                let kind = ItemKind::Atomic {
                    edges: *edges,
                    parent_style: *parent_style,
                };
                p.generated(kind, OBJECT_REPLACEMENT, *style, *node)?;
                p.after_space = false;
            }
            RawItem::OutOfFlow { node, kind, style } => {
                // Out-of-flow boxes are transparent to white-space collapsing.
                p.generated(
                    ItemKind::OutOfFlow { kind: *kind },
                    OBJECT_REPLACEMENT,
                    *style,
                    *node,
                )?;
            }
            RawItem::BlockInInline { node, style } => {
                p.close_bidi_scopes()?;
                p.generated(ItemKind::BlockInInline, PARAGRAPH_SEPARATOR, *style, *node)?;
                p.open_bidi_scopes()?;
                p.after_space = true;
            }
            RawItem::ForcedBreak {
                node,
                style,
                own_style,
            } => {
                p.close_bidi_scopes()?;
                p.generated(ItemKind::ForcedBreak, '\n', *style, *node)?;
                p.items.last_mut().expect("generated break").own_break_style = *own_style;
                p.open_bidi_scopes()?;
                p.after_space = true;
            }
        }
        p.ledger.after_raw(item);
    }
    let source_spans = if p.out.is_empty() {
        Vec::new()
    } else {
        p.mapping_cost.charge_with(TRANSFORM_SPAN_BYTES, |bytes| {
            p.ledger.charge(LimitKind::MappingBytes, bytes)
        })?;
        vec![crate::mapping::TransformSpan {
            old: 0..p.out.len() as u32,
            new: 0..p.out.len() as u32,
            kind: MappingKind::Identity,
        }]
    };
    Ok(Processed {
        mapping_cost: p.mapping_cost,
        width_origins: Vec::new(),
        source_spans,
        text: p.out,
        items: p.items,
        mapping: p.mapping,
        indivisible: Vec::new(),
    })
}

struct Processor<'a> {
    mapping_cost: MappingCost,
    ledger: &'a mut crate::limits::ledger::LimitsLedger,
    annotation: bool,
    out: String,
    items: Vec<Item>,
    mapping: Option<OffsetMapping>,
    after_space: bool,
    open: Vec<(NodeId, u32)>,
    /// Indices into `open` for scopes that emit controls. Normal inlines must
    /// not be visited again at every paragraph boundary.
    bidi_scopes: Vec<usize>,
    styles: &'a [InlineStyle],
}

impl Processor<'_> {
    fn close_bidi_scopes(&mut self) -> Result<(), LimitExceeded> {
        for i in (0..self.bidi_scopes.len()).rev() {
            #[cfg(test)]
            BIDI_SCOPE_VISITS.with(|visits| visits.set(visits.get() + 1));
            let (node, style) = self.open[self.bidi_scopes[i]];
            for &c in bidi_close(&self.styles[style as usize]) {
                self.generated(ItemKind::BidiControl, c, style, node)?;
            }
        }
        Ok(())
    }

    fn open_bidi_scopes(&mut self) -> Result<(), LimitExceeded> {
        for i in 0..self.bidi_scopes.len() {
            #[cfg(test)]
            BIDI_SCOPE_VISITS.with(|visits| visits.set(visits.get() + 1));
            let (node, style) = self.open[self.bidi_scopes[i]];
            for &c in bidi_open(&self.styles[style as usize]) {
                self.generated(ItemKind::BidiControl, c, style, node)?;
            }
        }
        Ok(())
    }

    fn check_item(&mut self) -> Result<(), LimitExceeded> {
        Limits::check(
            Some(u64::from(u32::MAX)),
            LimitKind::Items,
            self.items.len() as u64 + 1,
        )?;
        self.ledger.check_current_item()?;
        Ok(())
    }
    fn push_item(&mut self, item: Item) -> Result<(), LimitExceeded> {
        self.ledger.record_item()?;
        self.items.push(item);
        Ok(())
    }
    fn append(&mut self, c: char) -> Result<(), LimitExceeded> {
        let len = (self.out.len() as u64).saturating_add(c.len_utf8() as u64);
        Limits::check(Some(u64::from(u32::MAX)), LimitKind::TextBytes, len)?;
        self.ledger.transient_text(c.len_utf8() as u64)?;
        self.out.push(c);
        Ok(())
    }
    fn pos(&self) -> u32 {
        self.out.len() as u32
    }

    fn marker(&mut self, kind: ItemKind, style: u32, node: NodeId) -> Result<(), LimitExceeded> {
        self.check_item()?;
        let at = self.pos();
        self.push_item(Item {
            kind,
            text: at..at,
            style,
            node: Some(node),
            own_break_style: false,
        })?;
        Ok(())
    }

    fn generated(
        &mut self,
        kind: ItemKind,
        c: char,
        style: u32,
        node: NodeId,
    ) -> Result<(), LimitExceeded> {
        self.check_item()?;
        let start = self.pos();
        self.append(c)?;
        let text = start..self.pos();
        if let Some(m) = &mut self.mapping {
            let budget = &mut self.mapping_cost;
            let ledger = &mut self.ledger;
            m.try_push_generated(text.clone(), node, |bytes| {
                budget.charge_with(bytes, |bytes| ledger.mapping_record(bytes))
            })?;
        }
        self.push_item(Item {
            kind,
            text,
            style,
            node: Some(node),
            own_break_style: false,
        })?;
        Ok(())
    }

    fn map(
        &mut self,
        kind: MappingKind,
        node: NodeId,
        dom: Option<u32>,
        len: u32,
        text: std::ops::Range<u32>,
    ) -> Result<(), LimitExceeded> {
        let Some(m) = &mut self.mapping else {
            return Ok(());
        };
        let budget = &mut self.mapping_cost;
        let ledger = &mut self.ledger;
        let mut charge = |bytes| budget.charge_with(bytes, |bytes| ledger.mapping_record(bytes));
        match dom {
            // `dom` is a caller-supplied offset, not covered by any
            // resource limit; saturate instead of overflowing so no input
            // can panic.
            Some(dom) => m.try_push_unit(
                MappingUnit {
                    kind,
                    node,
                    dom: dom..dom.saturating_add(len),
                    text,
                },
                &mut charge,
            )?,
            None if !text.is_empty() => m.try_push_generated(text, node, &mut charge)?,
            None => {}
        }
        Ok(())
    }

    fn text(
        &mut self,
        s: &str,
        source: TextSource,
        style_index: u32,
        flags: &[u8],
    ) -> Result<(), LimitExceeded> {
        use WhiteSpaceCollapse::*;
        let style = &self.styles[style_index as usize];
        let collapse_spaces = matches!(style.white_space_collapse, Collapse | PreserveBreaks);
        let preserve_breaks = !self.annotation && !matches!(style.white_space_collapse, Collapse);
        // `preserve-spaces` keeps every space uncollapsed but, unlike
        // `preserve`, tabs and segment breaks lose their special meaning and
        // become an ordinary space character (CSS Text 4, `white-space-collapse`).
        let convert_to_space = matches!(style.white_space_collapse, PreserveSpaces);
        let node = source.node();
        let dom_base = match source {
            TextSource::Dom { offset, .. } => Some(offset),
            TextSource::Generated { .. } => None,
        };
        let mut segment: Option<u32> = None;
        for (i, raw_c) in s.char_indices() {
            let dom = dom_base.map(|b| b.saturating_add(i as u32));
            let len = raw_c.len_utf8() as u32;
            if flags[i] & REMOVE != 0 {
                let at = self.pos();
                self.map(MappingKind::Collapsed, node, dom, len, at..at)?;
                continue;
            }
            let c = if raw_c == '\r' || convert_to_space && matches!(raw_c, '\t' | '\n') {
                ' '
            } else {
                raw_c
            };
            let annotation_break =
                self.annotation && matches!(raw_c, '\n' | '\u{2028}' | '\u{2029}' | '\u{0085}');
            let control = match c {
                '\u{2028}' | '\u{2029}' | '\u{0085}' if !self.annotation => {
                    Some(ItemKind::ForcedBreak)
                }
                '\n' if preserve_breaks => Some(ItemKind::ForcedBreak),
                '\t' if !collapse_spaces => Some(ItemKind::Tab),
                _ => None,
            };
            if let Some(kind) = control {
                self.flush(&mut segment, style_index, node)?;
                if matches!(kind, ItemKind::ForcedBreak) {
                    self.close_bidi_scopes()?;
                }
                self.check_item()?;
                let start = self.pos();
                self.append(c)?;
                self.map(MappingKind::Identity, node, dom, len, start..self.pos())?;
                self.push_item(Item {
                    kind: kind.clone(),
                    text: start..self.pos(),
                    style: style_index,
                    node: Some(node),
                    own_break_style: false,
                })?;
                self.after_space = matches!(kind, ItemKind::ForcedBreak);
                if matches!(kind, ItemKind::ForcedBreak) {
                    self.open_bidi_scopes()?;
                }
                continue;
            }
            let collapsible = annotation_break || collapse_spaces && matches!(c, ' ' | '\t' | '\n');
            if collapsible && self.after_space {
                let at = self.pos();
                self.map(MappingKind::Collapsed, node, dom, len, at..at)?;
                continue;
            }
            if segment.is_none() {
                self.check_item()?;
            }
            segment.get_or_insert(self.pos());
            let start = self.pos();
            // A collapsible tab or segment break is kept as one space (same length).
            self.append(if collapsible { ' ' } else { c })?;
            self.map(
                if self.pos() - start == len {
                    MappingKind::Identity
                } else {
                    MappingKind::Expanded
                },
                node,
                dom,
                len,
                start..self.pos(),
            )?;
            if !ignorable(c) {
                self.after_space = collapsible;
            }
        }
        self.flush(&mut segment, style_index, node)
    }

    fn flush(
        &mut self,
        segment: &mut Option<u32>,
        style: u32,
        node: NodeId,
    ) -> Result<(), LimitExceeded> {
        if let Some(start) = segment.take() {
            let end = self.pos();
            if end > start {
                self.check_item()?;
                self.push_item(Item {
                    kind: ItemKind::Text,
                    text: start..end,
                    style,
                    node: Some(node),
                    own_break_style: false,
                })?;
            }
        }
        Ok(())
    }
}

fn bidi_open(style: &InlineStyle) -> &'static [char] {
    let rtl = style.direction == Direction::Rtl;
    match (style.unicode_bidi, rtl) {
        (UnicodeBidi::Normal, _) => &[],
        (UnicodeBidi::Embed, false) => &['\u{202A}'],
        (UnicodeBidi::Embed, true) => &['\u{202B}'],
        (UnicodeBidi::Isolate, false) => &['\u{2066}'],
        (UnicodeBidi::Isolate, true) => &['\u{2067}'],
        (UnicodeBidi::BidiOverride, false) => &['\u{202D}'],
        (UnicodeBidi::BidiOverride, true) => &['\u{202E}'],
        (UnicodeBidi::IsolateOverride, false) => &['\u{2066}', '\u{202D}'],
        (UnicodeBidi::IsolateOverride, true) => &['\u{2067}', '\u{202E}'],
        (UnicodeBidi::Plaintext, _) => &['\u{2068}'],
    }
}

fn bidi_close(style: &InlineStyle) -> &'static [char] {
    match style.unicode_bidi {
        UnicodeBidi::Normal => &[],
        UnicodeBidi::Embed | UnicodeBidi::BidiOverride => &['\u{202C}'],
        UnicodeBidi::Isolate | UnicodeBidi::Plaintext => &['\u{2069}'],
        UnicodeBidi::IsolateOverride => &['\u{202C}', '\u{2069}'],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::ParagraphBuilder;
    use crate::limits::Limits;
    use crate::mapping::{Affinity, TextOrigin};
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle, UnicodeBidi, WhiteSpaceCollapse};

    fn run(b: &ParagraphBuilder) -> Processed {
        process(&b.text, &b.items, &b.styles, true, &b.limits).unwrap()
    }

    fn dom(node: u64) -> TextSource {
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        }
    }

    fn builder() -> ParagraphBuilder {
        ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default())
    }

    #[test]
    fn first_line_analysis_generates_whitespace_flags_once() {
        let style = ParagraphStyle {
            first_line: Some(InlineStyle {
                font_size: 18.0,
                ..InlineStyle::default()
            }),
            ..ParagraphStyle::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &Limits::default());
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "a  \nb");

        super::super::whitespace_context::reset_flag_generations();
        let _analysis = builder.analyze().unwrap();

        assert_eq!(super::super::whitespace_context::flag_generations(), 1);
    }

    #[test]
    fn analysis_without_first_line_generates_whitespace_flags_once() {
        let mut builder = builder();
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "a  \nb");

        super::super::whitespace_context::reset_flag_generations();
        let _analysis = builder.analyze().unwrap();

        assert_eq!(super::super::whitespace_context::flag_generations(), 1);
    }

    #[test]
    fn first_line_ruby_annotation_reuses_flags_with_annotation_context() {
        use crate::font::FontFaceDescriptor;
        use crate::ruby::{
            Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
            RubyVisibility,
        };

        let limits = Limits::default();
        let fonts = crate::test_support::fonts::collection(&limits);
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        fonts
            .register_face(
                crate::test_support::fonts::CJK.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "CJK".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut root = InlineStyle {
            font_families: vec![
                crate::style::FontFamily::Named("Latin".into()),
                crate::style::FontFamily::Named("CJK".into()),
            ],
            ..Default::default()
        };
        let first = InlineStyle {
            font_size: 18.0,
            ..root.clone()
        };
        let style = ParagraphStyle {
            root: root.clone(),
            first_line: Some(first),
            ..Default::default()
        };
        let base = RubyContent::text(
            TextSource::Generated { node: NodeId(10) },
            "水",
            &root,
            &limits,
        );
        root.font_size = 8.0;
        let annotation_style = ParagraphStyle {
            root: root.clone(),
            first_line: Some(InlineStyle {
                font_size: 9.0,
                ..root.clone()
            }),
            ..Default::default()
        };
        let mut annotation_builder = ParagraphBuilder::new(&annotation_style, &limits);
        annotation_builder.push_text(TextSource::Generated { node: NodeId(11) }, "み\nず");
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: base,
                align: Default::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(11),
                    content: RubyContent::from_builder(annotation_builder),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap();
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_ruby(NodeId(12), &style.root, ruby);

        super::super::whitespace_context::reset_flag_generations();
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();

        assert_eq!(super::super::whitespace_context::flag_generations(), 2);
        assert!(paragraph.warnings().is_empty());
    }

    fn assert_processed_eq(actual: &Processed, expected: &Processed) {
        assert_eq!(actual.text, expected.text);
        assert_eq!(
            format!("{:?}", actual.items),
            format!("{:?}", expected.items)
        );
        assert_eq!(
            format!("{:?}", actual.mapping),
            format!("{:?}", expected.mapping)
        );
        assert_eq!(actual.indivisible, expected.indivisible);
        assert_eq!(actual.source_spans.len(), expected.source_spans.len());
        for (actual, expected) in actual.source_spans.iter().zip(&expected.source_spans) {
            assert_eq!(actual.old, expected.old);
            assert_eq!(actual.new, expected.new);
            assert_eq!(actual.kind, expected.kind);
        }
        assert_eq!(actual.width_origins.len(), expected.width_origins.len());
        for (actual, expected) in actual.width_origins.iter().zip(&expected.width_origins) {
            assert_eq!(actual.text, expected.text);
            assert_eq!(actual.before_width, expected.before_width);
        }
    }

    #[test]
    fn supplied_flags_match_regenerated_processing_for_whitespace_modes() {
        for mode in [
            WhiteSpaceCollapse::Collapse,
            WhiteSpaceCollapse::Preserve,
            WhiteSpaceCollapse::PreserveBreaks,
            WhiteSpaceCollapse::PreserveSpaces,
            WhiteSpaceCollapse::BreakSpaces,
        ] {
            for annotation in [false, true] {
                for with_mapping in [false, true] {
                    let root = InlineStyle {
                        white_space_collapse: mode,
                        ..InlineStyle::default()
                    };
                    let child = InlineStyle {
                        white_space_collapse: mode,
                        ..root.clone()
                    };
                    let paragraph_style = ParagraphStyle {
                        root,
                        ..Default::default()
                    };
                    let mut builder = ParagraphBuilder::new(&paragraph_style, &Limits::default());
                    builder
                        .push_text(dom(1), "日 ")
                        .open_inline(NodeId(2), &child, InlineEdges::default())
                        .push_text(dom(3), "\t\n \u{2028}b")
                        .close_inline();
                    let input = ProcessInput {
                        raw_text: &builder.text,
                        raw: &builder.items,
                        styles: &builder.styles,
                        with_mapping,
                        limits: &builder.limits,
                        annotation,
                    };
                    let flags = prepare_whitespace_flags(&input);
                    let expected = process_input(
                        input,
                        &mut crate::limits::ledger::LimitsLedger::new(input.limits),
                    )
                    .unwrap();
                    let mut bases = crate::limits::ledger::LimitsLedger::new(input.limits);
                    let actual = process_input_with_flags(input, &flags, &mut bases).unwrap();

                    assert_processed_eq(&actual, &expected);
                }
            }
        }
    }

    #[test]
    fn supplied_flags_keep_limit_rejection_kind_limit_and_actual() {
        let mut builder = builder();
        builder.push_text(dom(1), "a b");
        let flags_input = ProcessInput {
            raw_text: &builder.text,
            raw: &builder.items,
            styles: &builder.styles,
            with_mapping: true,
            limits: &builder.limits,
            annotation: false,
        };
        let flags = prepare_whitespace_flags(&flags_input);

        for limits in [
            Limits {
                max_text_bytes: Some(1),
                ..Limits::default()
            },
            Limits {
                max_items: Some(0),
                ..Limits::default()
            },
        ] {
            let input = ProcessInput {
                limits: &limits,
                ..flags_input
            };
            let expected = process_input(
                input,
                &mut crate::limits::ledger::LimitsLedger::new(input.limits),
            )
            .err()
            .expect("generated flags should hit the same resource limit");
            let mut bases = crate::limits::ledger::LimitsLedger::new(input.limits);
            let actual = process_input_with_flags(input, &flags, &mut bases)
                .err()
                .expect("supplied flags should hit the same resource limit");

            assert_eq!(actual.kind, expected.kind);
            assert_eq!(actual.limit, expected.limit);
            assert_eq!(actual.actual, expected.actual);
        }
    }

    #[test]
    fn collapses_across_element_boundaries_and_trims_the_start() {
        let mut b = builder();
        b.push_text(dom(1), "  a  ")
            .open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), " b")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a b");
        let m = p.mapping.unwrap();
        // The space in node 3 was collapsed into the one kept in node 1.
        assert_eq!(m.dom_to_text(NodeId(3), 0), Some((2, Affinity::Downstream)));
        assert_eq!(m.dom_to_text(NodeId(3), 1), Some((2, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(2, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(3),
                offset: 1
            })
        );
        // Round trip for every kept DOM offset.
        for (node, offset) in [(1, 2), (1, 3), (3, 1)] {
            let (t, a) = m.dom_to_text(NodeId(node), offset).unwrap();
            assert_eq!(
                m.text_to_dom(t, a),
                Some(TextOrigin::Dom {
                    node: NodeId(node),
                    offset
                })
            );
        }
    }

    #[test]
    fn preserved_tabs_and_newlines_become_control_items() {
        let pre = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &pre, InlineEdges::default())
            .push_text(dom(2), "a\tb\nc")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a\tb\nc");
        let kinds: Vec<_> = p
            .items
            .iter()
            .map(|i| std::mem::discriminant(&i.kind))
            .collect();
        use ItemKind::*;
        let expected: Vec<_> = [
            OpenInline {
                edges: InlineEdges::default(),
            },
            Text,
            Tab,
            Text,
            ForcedBreak,
            Text,
            CloseInline,
        ]
        .iter()
        .map(std::mem::discriminant)
        .collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn preserve_spaces_converts_tabs_and_breaks_to_space_without_collapsing() {
        let style = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::PreserveSpaces,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &style, InlineEdges::default())
            .push_text(dom(2), "a \t\nb")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a   b");
        let kinds: Vec<_> = p
            .items
            .iter()
            .map(|i| std::mem::discriminant(&i.kind))
            .collect();
        use ItemKind::*;
        let expected: Vec<_> = [
            OpenInline {
                edges: InlineEdges::default(),
            },
            Text,
            CloseInline,
        ]
        .iter()
        .map(std::mem::discriminant)
        .collect();
        assert_eq!(
            kinds, expected,
            "no Tab or ForcedBreak items under preserve-spaces"
        );
    }

    #[test]
    fn break_spaces_still_produces_tab_and_forced_break_items() {
        let style = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::BreakSpaces,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &style, InlineEdges::default())
            .push_text(dom(2), "a\tb\nc")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a\tb\nc");
        let kinds: Vec<_> = p
            .items
            .iter()
            .map(|i| std::mem::discriminant(&i.kind))
            .collect();
        use ItemKind::*;
        let expected: Vec<_> = [
            OpenInline {
                edges: InlineEdges::default(),
            },
            Text,
            Tab,
            Text,
            ForcedBreak,
            Text,
            CloseInline,
        ]
        .iter()
        .map(std::mem::discriminant)
        .collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn dom_offsets_near_u32_max_do_not_panic() {
        let mut b = builder();
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: u32::MAX - 1,
            },
            "abc",
        );
        let p = process(&b.text, &b.items, &b.styles, true, &b.limits).unwrap();
        assert_eq!(p.text, "abc");
        // The mapping itself must not panic either, in both directions.
        let m = p.mapping.unwrap();
        assert!(m.text_to_dom(2, Affinity::Downstream).is_some());
        assert!(m.dom_to_text(NodeId(1), u32::MAX - 1).is_some());
    }

    #[test]
    fn atomics_stop_collapsing_and_are_object_replacement_characters() {
        let mut b = builder();
        b.push_text(dom(1), "a ")
            .push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), " b");
        let p = run(&b);
        assert_eq!(p.text, "a \u{FFFC} b");
        let m = p.mapping.unwrap();
        assert_eq!(
            m.text_to_dom(2, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(2) })
        );
    }

    #[test]
    fn isolates_insert_bidi_controls() {
        let iso = InlineStyle {
            unicode_bidi: UnicodeBidi::Isolate,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &iso, InlineEdges::default())
            .push_text(dom(2), "x")
            .close_inline();
        assert_eq!(run(&b).text, "\u{2066}x\u{2069}");
    }

    #[test]
    fn mapping_can_be_disabled() {
        let mut b = builder();
        b.push_text(dom(1), "a");
        assert!(
            process(&b.text, &b.items, &b.styles, false, &b.limits)
                .unwrap()
                .mapping
                .is_none()
        );
    }
    #[test]
    fn segment_break_neighbors_cross_nodes() {
        let mut b = builder();
        b.push_text(dom(1), "日 \t\n")
            .open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_out_of_flow(NodeId(9), crate::node::OutOfFlowKind::Absolute)
            .push_text(dom(3), " \t本")
            .close_inline();
        assert_eq!(run(&b).text, "日\u{FFFC}本");
        // An actual inline box is a context barrier, unlike an OOF marker.
        let mut b = builder();
        b.push_text(dom(1), "日\n")
            .push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), "本");
        assert_eq!(run(&b).text, "日 \u{FFFC}本");
    }

    #[test]
    fn hangul_keeps_segment_space() {
        let mut b = builder();
        b.push_text(dom(1), "한\n글 日\n本 a\n\nb");
        assert_eq!(run(&b).text, "한 글 日本 a b");
    }

    #[test]
    fn zero_width_space_removes_adjacent_break() {
        for s in ["a\u{200B}\nb", "a\n\u{200B}b", "日\u{FE0F} \n \u{200E}本"] {
            let mut b = builder();
            b.push_text(dom(1), s);
            let expected = s.replace(['\n', ' '], "");
            assert_eq!(run(&b).text, expected);
        }
    }

    #[test]
    fn preserve_breaks_removes_surrounding_collapsible_spaces() {
        let pre_line = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::PreserveBreaks,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &pre_line, InlineEdges::default())
            .push_text(dom(2), "a \t\n \tb")
            .close_inline();
        assert_eq!(run(&b).text, "a\nb");
    }

    #[test]
    fn dom_carriage_return_is_space() {
        for (mode, expected) in [
            (WhiteSpaceCollapse::Preserve, "a \nb"),
            (WhiteSpaceCollapse::Collapse, "a b"),
            (WhiteSpaceCollapse::PreserveSpaces, "a  b"),
        ] {
            let style = ParagraphStyle {
                root: InlineStyle {
                    white_space_collapse: mode,
                    ..InlineStyle::default()
                },
                ..ParagraphStyle::default()
            };
            let mut b = ParagraphBuilder::new(&style, &Limits::default());
            b.push_text(dom(1), "a\r\nb");
            assert_eq!(run(&b).text, expected);
        }
    }

    #[test]
    fn paragraph_breaks_visit_only_active_bidi_scopes() {
        for depth in [32, 128] {
            for breaks in [32, 128] {
                for active in [false, true] {
                    let normal = InlineStyle {
                        white_space_collapse: WhiteSpaceCollapse::Preserve,
                        ..Default::default()
                    };
                    let mut b = builder();
                    for i in 0..depth {
                        let style = InlineStyle {
                            unicode_bidi: if active && i == 0 {
                                UnicodeBidi::Isolate
                            } else if active && i == depth - 1 {
                                UnicodeBidi::Embed
                            } else {
                                UnicodeBidi::Normal
                            },
                            ..normal.clone()
                        };
                        b.open_inline(NodeId(i as u64), &style, InlineEdges::default());
                    }
                    let text = "a\nb".repeat(breaks);
                    b.push_text(dom(1000), &text);
                    for _ in 0..depth {
                        b.close_inline();
                    }
                    BIDI_SCOPE_VISITS.with(|visits| visits.set(0));
                    let p = run(&b);
                    let visits = BIDI_SCOPE_VISITS.with(std::cell::Cell::get);
                    assert_eq!(
                        visits,
                        if active { 4 * breaks } else { 0 },
                        "depth={depth}/breaks={breaks}/active={active}"
                    );
                    let expected = if active {
                        let start = "\u{2066}\u{202A}";
                        let end = "\u{202C}\u{2069}";
                        format!(
                            "{start}{}{end}",
                            text.replace('\n', &format!("{end}\n{start}"))
                        )
                    } else {
                        text
                    };
                    assert_eq!(p.text, expected);
                }
            }
        }
    }

    #[test]
    fn bidi_scope_restarts_keep_order_after_normal_and_active_scopes_close() {
        for (mode, start, end) in [
            (UnicodeBidi::Isolate, "\u{2066}", "\u{2069}"),
            (
                UnicodeBidi::IsolateOverride,
                "\u{2066}\u{202D}",
                "\u{202C}\u{2069}",
            ),
        ] {
            let normal = InlineStyle {
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..Default::default()
            };
            let active = InlineStyle {
                unicode_bidi: mode,
                ..normal.clone()
            };
            let mut b = builder();
            b.open_inline(NodeId(1), &normal, InlineEdges::default())
                .open_inline(NodeId(2), &active, InlineEdges::default())
                .open_inline(NodeId(3), &normal, InlineEdges::default())
                .push_text(dom(4), "a\nb")
                .close_inline()
                .push_forced_break(NodeId(5))
                .close_inline()
                .push_forced_break(NodeId(6))
                .close_inline();
            BIDI_SCOPE_VISITS.with(|visits| visits.set(0));
            let p = run(&b);
            assert_eq!(
                p.text,
                format!("{start}a{end}\n{start}b{end}\n{start}{end}\n")
            );
            assert_eq!(BIDI_SCOPE_VISITS.with(std::cell::Cell::get), 4);
            let at = p.text.find('\n').unwrap() as u32;
            let mapping = p.mapping.unwrap();
            assert_eq!(
                mapping.text_to_dom(at, Affinity::Downstream),
                Some(TextOrigin::Dom {
                    node: NodeId(4),
                    offset: 1
                })
            );
            assert_eq!(
                mapping.text_to_dom(at + 1, Affinity::Downstream),
                Some(TextOrigin::Generated { node: NodeId(2) })
            );
        }
    }

    #[test]
    fn bidi_scopes_restart_at_paragraph_boundaries() {
        let outer = InlineStyle {
            unicode_bidi: UnicodeBidi::Isolate,
            ..InlineStyle::default()
        };
        let inner = InlineStyle {
            unicode_bidi: UnicodeBidi::Embed,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &outer, InlineEdges::default())
            .open_inline(NodeId(2), &inner, InlineEdges::default())
            .push_text(dom(3), "a\nb")
            .push_block_in_inline(NodeId(4))
            .push_text(dom(5), "c")
            .push_forced_break(NodeId(6))
            .push_text(dom(7), "d")
            .close_inline()
            .close_inline();
        let p = run(&b);
        let start = "\u{2066}\u{202A}";
        let end = "\u{202C}\u{2069}";
        assert_eq!(
            p.text,
            format!("{start}a{end}\n{start}b{end}\u{2029}{start}c{end}\n{start}d{end}")
        );
        let m = p.mapping.unwrap();
        let at = p.text.find('\n').unwrap() as u32;
        assert_eq!(
            m.text_to_dom(at, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(3),
                offset: 1
            })
        );
        assert_eq!(
            m.text_to_dom(at + 1, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(1) })
        );
    }

    #[test]
    fn segment_mapping_and_limits() {
        let mut b = builder();
        b.push_text(dom(1), "日 \n 本");
        let p = run(&b);
        assert_eq!(p.text, "日本");
        let m = p.mapping.unwrap();
        assert_eq!(m.dom_to_text(NodeId(1), 4), Some((3, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(3, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 6
            })
        );
        let style = ParagraphStyle {
            root: InlineStyle {
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..InlineStyle::default()
            },
            ..ParagraphStyle::default()
        };
        for limits in [
            Limits {
                max_text_bytes: Some(10),
                ..Limits::default()
            },
            Limits {
                max_items: Some(7),
                ..Limits::default()
            },
        ] {
            let iso = InlineStyle {
                unicode_bidi: UnicodeBidi::Isolate,
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..InlineStyle::default()
            };
            let mut b = ParagraphBuilder::new(&style, &limits);
            b.open_inline(NodeId(1), &iso, InlineEdges::default())
                .push_text(dom(2), "a\nb")
                .close_inline();
            assert!(process(&b.text, &b.items, &b.styles, true, &limits).is_err());
        }
    }

    #[test]
    fn transparent_boundaries_are_linear() {
        let mut b = builder();
        b.push_text(dom(0), "日");
        for i in 1..=4096 {
            b.open_inline(NodeId(i), &InlineStyle::default(), InlineEdges::default())
                .push_text(dom(10000 + i), " \n ")
                .push_out_of_flow(NodeId(20000 + i), crate::node::OutOfFlowKind::Absolute)
                .close_inline();
        }
        b.push_text(dom(50000), "本");
        let p = run(&b);
        assert_eq!(p.text, format!("日{}本", "\u{FFFC}".repeat(4096)));
        assert!(p.items.len() <= 3 * 4096 + 2);
    }
}

#[cfg(test)]
mod ledger_admission_tests {
    use super::*;

    #[test]
    fn exhausted_item_ledger_rejects_before_reserving_the_raw_item_array() {
        let limits = Limits {
            max_items: Some(1024),
            ..Limits::unlimited()
        };
        let mut ledger = crate::limits::ledger::LimitsLedger::new(&limits);
        ledger.charge(LimitKind::Items, 1024).unwrap();
        let styles = [InlineStyle::default()];
        let raw = vec![
            RawItem::Open {
                node: NodeId(1),
                style: 0,
                edges: Default::default()
            };
            1024
        ];
        let (result, peak) = crate::test_support::alloc::measure_peak(|| {
            process_input(
                ProcessInput {
                    raw_text: "",
                    raw: &raw,
                    styles: &styles,
                    with_mapping: false,
                    limits: &limits,
                    annotation: false,
                },
                &mut ledger,
            )
        });
        assert_eq!(
            result.err(),
            Some(LimitExceeded {
                kind: LimitKind::Items,
                limit: 1024,
                actual: 1025
            })
        );
        assert!(
            peak < std::mem::size_of::<Item>() as u64 * raw.len() as u64,
            "rejected input reserved an unadmitted Item array: peak={peak}"
        );
    }
}

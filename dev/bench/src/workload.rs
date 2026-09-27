use crate::BenchError;
use serde::Serialize;
use shodo::{
    AtomicIntrinsics, AtomicSize, AtomicSizes, IntrinsicSizes, LayoutContext, Line, LineConstraint,
    LineResult, Paragraph, ParagraphBuilder, RichText,
};
use shodo::{
    limits::Limits,
    node::{InlineEdges, NodeId, OutOfFlowKind, Sides, TextSource},
    style::{FontFamily, LineOptions, ParagraphStyle, TabSize, TextAlign, WhiteSpaceCollapse},
};
use shodo_fixtures::{FixtureFonts, case, cases, font};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    FirstLine,
    AllLines,
    Intrinsic,
    ReuseWidths,
    RebuildWidths,
    PageRetry,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Corpus,
    Many,
    Nested,
    Tabs,
    Float,
    Justify,
    Fallback,
}

#[derive(Clone, Debug)]
pub struct Workload {
    pub id: String,
    pub scale: usize,
    pub width: f32,
    pub text: String,
    kind: Kind,
    style: ParagraphStyle,
}
impl Workload {
    pub fn named(id: &str, scale: usize) -> Result<Self, BenchError> {
        if ![1, 8, 64].contains(&scale) {
            return Err(BenchError("scale must be1,8,64".into()));
        }
        let (base, kind) = match id {
            "many-short-latin" => ("latin-short", Kind::Many),
            "nested-atomic" => ("latin-short", Kind::Nested),
            "preserved-tabs" => ("latin-short", Kind::Tabs),
            "float-retry" => ("latin-short", Kind::Float),
            "justify" => ("latin-short", Kind::Justify),
            "fallback" => ("mixed-scripts", Kind::Fallback),
            _ => (id, Kind::Corpus),
        };
        let c = case(base).ok_or_else(|| BenchError(format!("unknown workload {id}")))?;
        let mut style = ParagraphStyle {
            direction: c.direction,
            ..Default::default()
        };
        style.root.direction = c.direction;
        style.root.font_size = c.font_size;
        style.root.lang = c.lang.clone();
        style.root.font_families = c
            .font_ids
            .iter()
            .take(if kind == Kind::Fallback { 1 } else { 3 })
            .map(|id| FontFamily::Named(font(id).unwrap().family.into()))
            .collect();
        if kind == Kind::Tabs {
            style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
            style.root.tab_size = TabSize::Spaces(8.0);
            style.root.font_size = 16.0;
        }
        let source = if kind == Kind::Tabs {
            "One  two\tthree\nFour five."
        } else {
            &c.text
        };
        let text = if kind == Kind::Many {
            source.to_owned()
        } else {
            source.repeat(scale)
        };
        Ok(Self {
            id: id.into(),
            scale,
            width: if kind == Kind::Tabs { 90.0 } else { c.width },
            text,
            kind,
            style,
        })
    }
    pub fn settings(&self) -> serde_json::Value {
        serde_json::json!({"id":self.id,"scale":self.scale,"width":self.width,"widths":self.widths(),"text":self.text,"kind":self.kind,"style":format!("{:?}",self.style),"paragraphs":self.paragraph_count(),"nested_depth":if self.kind==Kind::Nested {4} else {0},"atomic_size":[20,20],"atomic_baseline":16,"end_padding":2,"float_widths":[40,75],"height_limit":0})
    }
    pub fn paragraph_count(&self) -> usize {
        if self.kind == Kind::Many {
            self.scale
        } else {
            1
        }
    }
    pub fn widths(&self) -> [f32; 3] {
        [self.width, self.width / 2.0, self.width * 1.5]
    }
    pub fn atomics(&self) -> AtomicSizes {
        let mut sizes = AtomicSizes::new();
        if self.kind == Kind::Nested {
            sizes.insert(
                NodeId(1000),
                AtomicSize {
                    inline_size: 20.0,
                    block_size: 20.0,
                    baseline: Some(16.0),
                    ..Default::default()
                },
            );
        }
        sizes
    }
    pub fn intrinsics(&self) -> AtomicIntrinsics {
        let mut inputs = AtomicIntrinsics::new();
        if self.kind == Kind::Nested {
            inputs.insert_atomic(
                NodeId(1000),
                shodo::AtomicIntrinsic {
                    min_content: 20.0,
                    max_content: 20.0,
                },
            );
        }
        if self.kind == Kind::Float {
            for (node, width) in [(1001, 40.0), (1002, 75.0)] {
                inputs.insert_float(
                    NodeId(node),
                    shodo::FloatIntrinsic {
                        min_content: width,
                        max_content: width,
                        ..Default::default()
                    },
                );
            }
        }
        inputs
    }
    fn options(&self) -> LineOptions {
        LineOptions {
            text_align: if self.kind == Kind::Justify {
                TextAlign::Justify
            } else {
                TextAlign::Start
            },
            ..Default::default()
        }
    }
    pub fn build(
        &self,
        cx: &mut LayoutContext,
        fonts: &FixtureFonts,
        limits: &Limits,
    ) -> Result<Vec<Paragraph>, BenchError> {
        let mut paragraphs = Vec::with_capacity(self.paragraph_count());
        for _ in 0..self.paragraph_count() {
            let p = if matches!(
                self.kind,
                Kind::Corpus | Kind::Many | Kind::Fallback | Kind::Justify
            ) {
                RichText::with_limits(&self.style, limits)
                    .push(&self.text, &self.style.root)
                    .build(cx, &fonts.collection)
            } else {
                let mut b = ParagraphBuilder::new(&self.style, limits);
                if self.kind == Kind::Nested {
                    for i in 0..4 {
                        b.open_inline(
                            NodeId(i),
                            &self.style.root,
                            InlineEdges {
                                padding: Sides {
                                    inline_end: 2.0,
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                    b.push_atomic(NodeId(1000), &self.style.root, InlineEdges::default());
                }
                if self.kind == Kind::Float {
                    b.push_out_of_flow(NodeId(1001), OutOfFlowKind::Float)
                        .push_out_of_flow(NodeId(1002), OutOfFlowKind::Float);
                }
                b.push_text(TextSource::Generated { node: NodeId(100) }, &self.text);
                if self.kind == Kind::Nested {
                    for _ in 0..4 {
                        b.close_inline();
                    }
                }
                b.build(cx, &fonts.collection)
            }
            .map_err(|e| BenchError(e.to_string()))?;
            paragraphs.push(p);
        }
        Ok(paragraphs)
    }
}
pub fn workloads() -> Vec<Workload> {
    cases()
        .iter()
        .map(|c| c.id.as_str())
        .chain([
            "many-short-latin",
            "nested-atomic",
            "preserved-tabs",
            "float-retry",
            "justify",
            "fallback",
        ])
        .flat_map(|id| [1, 8, 64].map(|scale| Workload::named(id, scale).unwrap()))
        .collect()
}
#[derive(Default)]
pub struct Run {
    pub lines: Vec<Line>,
    pub float_reports: usize,
    pub height_retries: usize,
    pub intrinsics: Vec<IntrinsicSizes>,
}

pub fn layout(
    w: &Workload,
    paragraphs: &[Paragraph],
    cx: &mut LayoutContext,
    fonts: &FixtureFonts,
    limits: &Limits,
    operation: Operation,
) -> Result<Run, BenchError> {
    if paragraphs.len() != w.paragraph_count() {
        return Err(BenchError("wrong paragraph count".into()));
    }
    let options = w.options();
    let atomics = w.atomics();
    let mut run = Run::default();
    if operation == Operation::Intrinsic {
        for p in paragraphs {
            run.intrinsics
                .push(p.intrinsic_sizes(cx, &options, &w.intrinsics()));
        }
        return Ok(run);
    }
    let widths: Vec<_> = if matches!(operation, Operation::ReuseWidths | Operation::RebuildWidths) {
        w.widths().to_vec()
    } else {
        vec![w.width]
    };
    for width in widths {
        let rebuilt;
        let paragraphs = if operation == Operation::RebuildWidths {
            rebuilt = w.build(cx, fonts, limits)?;
            &rebuilt
        } else {
            paragraphs
        };
        for p in paragraphs {
            let mut token = p.start_token();
            let mut constraint = LineConstraint::new(width);
            let mut consumed = 0;
            let mut done = false;
            let mut float_width = 0.0;
            let mut float_count = 0;
            let budget = p
                .text()
                .len()
                .saturating_mul(4)
                .saturating_add(32)
                .min(2_000_000);
            for _ in 0..budget {
                let result = if operation == Operation::PageRetry {
                    let rejected = LineConstraint {
                        max_block_size: Some(0.0),
                        ..constraint
                    };
                    match p.next_line(cx, token, &options, &rejected, &atomics) {
                        LineResult::BlockSizeExceeded { .. } => {
                            run.height_retries += 1;
                            p.next_line(cx, token, &options, &constraint, &atomics)
                        }
                        other => other,
                    }
                } else {
                    p.next_line(cx, token, &options, &constraint, &atomics)
                };
                match result {
                    LineResult::Line(line) => {
                        if line.text_range().end <= consumed {
                            return Err(BenchError("line source did not progress".into()));
                        }
                        consumed = line.text_range().end;
                        token = line.break_token();
                        run.lines.push(line);
                        if operation == Operation::FirstLine {
                            done = true;
                            break;
                        }
                    }
                    LineResult::FloatEncountered { float_cursor, .. } => {
                        if constraint.floats_placed_through == Some(float_cursor) {
                            return Err(BenchError("float retry did not progress".into()));
                        }
                        run.float_reports += 1;
                        constraint.floats_placed_through = Some(float_cursor);
                        if float_count >= 2 {
                            return Err(BenchError("unexpected float report".into()));
                        }
                        float_width += [40.0, 75.0][float_count];
                        float_count += 1;
                        constraint.available_inline_size = (width - float_width).max(1.0);
                    }
                    LineResult::Done => {
                        if consumed != p.text().len() {
                            return Err(BenchError("incomplete source consumption".into()));
                        }
                        done = true;
                        break;
                    }
                    other => return Err(BenchError(format!("unexpected line result {other:?}"))),
                }
            }
            if !done {
                return Err(BenchError("retry limit exceeded".into()));
            }
        }
    }
    Ok(run)
}

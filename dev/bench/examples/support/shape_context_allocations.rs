use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, Hyphens, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder};
use shodo_bench::Run;
use shodo_fixtures::FixtureFonts;

pub const CONTEXT_RUNS: usize = 48;

struct Span {
    style: InlineStyle,
    text: String,
}

pub struct Workloads {
    limits: Limits,
    context_style: ParagraphStyle,
    context_spans: Vec<Span>,
    edge_paragraph: Paragraph,
}

impl Workloads {
    pub fn new(fonts: &FixtureFonts, limits: &Limits) -> Self {
        let families = ["cjk", "latin"]
            .into_iter()
            .map(|id| FontFamily::Named(shodo_fixtures::font(id).unwrap().family.into()))
            .collect();
        let root = InlineStyle {
            font_size: 16.0,
            font_families: families,
            lang: Some("ja".into()),
            ..Default::default()
        };
        let context_style = ParagraphStyle {
            root: root.clone(),
            ..Default::default()
        };
        let context_spans = (0..CONTEXT_RUNS)
            .map(|index| {
                let mut style = root.clone();
                style.font_size = if index % 2 == 0 { 15.0 } else { 16.0 };
                Span {
                    style,
                    text: "𠮷".repeat(5),
                }
            })
            .collect();

        let edge_style = InlineStyle {
            font_size: 14.0,
            font_families: ["latin", "cjk", "arabic"]
                .into_iter()
                .map(|id| FontFamily::Named(shodo_fixtures::font(id).unwrap().family.into()))
                .collect(),
            hyphens: Hyphens::Manual,
            ..Default::default()
        };
        let edge_paragraph_style = ParagraphStyle {
            root: edge_style,
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&edge_paragraph_style, limits);
        builder.push_text(
            TextSource::Generated {
                node: NodeId(900_000),
            },
            &"hyphen\u{ad}ation ".repeat(32),
        );
        let edge_paragraph = builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .expect("soft-hyphen edge workload builds");

        Self {
            limits: limits.clone(),
            context_style,
            context_spans,
            edge_paragraph,
        }
    }

    pub fn build_context_paragraph(&self, fonts: &FixtureFonts) -> Paragraph {
        let mut builder = ParagraphBuilder::new(&self.context_style, &self.limits);
        for (index, span) in self.context_spans.iter().enumerate() {
            let inline = NodeId(1_000_000 + index as u64 * 2);
            builder.open_inline(inline, &span.style, InlineEdges::default());
            builder.push_text(
                TextSource::Generated {
                    node: NodeId(inline.0 + 1),
                },
                &span.text,
            );
            builder.close_inline();
        }
        builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .expect("alternating four-byte context workload builds")
    }

    pub fn layout_edge(&self) -> Run {
        layout(&self.edge_paragraph, 45.0, &LineOptions::default())
    }
}

pub fn layout(paragraph: &Paragraph, width: f32, options: &LineOptions) -> Run {
    let mut cx = LayoutContext::new();
    let constraint = LineConstraint::new(width);
    let mut token = paragraph.start_token();
    let mut run = Run::default();
    for _ in 0..4096 {
        match paragraph.next_line(&mut cx, token, options, &constraint, &AtomicSizes::EMPTY) {
            LineResult::Line(line) => {
                assert_ne!(line.break_token(), token, "layout made no progress");
                token = line.break_token();
                run.lines.push(line);
            }
            LineResult::Done => return run,
            other => panic!("unexpected layout result: {other:?}"),
        }
    }
    panic!("bounded shape-context workload exceeded 4096 lines")
}

pub fn digest(run: &Run, fonts: &FixtureFonts) -> String {
    shodo_bench::digest(run, fonts).unwrap().sha256
}

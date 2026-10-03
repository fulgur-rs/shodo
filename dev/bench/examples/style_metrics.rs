//! Paired build timing and allocation probe for paragraph style metrics.
//! Run without features for timing, then with `allocation-counting` and `alloc`.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::geometry::LogicalRect;
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder};
use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const WARMUP: usize = 3;

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    styles: usize,
    distinct_metrics: bool,
}

fn cases() -> [Case; 5] {
    [
        Case {
            name: "paint-only-1",
            styles: 1,
            distinct_metrics: false,
        },
        Case {
            name: "paint-only-64",
            styles: 64,
            distinct_metrics: false,
        },
        Case {
            name: "paint-only-1024",
            styles: 1024,
            distinct_metrics: false,
        },
        Case {
            name: "distinct-64",
            styles: 64,
            distinct_metrics: true,
        },
        Case {
            name: "distinct-1024",
            styles: 1024,
            distinct_metrics: true,
        },
    ]
}

fn paint_color(index: usize) -> [u8; 4] {
    [index as u8, (index >> 8) as u8, 17, 255]
}

fn builder(case: Case, limits: &Limits) -> ParagraphBuilder {
    let mut root = InlineStyle {
        font_families: vec![FontFamily::Named(
            shodo_fixtures::font("latin").unwrap().family.into(),
        )],
        font_size: 16.0,
        ..Default::default()
    };
    root.paint.color = [255, 255, 255, 255];
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    for i in 0..case.styles {
        let mut style = root.clone();
        style.paint.color = paint_color(i);
        if case.distinct_metrics {
            style.font_size = 16.125 + i as f32 * 0.001;
        }
        let node = NodeId(i as u64 + 1);
        builder
            .open_inline(node, &style, InlineEdges::default())
            .push_text(TextSource::Generated { node }, "a")
            .close_inline();
    }
    builder
}

fn add_usize(hash: &mut Sha256, value: usize) {
    hash.update((value as u64).to_le_bytes());
}

fn add_f32(hash: &mut Sha256, value: f32) {
    hash.update(value.to_bits().to_le_bytes());
}

fn add_rect(hash: &mut Sha256, rect: LogicalRect) {
    for value in [
        rect.inline_start,
        rect.block_start,
        rect.inline_size,
        rect.block_size,
    ] {
        add_f32(hash, value);
    }
}

struct Snapshot {
    paint_spans: usize,
    paint_sha256: String,
    geometry_sha256: String,
    warning_sha256: String,
    warnings: Vec<String>,
}

fn snapshot(paragraph: &Paragraph, context: &mut LayoutContext, case: Case) -> Snapshot {
    let lines = paragraph.break_all(
        context,
        &LineOptions::default(),
        1_000_000.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1, "fixture should remain a single line");
    let spans: Vec<_> = lines.iter().flat_map(|line| line.paint_spans()).collect();
    assert_eq!(
        spans.len(),
        case.styles,
        "paint span count for {}",
        case.name
    );
    let colors: BTreeSet<_> = spans.iter().map(|span| span.style.color).collect();
    assert_eq!(
        colors.len(),
        case.styles,
        "distinct paint colors for {}",
        case.name
    );
    for (i, span) in spans.iter().enumerate() {
        assert_eq!(span.text_range, i..i + 1, "span range for {}", case.name);
        assert_eq!(
            span.style.color,
            paint_color(i),
            "span color for {}",
            case.name
        );
    }

    let mut paint = Sha256::new();
    add_usize(&mut paint, spans.len());
    for span in &spans {
        add_usize(&mut paint, span.text_range.start);
        add_usize(&mut paint, span.text_range.end);
        paint.update(span.style.color);
    }

    let mut geometry = Sha256::new();
    add_usize(&mut geometry, lines.len());
    for (line_index, line) in lines.iter().enumerate() {
        add_usize(&mut geometry, line.text_range().start);
        add_usize(&mut geometry, line.text_range().end);
        for value in [
            line.inline_size(),
            line.block_size(),
            line.block_offset(),
            line.hang_start(),
            line.hang_end(),
        ] {
            add_f32(&mut geometry, value);
        }
        let line_spans = line.paint_spans();
        for span in &line_spans {
            add_usize(&mut geometry, span.text_range.start);
            add_usize(&mut geometry, span.text_range.end);
            add_rect(&mut geometry, span.rect);
        }
        let layout = LineLayout::new(std::slice::from_ref(line));
        for offset in 0..=case.styles {
            let position = TextPosition {
                line: 0,
                offset: offset as u32,
                affinity: Affinity::Downstream,
            };
            let caret = layout.caret(position).expect("accepted caret stop");
            add_usize(&mut geometry, caret.position.offset as usize);
            add_rect(&mut geometry, caret.rect);
        }
        for offset in 0..case.styles {
            let start = TextPosition {
                line: 0,
                offset: offset as u32,
                affinity: Affinity::Downstream,
            };
            let end = TextPosition {
                line: 0,
                offset: offset as u32 + 1,
                affinity: Affinity::Downstream,
            };
            let rects = layout.selection_rects(start, end);
            add_usize(&mut geometry, rects.len());
            for rect in rects {
                add_rect(&mut geometry, rect);
            }
        }
        assert_eq!(line_index, 0);
    }

    let mut warnings: Vec<_> = paragraph
        .warnings()
        .iter()
        .map(|warning| format!("{:?}:{}", warning.kind, warning.message))
        .collect();
    warnings.extend(
        context
            .take_warnings()
            .into_iter()
            .map(|warning| format!("{:?}:{}", warning.kind, warning.message)),
    );
    let mut warning_hash = Sha256::new();
    for warning in &warnings {
        warning_hash.update((warning.len() as u64).to_le_bytes());
        warning_hash.update(warning.as_bytes());
    }

    Snapshot {
        paint_spans: spans.len(),
        paint_sha256: format!("{:x}", paint.finalize()),
        geometry_sha256: format!("{:x}", geometry.finalize()),
        warning_sha256: format!("{:x}", warning_hash.finalize()),
        warnings,
    }
}

fn sample(case: Case, mode: &str, fonts: &shodo_fixtures::FixtureFonts, index: usize) -> Value {
    let limits = Limits::default();
    let builder = builder(case, &limits);
    let mut context = LayoutContext::new();
    let (paragraph, measurement) = match mode {
        "time" => {
            let start = Instant::now();
            let paragraph = black_box(builder)
                .build(&mut context, &fonts.collection)
                .unwrap();
            let elapsed = start.elapsed().as_nanos() as u64;
            black_box(&paragraph);
            (paragraph, json!({"ns":elapsed}))
        }
        "alloc" => {
            #[cfg(feature = "allocation-counting")]
            {
                let scope = ALLOC.begin().unwrap();
                let paragraph = black_box(builder)
                    .build(&mut context, &fonts.collection)
                    .unwrap();
                let allocation = scope.finish();
                black_box(&paragraph);
                (paragraph, json!({"allocation":allocation}))
            }
            #[cfg(not(feature = "allocation-counting"))]
            panic!("alloc mode requires --features allocation-counting");
        }
        _ => panic!("mode must be time or alloc"),
    };
    let output = snapshot(&paragraph, &mut context, case);
    assert!(
        output.warnings.is_empty(),
        "unexpected warnings: {:?}",
        output.warnings
    );
    json!({
        "schema": 1,
        "mode": mode,
        "fixture": case.name,
        "style_count": case.styles,
        "sample": index,
        "measurement": measurement,
        "output": {
            "paint_spans": output.paint_spans,
            "paint_sha256": output.paint_sha256,
            "geometry_sha256": output.geometry_sha256,
            "warning_sha256": output.warning_sha256,
            "warnings": output.warnings,
        }
    })
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "time".into());
    let expected_mode = if cfg!(feature = "allocation-counting") {
        "alloc"
    } else {
        "time"
    };
    assert_eq!(
        mode, expected_mode,
        "use separate time and allocation builds"
    );
    let samples = if mode == "time" { 21 } else { 9 };
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for case in cases() {
        for _ in 0..WARMUP {
            black_box(sample(case, &mode, &fonts, usize::MAX));
        }
        for index in 0..samples {
            println!(
                "{}",
                serde_json::to_string(&sample(case, &mode, &fonts, index)).unwrap()
            );
        }
    }
}

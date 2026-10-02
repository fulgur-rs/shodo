//! Fixed-font unit-building A/B. Separate `time` and allocation feature `alloc`.
use serde_json::{Value, json};
use shodo::font::{FontCollection, FontOptions};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, Paragraph, ParagraphBuilder};
use std::{hint::black_box, time::Instant};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
struct Case {
    name: &'static str,
    text: String,
    many: bool,
    missing: bool,
}
fn cases() -> Vec<Case> {
    [
        ("latin", "abc def ".repeat(128)),
        ("cjk", "日本語文字".repeat(64)),
        ("combining", "a\u{301}\u{300} b ".repeat(64)),
        ("rtl", "سلام لا ".repeat(64)),
        ("giant", format!("a{} b", "\u{301}".repeat(1024))),
        ("many-runs", "a b ".repeat(64)),
        ("missing", "a\u{301} 日本語 ".repeat(64)),
        ("windowed", "abc日本語 ".repeat(64)),
    ]
    .into_iter()
    .map(|(name, text)| Case {
        name,
        text,
        many: name == "many-runs",
        missing: name == "missing",
    })
    .collect()
}
fn builder(c: &Case, limits: &Limits) -> ParagraphBuilder {
    let root = InlineStyle {
        font_families: shodo_fixtures::FONTS
            .iter()
            .map(|f| FontFamily::Named(f.family.into()))
            .collect(),
        ..Default::default()
    };
    let ps = ParagraphStyle {
        root: root.clone(),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ps, limits);
    if c.many {
        let mut offset = 0;
        for (i, part) in c.text.split_inclusive(' ').enumerate() {
            let style = InlineStyle {
                font_size: 16.0 + (i % 2) as f32,
                ..root.clone()
            };
            b.open_inline(NodeId(i as u64 + 1), &style, InlineEdges::default());
            b.push_text(
                TextSource::Dom {
                    node: NodeId(i as u64 + 1),
                    offset,
                },
                part,
            );
            b.close_inline();
            offset += part.len() as u32;
        }
    } else {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            &c.text,
        );
    }
    b
}
fn snapshot(p: &Paragraph, cx: &mut LayoutContext) -> Value {
    let mut widths = Vec::new();
    for width in [48., 160., 1024.] {
        let lines = p.break_all(cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
        widths.push(lines.iter().map(|l| {
            let runs=l.fragments().map(|f|match f {
                Fragment::GlyphRun(r)=>json!({"kind":"glyph","font":[r.font().layer(),r.font().index()],"style":r.style_index(),"script":r.script(),"language":r.language(),"range":r.text_range(),"bidi":r.bidi_level(),"metrics":format!("{:?}",r.metrics()),"vertical":format!("{:?}",r.vertical_metrics()),"variations":format!("{:?}",r.variations()),"coords":format!("{:?}",r.normalized_coords()),"paint":format!("{:?}",r.paint_style()),"transform":format!("{:?}",r.glyph_transform()),"orientation":format!("{:?}",r.orientation()),"source":format!("{:?}",r.source()),"embolden":r.embolden(),"skew":r.skew().map(f32::to_bits),"placement":[r.inline_start().to_bits(),r.inline_size().to_bits(),r.baseline().to_bits(),r.font_size().to_bits()],"clusters":format!("{:?}",r.clusters().collect::<Vec<_>>()),"glyphs":r.glyphs().enumerate().map(|(i,g)|json!([g.id,g.cluster,g.inline_position.to_bits(),g.block_offset.to_bits(),g.advance.to_bits(),r.glyph_origin(i).map(|(x,y)|[x.to_bits(),y.to_bits()])])).collect::<Vec<_>>()}),
                _=>json!({"kind":"other","debug":format!("{f:?}")}),
            }).collect::<Vec<_>>();
            json!({"text":l.text(),"range":l.text_range(),"geometry":[l.inline_size().to_bits(),l.block_size().to_bits(),l.block_offset().to_bits(),l.hang_start().to_bits(),l.hang_end().to_bits()],"reason":format!("{:?}",l.break_reason()),"metrics":format!("{:?}",l.metrics()),"overflow":format!("{:?}",l.overflow_rect()),"owners":format!("{:?}",l.owners().collect::<Vec<_>>()),"combinations":format!("{:?}",l.text_combinations().collect::<Vec<_>>()),"runs":runs})
        }).collect::<Vec<_>>());
    }
    json!({"text":p.text(),"mapping":format!("{:?}",p.offset_mapping()),"warnings":format!("{:?}",p.warnings()),"line_warnings":format!("{:?}",cx.take_warnings()),"lines":widths})
}

fn main() {
    let mode = std::env::args().nth(1).expect("time or alloc");
    let fixed = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    let missing = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut rows = Vec::new();
    for c in cases() {
        let limits = Limits {
            max_shaping_run_bytes: if c.name == "windowed" {
                Some(32)
            } else {
                Limits::default().max_shaping_run_bytes
            },
            ..Default::default()
        };
        let fonts = if c.missing {
            &missing
        } else {
            &fixed.collection
        };
        let mut cx = LayoutContext::new();
        for _ in 0..3 {
            black_box(builder(&c, &limits).build(&mut cx, fonts).unwrap());
        }
        let mut samples = Vec::new();
        for _ in 0..if mode == "time" { 9 } else { 3 } {
            let b = builder(&c, &limits);
            match mode.as_str() {
                "time" => {
                    let start = Instant::now();
                    let p = b.build(&mut cx, fonts).unwrap();
                    let elapsed = start.elapsed().as_nanos();
                    black_box(&p);
                    samples.push(json!(elapsed));
                }
                #[cfg(feature = "allocation-counting")]
                "alloc" => {
                    let scope = ALLOC.begin().unwrap();
                    let p = b.build(&mut cx, fonts).unwrap();
                    let count = scope.finish();
                    black_box(&p);
                    samples.push(json!(count));
                }
                _ => panic!("alloc needs allocation-counting"),
            }
        }
        let p = builder(&c, &limits).build(&mut cx, fonts).unwrap();
        let large_output = json!({"text":p.text(),"mapping":format!("{:?}",p.offset_mapping()),"warnings":format!("{:?}",p.warnings())});
        let small = Case {
            name: c.name,
            text: c.text.chars().take(24).collect(),
            many: c.many,
            missing: c.missing,
        };
        let small_p = builder(&small, &limits).build(&mut cx, fonts).unwrap();
        let output = snapshot(&small_p, &mut cx);
        let controls = [0, 1, 8].map(|cap| {
            let limited = Limits {
                max_shaped_glyphs: Some(cap),
                ..limits.clone()
            };
            match builder(&small, &limited).build(&mut LayoutContext::new(), fonts) {
                Ok(p) => format!("ok:{:?}:{:?}", p.text(), p.warnings()),
                Err(e) => format!("error:{e:?}"),
            }
        });
        rows.push(json!({"case":c.name,"bytes":c.text.len(),"samples":samples,"large_output":large_output,"output":output,"controls":controls}));
    }
    println!(
        "{}",
        json!({"mode":mode,"scope":"build of prebuilt builder; paragraph/context retained; layout/JSON/drop excluded","warmup":3,"fonts":shodo_fixtures::FONTS.iter().map(|f|(f.id,f.sha256)).collect::<Vec<_>>(),"rows":rows})
    );
}

//! Lossless public output probe; Debug fields are pinned to the archived source.
use serde_json::{Value, json};
use shodo::{Fragment, Line, LineResult};

pub fn token(token: shodo::BreakToken) -> String {
    let s = format!("{token:?}");
    let (_, tail) = s.split_once(", unit:").expect("pinned BreakToken Debug");
    format!("BreakToken {{ para: <owner>, unit:{tail}")
}
pub fn plan(plan: &shodo::BreakPlan) -> String {
    let s = format!("{plan:?}");
    let (_, tail) = s.split_once(", width:").expect("pinned BreakPlan Debug");
    format!("BreakPlan {{ para: <owner>, width:{tail}")
}
pub fn line(l: &Line) -> Value {
    let fragments = l.fragments().map(|f| match f {
        Fragment::GlyphRun(r) => {
            let data = r.font_data().expect("fixed retained font");
            let font = shodo_fixtures::FONTS.iter().find(|f| f.face_index == data.index && f.bytes == data.data.as_ref()).expect("fixed font bytes");
            let (font_id, font_sha256) = (font.id, font.sha256);
            assert!(r.glyphs().all(|g| g.id != 0), "unexpected missing glyph in matched fixed face");
            let clusters=r.clusters().map(|c|{let source=l.text().get(c.text_range.clone()).expect("cluster source");assert_eq!(c.source_char,source.chars().next());json!({"range":c.text_range,"advance_bits":c.advance.to_bits(),"shaping_advance_bits":c.shaping_advance.to_bits(),"source_char":c.source_char,"flags":{"whitespace":c.flags.whitespace,"punctuation":c.flags.punctuation,"synthetic_hyphen":c.flags.synthetic_hyphen,"emphasis_excluded":c.flags.emphasis_excluded}})}).collect::<Vec<_>>();
            json!({"kind":"glyph", "style_index":r.style_index(),"script":r.script(),"language":r.language(),"embolden":r.embolden(),"skew_bits":r.skew().map(f32::to_bits),"vertical_metrics_bits":r.vertical_metrics().map(|m|[m.ascent.to_bits(),m.descent.to_bits(),m.line_gap.to_bits()]),"clusters":clusters, "node":r.node().map(|n|n.0), "range":r.text_range(), "font":font_id,"font_sha256":font_sha256,"font_index":data.index,"font_id":json!({"layer":"<owner>","index":r.font().index()}),"size_bits":r.font_size().to_bits(),"bidi":r.bidi_level(),"paint":format!("{:?}",r.paint_style()),"coords":format!("{:?}",r.normalized_coords()),"variations":format!("{:?}",r.variations()),"orientation":format!("{:?}",r.orientation()),"transform":format!("{:?}",r.glyph_transform()),"metrics":format!("{:?}",r.metrics()),"inline_bits":r.inline_start().to_bits(),"size_inline_bits":r.inline_size().to_bits(),"baseline_bits":r.baseline().to_bits(),"glyphs":r.glyphs().enumerate().map(|(i,g)|json!({"id":g.id,"cluster":g.cluster,"inline_bits":g.inline_position.to_bits(),"block_bits":g.block_offset.to_bits(),"advance_bits":g.advance.to_bits(),"origin_bits":r.glyph_origin(i).map(|(x,y)|[x.to_bits(),y.to_bits()])})).collect::<Vec<_>>()})
        }
        Fragment::RubyAnnotation(_) => json!({"kind":"ruby"}),
        _ => json!({"kind":"other","debug":format!("{f:?}")}),
    }).collect::<Vec<_>>();
    let ruby = l.ruby_annotations().map(|a|json!({"container":a.container().0,"nodes":a.base_nodes().iter().map(|n|n.0).collect::<Vec<_>>(),"node":a.node().map(|n|n.0),"level":a.level(),"base_range":a.base_text_range(),"range":a.text_range(),"visibility":format!("{:?}",a.visibility()),"origin":a.origin(),"transform":format!("{:?}",a.transform()),"line":line(a.line())})).collect::<Vec<_>>();
    json!({"text":l.text(),"range":l.text_range(),"mapping":l.offset_mapping().map(|m|format!("{m:?}")),"token":token(l.break_token()),"reason":format!("{:?}",l.break_reason()),"last":l.is_last(),"geometry_bits":[l.inline_size().to_bits(),l.block_size().to_bits(),l.block_offset().to_bits(),l.hang_start().to_bits(),l.hang_end().to_bits()],"metrics":format!("{:?}",l.metrics()),"overflow":format!("{:?}",l.overflow_rect()),"writing":format!("{:?}",l.writing_mode()),"direction":format!("{:?}",l.used_direction()),"displaced":format!("{:?}",l.displaced_floats()),"combinations":format!("{:?}",l.text_combinations().collect::<Vec<_>>()),"fragments":fragments,"ruby":ruby})
}
pub fn results(results: &[LineResult]) -> Value {
    json!(
        results
            .iter()
            .map(|r| match r {
                LineResult::Line(l) => line(l),
                LineResult::BlockInInline { node, token_after } =>
                    json!({"block":node.0,"token":token(*token_after)}),
                LineResult::Done => json!({"done":true}),
                other => panic!("unexpected accepted result: {other:?}"),
            })
            .collect::<Vec<_>>()
    )
}

/// Exact public event fields; Line geometry is retained separately in results.
pub fn event(r: &LineResult) -> Value {
    match r {
        LineResult::Line(l) => {
            json!({"line_token":token(l.break_token()),"range":l.text_range(),"reason":format!("{:?}",l.break_reason())})
        }
        LineResult::BlockSizeExceeded { needed_block_size } => {
            json!({"height_needed_bits":needed_block_size.to_bits()})
        }
        LineResult::FloatEncountered {
            node,
            line_start,
            inline_position,
            float_cursor,
        } => {
            json!({"float":node.0,"token":token(*line_start),"position_bits":inline_position.to_bits(),"cursor":format!("{float_cursor:?}")})
        }
        LineResult::BlockInInline { node, token_after } => {
            json!({"block":node.0,"token":token(*token_after)})
        }
        LineResult::Done => json!({"done":true}),
        LineResult::InvalidToken => json!({"invalid":true}),
        _ => panic!("new event requires explicit output coverage"),
    }
}

//! Complete accepted output fields used by development caller differential checks.
use super::caller::Output;
use serde_json::{Value, json};
use shodo::Fragment;
pub fn output(out: &Output) -> Value {
    let lines=out.lines.iter().map(|l|{
        let fragments=l.fragments().map(|f|match f {
            Fragment::GlyphRun(r)=>json!({"kind":"glyph","node":r.node().map(|n|n.0),"text":r.text_range(),"font":format!("{:?}",r.font()),"size":r.font_size(),"bidi":r.bidi_level(),"paint":format!("{:?}",r.paint_style()),"coords":format!("{:?}",r.normalized_coords()),"variations":format!("{:?}",r.variations()),"glyphs":r.glyphs().map(|g|json!([g.id as f64,g.cluster as f64,g.inline_position as f64,g.block_offset as f64,g.advance as f64])).collect::<Vec<_>>() }),
            _=>json!({"kind":"other","value":format!("{f:?}")}),
        }).collect::<Vec<_>>();
        json!({"text":l.text(),"range":l.text_range(),"inline":l.inline_size(),"block":l.block_size(),"offset":l.block_offset(),"metrics":format!("{:?}",l.metrics()),"writing":format!("{:?}",l.writing_mode()),"direction":format!("{:?}",l.used_direction()),"reason":format!("{:?}",l.break_reason()),"last":l.is_last(),"mapping":l.offset_mapping().map(|m|format!("{:?}",m.units())),"fragments":fragments})
    }).collect::<Vec<_>>();
    let links=out.links.iter().map(|r|json!({"line":r.line,"node":r.node.0,"dom":r.dom,"text":r.text,"kind":format!("{:?}",r.kind),"element":r.link.element.0,"href":r.link.href,"rect":[r.rect.inline_start,r.rect.block_start,r.rect.inline_size,r.rect.block_size]})).collect::<Vec<_>>();
    json!({"lines":lines,"links":links})
}

//! Complete accepted output fields used by development caller differential checks.
use super::caller::Output;
use serde_json::{Value, json};
use shodo::Fragment;
pub fn output(out: &Output) -> Value {
    let mut mappings = Vec::<String>::new();
    let lines=out.lines.iter().map(|l|{
        let fragments=l.fragments().map(|f|match f {
            Fragment::GlyphRun(r)=>json!({"kind":"glyph","node":r.node().map(|n|n.0),"text":r.text_range(),"font":format!("{:?}",r.font()),"size":r.font_size(),"bidi":r.bidi_level(),"paint":format!("{:?}",r.paint_style()),"coords":format!("{:?}",r.normalized_coords()),"variations":format!("{:?}",r.variations()),"glyphs":r.glyphs().map(|g|json!([g.id as f64,g.cluster as f64,g.inline_position as f64,g.block_offset as f64,g.advance as f64])).collect::<Vec<_>>() }),
            Fragment::InlineBox(b)=>json!({"kind":"inline_box","node":b.node.0,"rect":[b.rect.inline_start,b.rect.block_start,b.rect.inline_size,b.rect.block_size],"content_rect":[b.content_rect.inline_start,b.content_rect.block_start,b.content_rect.inline_size,b.content_rect.block_size],"slice_offset":b.slice_offset,"baseline":b.baseline,"has_start_edge":b.has_start_edge,"has_end_edge":b.has_end_edge,"start_edge_is_reversed":b.start_edge_is_reversed,"parent":b.parent,"font":format!("{:?}",b.font),"font_size":b.font_size}),
            _=>json!({"kind":"other","value":format!("{f:?}")}),
        }).collect::<Vec<_>>();
        let mapping=l.offset_mapping().map(|m|{
            let value=format!("{:?}",m.units());
            if let Some(i)=mappings.iter().position(|v|v==&value){i}else{mappings.push(value);mappings.len()-1}
        });
        json!({"accepted_text":&l.text()[l.text_range()],"range":l.text_range(),"inline":l.inline_size(),"block":l.block_size(),"offset":l.block_offset(),"metrics":format!("{:?}",l.metrics()),"writing":format!("{:?}",l.writing_mode()),"direction":format!("{:?}",l.used_direction()),"reason":format!("{:?}",l.break_reason()),"last":l.is_last(),"mapping_index":mapping,"fragments":fragments})
    }).collect::<Vec<_>>();
    let links=out.links.iter().map(|r|json!({"line":r.line,"node":r.node.0,"dom":r.dom,"text":r.text,"kind":format!("{:?}",r.kind),"element":r.link.element.0,"href":r.link.href,"rect":[r.rect.inline_start,r.rect.block_start,r.rect.inline_size,r.rect.block_size]})).collect::<Vec<_>>();
    json!({"text":out.lines.first().map_or("",|l|l.text()),"mappings":mappings,"lines":lines,"links":links})
}

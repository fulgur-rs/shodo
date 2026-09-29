//! Partition an ordinary paragraph's processed source into accepted lines and
//! generated block separators handed off by the real layout event stream.
use shodo::mapping::{Affinity, TextOrigin};
use shodo::{LineResult, Paragraph};
use std::ops::Range;

pub struct BlockRange {
    pub node: u64,
    pub range: Range<usize>,
}
pub struct Coverage {
    pub processed_bytes: usize,
    pub line_ranges: Vec<Range<usize>>,
    pub blocks: Vec<BlockRange>,
}

/// Complete IFC source coverage does not prove that separately owned blocks
/// have been laid out or painted. The caller must retain these handoffs.
pub fn audit(paragraph: &Paragraph, events: &[LineResult]) -> Result<Coverage, String> {
    let text = paragraph.text();
    let mut consumed = 0;
    let mut done = false;
    let mut result = Coverage {
        processed_bytes: text.len(),
        line_ranges: Vec::new(),
        blocks: Vec::new(),
    };
    for event in events {
        if done {
            return Err("layout event after terminal Done".into());
        }
        match event {
            LineResult::Line(line) => {
                if line.text() != text {
                    return Err("line uses a different processed text set".into());
                }
                let range = line.text_range();
                if range.start != consumed
                    || range.end < range.start
                    || !text.is_char_boundary(range.end)
                {
                    return Err(format!(
                        "line does not continue source at {consumed}: {range:?}"
                    ));
                }
                consumed = range.end;
                result.line_ranges.push(range);
            }
            LineResult::BlockInInline { node, .. } => {
                let mapping = paragraph
                    .offset_mapping()
                    .ok_or("generated block source mapping is missing")?;
                let offset =
                    u32::try_from(consumed).map_err(|_| "source exceeds mapping offset space")?;
                if !text
                    .get(consumed..)
                    .is_some_and(|suffix| suffix.starts_with('\u{2029}'))
                    || mapping.text_to_dom(offset, Affinity::Downstream)
                        != Some(TextOrigin::Generated { node: *node })
                {
                    return Err(format!(
                        "block {} has no generated separator at {consumed}",
                        node.0
                    ));
                }
                let end = consumed + '\u{2029}'.len_utf8();
                result.blocks.push(BlockRange {
                    node: node.0,
                    range: consumed..end,
                });
                consumed = end;
            }
            LineResult::Done => {
                done = true;
            }
            _ => return Err("layout trace contains an unhandled or invalid result".into()),
        }
    }
    if !done || consumed != text.len() {
        return Err(format!(
            "source coverage incomplete: {consumed}/{} bytes, terminal Done={done}",
            text.len()
        ));
    }
    Ok(result)
}

//! Lossless geometry and mark comparison across the auxiliary-metrics matrix.
#[path = "support/emphasis_geometry_input.rs"]
mod input;
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use shodo::{Fragment, Line, geometry::WritingMode};

fn marks(line: &Line) -> Value {
    json!({
        "fragments": line.fragments().filter_map(|f| match f {
            Fragment::GlyphRun(run) => Some(format!("{:?}", run.emphasis_mark())),
            _ => None,
        }).collect::<Vec<_>>(),
        "ruby": line.ruby_annotations().map(|a| marks(a.line())).collect::<Vec<_>>(),
    })
}

fn main() {
    let mut cases = Vec::new();
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for marked in [false, true] {
            for grouped in [false, true] {
                for combined in [false, true] {
                    for ruby in [false, true] {
                        let line = input::accepted(mode, marked, grouped, combined, ruby);
                        cases.push(json!({"mode":format!("{mode:?}"),"marked":marked,
                            "grouped":grouped,"combined":combined,"ruby":ruby,
                            "line":snapshot::line(&line),"marks":marks(&line)}));
                    }
                }
            }
        }
    }
    println!("{}", json!(cases));
}

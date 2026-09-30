//! Same source is installed in both historical revisions by the sibling tool.
use super::{Cursor, Summary};
use std::hint::black_box;
use std::time::Instant;

fn assert_summary(summary: Summary, cost: i64) {
    assert_eq!(summary.cost, cost);
    assert!(summary.first.is_none());
    assert!(summary.last.is_none());
    assert!(!summary.before);
    assert!(!summary.after);
    assert!(!summary.hang_before);
    assert!(!summary.hang_after);
}

#[test]
#[ignore = "manual, CPU-sensitive elapsed-time probe for shodo-8u4"]
fn measure_cursor_summary() {
    let limits = crate::limits::Limits::default();
    let style = crate::style::ParagraphStyle::default();
    let make_data = || {
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "水a",
        );
        builder
            .build(
                &mut crate::LayoutContext::new(),
                &crate::font::FontCollection::new(&limits),
            )
            .unwrap()
    };
    let context_a = make_data();
    let context_b = make_data();

    let trials = 20;
    let batch = 512;
    let repeats = 50_000;
    for depth in [1u8, 4, 127] {
        let make_cursor = || {
            let mut cursor = Cursor::default();
            for level in 0..black_box(depth) {
                cursor.push(
                    level,
                    Summary {
                        cost: i64::from(level),
                        ..Default::default()
                    },
                    None,
                );
            }
            cursor
        };
        let expected = i64::from(depth) * i64::from(depth - 1) / 2;
        for scenario in ["first", "repeat", "after_push", "context_switch"] {
            for trial in 0..trials {
                let count = if scenario == "repeat" { 1 } else { batch };
                let setup_start = Instant::now();
                let mut cursors: Vec<_> = (0..count).map(|_| make_cursor()).collect();
                match scenario {
                    "repeat" => {
                        assert_summary(cursors[0].summary(None), expected);
                    }
                    "after_push" => {
                        for cursor in &mut cursors {
                            assert_summary(cursor.summary(None), expected);
                            cursor.push(
                                depth - 1,
                                Summary {
                                    cost: 7,
                                    ..Default::default()
                                },
                                None,
                            );
                        }
                    }
                    "context_switch" => {
                        for cursor in &cursors {
                            assert_summary(cursor.summary(Some(context_a.data.as_ref())), expected);
                        }
                    }
                    _ => {}
                }
                let setup_ns = setup_start.elapsed().as_nanos();
                let iterations = if scenario == "repeat" { repeats } else { batch };
                let loop_start = Instant::now();
                if scenario == "repeat" {
                    for _ in 0..repeats {
                        black_box(0u32);
                    }
                } else {
                    for cursor in &cursors {
                        black_box(cursor.frames.len());
                    }
                }
                let loop_ns = loop_start.elapsed().as_nanos();

                let start = Instant::now();
                let mut checksum = 0i64;
                if scenario == "repeat" {
                    for _ in 0..repeats {
                        checksum += black_box(cursors[0].summary(None)).cost;
                    }
                } else {
                    for cursor in &cursors {
                        let data = if scenario == "context_switch" {
                            Some(context_b.data.as_ref())
                        } else {
                            None
                        };
                        checksum += black_box(cursor.summary(data)).cost;
                    }
                }
                let operation_ns = start.elapsed().as_nanos();
                let wanted = (expected + if scenario == "after_push" { 7 } else { 0 })
                    * i64::try_from(iterations).unwrap();
                assert_eq!(checksum, wanted, "{scenario} depth={depth}");
                for cursor in &cursors {
                    let data = if scenario == "context_switch" {
                        Some(context_b.data.as_ref())
                    } else {
                        None
                    };
                    assert_summary(
                        cursor.summary(data),
                        expected + if scenario == "after_push" { 7 } else { 0 },
                    );
                }
                println!(
                    "SHODO_8U4={}",
                    serde_json::json!({
                        "depth": depth,
                        "scenario": scenario,
                        "trial": trial,
                        "iterations": iterations,
                        "setup_ns": setup_ns,
                        "loop_ns": loop_ns,
                        "operation_ns": operation_ns,
                        "checksum": checksum,
                    })
                );
            }
        }
    }
}

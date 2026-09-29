//! Representative raikiri caller step: split the flow/paint-owned CSS that the
//! frozen S4 style gate rejects into typed per-owner handoffs.
//! Development-only; it does not adopt or change either S4 spike.
#[path = "support/raikiri_style_diffs.rs"]
#[allow(dead_code)]
mod diagnostic;
#[path = "support/style_handoff.rs"]
#[allow(dead_code)] // `solid_underline` is exercised only by tests until the caller adopts it.
mod handoff;
#[path = "support/offline_wpt.rs"]
#[allow(dead_code)]
mod offline;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, collections::BTreeSet, path::Path};

const RAIKIRI_PIN: &str = "ab7e619a8f321f03de8b8c8b9342954868e044c8";
const REASON: &str = "noninitial style not mapped yet";
const CLASSIFICATION: &str = include_str!("../data/raikiri-style-diagnostics.json");

fn error_node(error: &str) -> Result<usize, String> {
    error
        .strip_prefix("Unsupported { node: ")
        .and_then(|s| s.strip_suffix(", reason: \"noninitial style not mapped yet\" }"))
        .ok_or("unrecognized original error format")?
        .parse()
        .map_err(|_| "invalid error node".into())
}

/// The handoff must account for exactly the fields, and the same number of
/// blocks per field, that the committed classification recorded.
fn compare_counts(actual: &BTreeMap<String, usize>, expected: &Value) -> Result<(), String> {
    let expected = expected.as_object().ok_or("classification has no fields")?;
    let names: BTreeSet<_> = expected
        .keys()
        .cloned()
        .chain(actual.keys().cloned())
        .collect();
    for name in names {
        let want = expected.get(&name).and_then(|f| f["blocks"].as_u64());
        let have = actual.get(&name).map(|&n| n as u64);
        if want != have {
            return Err(format!(
                "field {name}: classified {want:?} blocks, handed off {have:?}"
            ));
        }
    }
    Ok(())
}

fn targets(case: &Value) -> Vec<&Value> {
    case["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["error"].as_str().is_some_and(|e| e.contains(REASON)))
        .collect()
}

fn replay_case(
    wpt: &Path,
    case: &Value,
    fields: &mut BTreeMap<String, usize>,
    owners: &mut BTreeMap<&'static str, usize>,
) -> Result<Vec<Value>, String> {
    let id = case["id"].as_str().ok_or("missing original case ID")?;
    let expected: Vec<offline::ResourceRecord> =
        serde_json::from_value(case["resources"].clone()).map_err(|e| e.to_string())?;
    offline::verify_original_resources(wpt, &expected)?;
    let input = offline::parse_screen(wpt, id)?;
    let mut blocks = Vec::new();
    for target in targets(case) {
        let node = error_node(target["error"].as_str().ok_or("missing original error")?)?;
        let root = target["root"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("invalid original root")?;
        let dom = &input.parsed.dom;
        if json!(
            dom.get_node(root)
                .ok_or("original root no longer exists")?
                .tag_name()
        ) != target["tag"]
        {
            return Err("original root tag changed".into());
        }
        let values = input
            .cascade
            .computed
            .get(node)
            .ok_or("missing node style")?;
        let profile = if node == root {
            diagnostic::InputProfile::MeasuredBlock
        } else if values.display == raikiri_style::property::DisplayValue::InlineBlock {
            diagnostic::InputProfile::Atomic
        } else {
            diagnostic::InputProfile::Plain
        };
        let handoff = handoff::split(values, profile).map_err(|e| format!("node {node}: {e}"))?;
        for (field, owner) in &handoff.residual {
            *fields.entry(field.clone()).or_default() += 1;
            *owners.entry(owner.name()).or_default() += 1;
        }
        blocks.push(json!({
            "root": root, "error_node": node, "input_profile": profile.name(),
            "out_of_flow": handoff.positioned.out_of_flow,
            "residual": handoff.residual.iter().map(|(f, o)| json!({"field": f, "owner": o.name()})).collect::<Vec<_>>(),
            "handed_off": {
                "flow": format!("{:?}", handoff.flow), "positioned": format!("{:?}", handoff.positioned),
                "box_paint": format!("{:?}", handoff.box_paint), "decoration": format!("{:?}", handoff.decoration),
            },
        }));
    }
    Ok(blocks)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: style_handoff <wpt-root> <original-comparison.json> <output.json>".into(),
        );
    }
    let wpt = Path::new(&args[0]);
    let source = std::fs::read(&args[1])?;
    let comparison: Value = serde_json::from_slice(&source)?;
    if comparison["raikiri_revision"] != RAIKIRI_PIN {
        return Err("comparison does not use the compiled raikiri pin".into());
    }
    let classification: Value = serde_json::from_str(CLASSIFICATION)?;
    let cases = comparison["cases"]
        .as_array()
        .ok_or("missing original cases")?;
    let selected: Vec<_> = cases.iter().filter(|c| !targets(c).is_empty()).collect();
    if selected.is_empty() {
        return Err("no original residual rejections selected".into());
    }
    let (mut fields, mut owners) = (BTreeMap::new(), BTreeMap::new());
    let (mut records, mut errors) = (Vec::new(), Vec::new());
    let mut block_count = 0;
    for case in &selected {
        let id = case["id"].as_str().ok_or("missing original case ID")?;
        match replay_case(wpt, case, &mut fields, &mut owners) {
            Ok(blocks) => {
                block_count += blocks.len();
                records.push(json!({"id": id, "blocks": blocks}));
            }
            Err(error) => errors.push(json!({"id": id, "error": error})),
        }
    }
    let counts = compare_counts(&fields, &classification["fields"]);
    let complete = errors.is_empty()
        && counts.is_ok()
        && selected.len() as u64 == classification["documents"].as_u64().unwrap_or(0)
        && block_count as u64 == classification["blocks"].as_u64().unwrap_or(0);
    let report = json!({
        "scope": "ownership split of the frozen S4 residual style gate over the original blocks; no layout, paint, WPT verdict or cutover-necessity claim",
        "raikiri_revision": RAIKIRI_PIN,
        "original_comparison_sha256": format!("{:x}", Sha256::digest(&source)),
        "documents": selected.len(), "blocks": block_count,
        "unmapped_errors": errors, "field_counts_match_classification": counts.is_ok(),
        "field_counts": fields, "owner_counts": owners,
        "complete": complete, "candidate_wpt_image_verdicts": 0, "pass_delta": null,
        "cases": records,
    });
    std::fs::write(&args[2], serde_json::to_string_pretty(&report)? + "\n")?;
    println!(
        "{} documents, {block_count} blocks, {} errors",
        selected.len(),
        errors.len()
    );
    if !complete {
        return Err(format!("handoff replay incomplete: {:?}", counts.err()).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{diagnostic::InputProfile, handoff, offline};
    use handoff::Owner;
    use raikiri_style::{ComputedValues, property as css};
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(html: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("shodo-handoff-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("index.html"), html).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Resolved style of `<div id=root>` under `css`, through the real parser and cascade.
    fn root(css: &str) -> ComputedValues {
        let html = format!("<!doctype html><style>#root{{{css}}}</style><div id=root>x</div>");
        let dir = TempDir::new(&html);
        let input = offline::parse_screen(&dir.0, "index.html").unwrap();
        let dom = &input.parsed.dom;
        let id = (0..dom.node_count())
            .find(|&i| {
                dom.get_node(i)
                    .is_some_and(|n| n.attribute("id") == Some("root"))
            })
            .unwrap();
        input.cascade.computed[id].clone()
    }

    fn split(css: &str) -> Result<handoff::Handoff, String> {
        handoff::split(&root(css), InputProfile::MeasuredBlock)
    }

    #[test]
    fn box_paint_values_are_retained_not_reset() {
        let h = split(
            "background-color:#00f;background-image:linear-gradient(red,red);\
             background-size:20px 20px;background-repeat:no-repeat;background-position:0 0;\
             outline:2px solid red;overflow:hidden",
        )
        .unwrap();
        let initial = ComputedValues::initial();
        let p = &h.box_paint;
        assert_eq!(
            (
                p.background_color.r,
                p.background_color.g,
                p.background_color.b
            ),
            (0, 0, 255)
        );
        assert_ne!(p.background_image, initial.background_image);
        assert_ne!(p.background_size, initial.background_size);
        assert_ne!(p.background_repeat, initial.background_repeat);
        assert_ne!(p.background_position, initial.background_position);
        assert_ne!(p.outline, initial.outline);
        assert_ne!(p.overflow, initial.overflow);
        for field in [
            "background_color",
            "background_image",
            "outline",
            "overflow",
        ] {
            assert!(
                h.residual
                    .iter()
                    .any(|(f, o)| f == field && *o == Owner::BoxPaint),
                "{field}"
            );
        }
    }

    #[test]
    fn outline_offset_is_retained_through_split() {
        let h = split("outline:2px solid red;outline-offset:3px").unwrap();
        assert!(
            h.residual
                .iter()
                .any(|(f, o)| f == "outline_offset" && *o == Owner::BoxPaint)
        );
        assert_ne!(
            h.box_paint.outline_offset,
            ComputedValues::initial().outline_offset
        );
    }

    #[test]
    fn extra_decoration_fields_are_owned_through_split() {
        let h = split(
            "text-decoration:underline;text-decoration-style:dotted;text-decoration-color:lime;\
             text-decoration-thickness:2px;text-underline-offset:1px",
        )
        .unwrap();
        for field in [
            "text_decoration_line",
            "text_decoration_style",
            "text_decoration_color",
            "text_decoration_thickness",
            "text_underline_offset",
        ] {
            assert!(
                h.residual
                    .iter()
                    .any(|(f, o)| f == field && *o == Owner::Decoration),
                "{field}: {:?}",
                h.residual
            );
        }
    }

    #[test]
    fn flow_values_are_retained() {
        let h = split("float:left;clear:both").unwrap();
        assert!(!h.residual.is_empty());
        assert_ne!(h.flow.float, ComputedValues::initial().float);
        assert_eq!(h.flow.clear, css::ClearValue::Both);
        assert!(h.residual.iter().all(|(_, o)| *o == Owner::Flow));
    }

    #[test]
    fn relative_position_offsets_and_z_order_are_retained() {
        let h = split("position:relative;left:5px;top:6px;z-index:-1").unwrap();
        let initial = ComputedValues::initial();
        assert_eq!(h.positioned.position, css::PositionValue::Relative);
        assert_ne!(h.positioned.left, initial.left);
        assert_ne!(h.positioned.top, initial.top);
        assert_ne!(h.positioned.z_index, initial.z_index);
        assert!(!h.positioned.out_of_flow);
    }

    #[test]
    fn absolute_is_out_of_flow_but_keeps_its_values() {
        let h = split("position:absolute;left:1px;top:2px").unwrap();
        assert_eq!(h.positioned.position, css::PositionValue::Absolute);
        assert!(h.positioned.out_of_flow);
        assert_ne!(h.positioned.left, ComputedValues::initial().left);
    }

    #[test]
    fn sibling_issue_fields_are_accounted_not_unmapped() {
        let h = split("hanging-punctuation:first;writing-mode:vertical-rl").unwrap();
        assert!(
            h.residual
                .iter()
                .any(|(f, o)| f == "hanging_punctuation" && *o == Owner::HangingPunctuation)
        );
        assert!(
            h.residual
                .iter()
                .any(|(f, o)| f == "cssom_writing_mode" && *o == Owner::VerticalText)
        );
    }

    #[test]
    fn unknown_residual_field_fails_closed() {
        let error = split("opacity:0.5").unwrap_err();
        assert!(error.starts_with("unmapped: opacity"), "{error}");
    }

    #[test]
    fn initial_style_has_no_residual() {
        let h = split("").unwrap();
        assert!(h.residual.is_empty());
        assert!(!h.positioned.out_of_flow);
    }

    #[test]
    fn every_owned_field_name_has_an_owner() {
        for field in [
            "float",
            "clear",
            "position",
            "left",
            "top",
            "z_index",
            "background_color",
            "background_image",
            "background_position",
            "background_repeat",
            "background_size",
            "outline",
            "outline_offset",
            "overflow",
            "text_decoration_line",
            "text_decoration_style",
            "text_decoration_color",
            "text_decoration_thickness",
            "text_underline_offset",
            "hanging_punctuation",
            "cssom_writing_mode",
            "text_orientation",
        ] {
            assert!(handoff::owner(field).is_some(), "{field}");
        }
        assert!(handoff::owner("opacity").is_none());
    }

    #[test]
    fn error_node_is_parsed_from_the_original_error_text() {
        let text = "Unsupported { node: 42, reason: \"noninitial style not mapped yet\" }";
        assert_eq!(super::error_node(text), Ok(42));
        assert!(super::error_node("Unsupported { node: x }").is_err());
    }

    #[test]
    fn block_counts_must_equal_the_committed_classification() {
        use std::collections::BTreeMap;
        let expected = serde_json::json!({"position": {"blocks": 2}, "float": {"blocks": 1}});
        let mut actual = BTreeMap::new();
        actual.insert("position".to_owned(), 2);
        actual.insert("float".to_owned(), 1);
        assert!(super::compare_counts(&actual, &expected).is_ok());
        actual.insert("position".to_owned(), 3);
        assert!(super::compare_counts(&actual, &expected).is_err());
        actual.insert("position".to_owned(), 2);
        actual.insert("opacity".to_owned(), 1);
        assert!(super::compare_counts(&actual, &expected).is_err());
    }

    #[test]
    fn only_solid_underline_converts() {
        let underline = handoff::solid_underline(&root(
            "text-decoration:underline;text-decoration-color:lime;text-decoration-thickness:2px",
        ))
        .unwrap()
        .expect("underline");
        assert_eq!(underline.color, Some([0, 255, 0, 255]));
        assert_eq!(underline.thickness, Some(2.0));
        assert!(handoff::solid_underline(&root("")).unwrap().is_none());
        for css in [
            "text-decoration:overline",
            "text-decoration:line-through",
            "text-decoration:underline dotted",
        ] {
            assert!(handoff::solid_underline(&root(css)).is_err(), "{css}");
        }
    }
}

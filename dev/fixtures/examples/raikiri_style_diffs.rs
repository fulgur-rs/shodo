//! Read-only diagnostics for the frozen S4 closed-world style gate.
#[path = "support/raikiri_style_diffs.rs"]
mod diagnostic;
#[path = "support/offline_wpt.rs"]
mod offline;

use raikiri_traits::{Dom, NodeId};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

const RAIKIRI_PIN: &str = "ab7e619a8f321f03de8b8c8b9342954868e044c8";
const REASON: &str = "noninitial style not mapped yet";
const PROFILE_SHA: &str = "4a225616ca97c1b83eeadff6c5228cc23c32ab36a033bb3a4f8d66ecdf4f977f";

fn targets(case: &Value) -> Vec<&Value> {
    case["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["error"].as_str().is_some_and(|e| e.contains(REASON)))
        .collect()
}

fn inspect_case(wpt: &Path, case: &Value) -> Result<Value, String> {
    let id = case["id"].as_str().ok_or("missing original case ID")?;
    let expected: Vec<offline::ResourceRecord> =
        serde_json::from_value(case["resources"].clone()).map_err(|e| e.to_string())?;
    offline::verify_original_resources(wpt, &expected)?;
    let input = offline::parse_screen(wpt, id)?;
    for resource in &input.resources {
        if !expected.contains(resource) {
            return Err(format!(
                "replayed parser resource differs from original: {}",
                resource.url
            ));
        }
    }
    let warnings: Vec<_> = input
        .parsed
        .warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .collect();
    if json!(warnings) != case["parse_warnings"] {
        return Err("original parse warning trace changed".into());
    }
    let count = input.parsed.dom.node_count();
    if input.cascade.computed.len() != count {
        return Err("cascade does not cover the original DOM".into());
    }
    let mut parents = vec![None; count];
    for parent in 0..count {
        for child in input.parsed.dom.child_ids(NodeId(parent as u64)) {
            parents[child.0 as usize] = Some(parent);
        }
    }
    let mut blocks = Vec::new();
    for target in targets(case) {
        let error = target["error"]
            .as_str()
            .ok_or("missing original Unsupported error")?;
        let node: usize = error
            .strip_prefix("Unsupported { node: ")
            .and_then(|s| s.strip_suffix(", reason: \"noninitial style not mapped yet\" }"))
            .ok_or("unrecognized original error format")?
            .parse()
            .map_err(|_| "invalid error node")?;
        let root = target["root"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("invalid original root")?;
        let root_node = input
            .parsed
            .dom
            .get_node(root)
            .ok_or("original root no longer exists")?;
        let source_node = input
            .parsed
            .dom
            .get_node(node)
            .ok_or("original error node no longer exists")?;
        if json!(root_node.tag_name()) != target["tag"] {
            return Err("original root tag changed".into());
        }
        let mut ancestors = Vec::new();
        let mut current = Some(node);
        while let Some(id) = current {
            let dom = input.parsed.dom.get_node(id).ok_or("invalid ancestor")?;
            let cv = &input.cascade.computed[id];
            ancestors.push(json!({"node":id,"tag":dom.tag_name(),"element_id":dom.attribute("id"),
                "position":format!("{:?}",cv.position),"float":format!("{:?}",cv.float),"display":format!("{:?}",cv.display)}));
            current = parents[id];
        }
        if !ancestors.iter().any(|a| a["node"] == root) {
            return Err("original error node is outside its recorded root".into());
        }
        let profile = if node == root {
            diagnostic::InputProfile::MeasuredBlock
        } else if input.cascade.computed[node].display
            == raikiri_style::property::DisplayValue::InlineBlock
        {
            diagnostic::InputProfile::Atomic
        } else {
            diagnostic::InputProfile::Plain
        };
        let values = diagnostic::prepare_input(&input.cascade.computed[node], profile);
        let differences = diagnostic::differences(&values);
        if differences.is_empty() || differences.iter().any(|d| d.field == "unreported_residual") {
            return Err(format!(
                "original residual rejection not fully reproduced at node {node}"
            ));
        }
        blocks.push(json!({"root":root,"root_tag":root_node.tag_name(),"error_node":node,
            "node_tag":source_node.tag_name(),"element_id":source_node.attribute("id"),
            "class":source_node.attribute("class"),"inline_css":source_node.attribute("style"),
            "original_error":error,"classification":"reproduced-residual-diagnostic","input_profile":profile.name(),
            "ancestors":ancestors,"differences":differences}));
    }
    Ok(
        json!({"id":id,"classification":"noninitial-style-diagnostic","blocks":blocks,
        "original_resources_verified":expected,"parser_resource_trace":input.resources,
        "parse_warnings":warnings,"original_font_sha256":case["font_sha256"],
        "dom_node_count":count,"wpt_image_verdict":null}),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: raikiri_style_diffs <wpt-root> <original-comparison.json> <output.json>".into(),
        );
    }
    let wpt = Path::new(&args[0]);
    let source = std::fs::read(&args[1])?;
    let comparison: Value = serde_json::from_slice(&source)?;
    if comparison["raikiri_revision"] != RAIKIRI_PIN {
        return Err("comparison does not use the compiled raikiri pin".into());
    }
    let cases = comparison["cases"]
        .as_array()
        .ok_or("missing original cases")?;
    let selected: Vec<_> = cases.iter().filter(|c| !targets(c).is_empty()).collect();
    let mut ids = BTreeSet::new();
    let mut records = Vec::new();
    let expected_blocks: usize = selected.iter().map(|c| targets(c).len()).sum();
    if selected.is_empty() {
        return Err("no original residual rejections selected".into());
    }
    for case in &selected {
        let id = case["id"].as_str().ok_or("missing original case ID")?;
        if !ids.insert(id) {
            return Err("duplicate original case ID".into());
        }
        let report = match inspect_case(wpt, case) {
            Ok(report) => report,
            Err(error) => json!({"id":id,"classification":"diagnostic-input-error","error":error}),
        };
        records.push(report);
    }
    let errors = records
        .iter()
        .filter(|r| r["classification"] == "diagnostic-input-error")
        .count();
    let reported_blocks: usize = records
        .iter()
        .filter_map(|r| r["blocks"].as_array())
        .map(Vec::len)
        .sum();
    let complete = errors == 0 && reported_blocks == expected_blocks;
    let report = json!({"scope":"original screen cascade and frozen S4 residual gate only; no layout/paint or WPT pass verdict",
        "raikiri_revision":RAIKIRI_PIN,"s4_style_source_sha256":PROFILE_SHA,
        "original_comparison_sha256":format!("{:x}",Sha256::digest(&source)),
        "viewport_css_px":comparison["viewport_css_px"],
        "expected_documents":selected.len(),"expected_blocks":expected_blocks,
        "reported_blocks":reported_blocks,"input_errors":errors,"complete":complete,
        "cases":records,"candidate_wpt_image_verdicts":0,"pass_delta":null});
    std::fs::write(&args[2], serde_json::to_string_pretty(&report)? + "\n")?;
    println!(
        "{} documents, {reported_blocks}/{expected_blocks} residual blocks, {errors} input errors",
        selected.len()
    );
    if !complete {
        return Err("diagnostic replay is incomplete; inspect per-document errors".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::diagnostic;
    use super::offline;
    use raikiri_html::{ParseOptions, parse_html};
    use raikiri_style::ComputedValues;

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "shodo-style-diffs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn write(&self, name: &str, bytes: &str) {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn original_linked_imports_and_screen_media_reach_the_same_dom() {
        let dir = Scratch::new();
        let html = "<link rel=stylesheet href='style/main.css'><div id=root>x</div>";
        dir.write("index.html", html);
        dir.write(
            "style/main.css",
            "@import '../import.css'; @media print {#root{background-color:red}}",
        );
        dir.write("import.css", "#root{background-color:lime}");
        let input = offline::parse_screen(&dir.0, "index.html").unwrap();
        let id = (0..input.parsed.dom.node_count())
            .find(|&id| input.parsed.dom.get_node(id).unwrap().attribute("id") == Some("root"))
            .unwrap();
        assert_eq!(input.cascade.computed[id].background_color.g, 255);
        assert_eq!(input.cascade.computed[id].background_color.r, 0);
        let plain = raikiri_html::parse(
            html.as_bytes(),
            &ParseOptions {
                extra_stylesheets: &[],
                network: None,
                base_url: None,
            },
        )
        .unwrap();
        assert_eq!(plain.dom.node_count(), input.parsed.dom.node_count());
        assert_eq!(
            plain.dom.get_node(id).unwrap().attribute("id"),
            Some("root")
        );
        assert_eq!(input.resources.len(), 3);
        assert!(
            input
                .resources
                .iter()
                .any(|r| r.url.ends_with("/import.css"))
        );
    }

    #[test]
    fn changed_original_css_is_rejected_instead_of_classified() {
        let dir = Scratch::new();
        dir.write(
            "index.html",
            "<link rel=stylesheet href='style.css'><div id=root>x</div>",
        );
        dir.write("style.css", "#root{color:blue}");
        let original = offline::parse_screen(&dir.0, "index.html")
            .unwrap()
            .resources;
        assert!(!original.is_empty());
        offline::verify_original_resources(&dir.0, &original).unwrap();
        dir.write("style.css", "#root{color:red}");
        assert!(offline::verify_original_resources(&dir.0, &original).is_err());
    }

    #[test]
    fn failed_import_is_an_input_error_not_an_initial_style() {
        let dir = Scratch::new();
        dir.write(
            "index.html",
            "<link rel=stylesheet href='style.css'><div id=root>x</div>",
        );
        dir.write("style.css", "@import 'missing.css'; #root{color:blue}");
        assert!(offline::parse_screen(&dir.0, "index.html").is_err());
    }

    #[test]
    fn original_error_node_is_diagnosed_and_false_reproduction_is_rejected() {
        let dir = Scratch::new();
        dir.write("index.html", "<style>#child{background-color:lime}</style><div id=root><span id=child>x</span></div>");
        let input = offline::parse_screen(&dir.0, "index.html").unwrap();
        let find = |name| {
            (0..input.parsed.dom.node_count())
                .find(|&id| input.parsed.dom.get_node(id).unwrap().attribute("id") == Some(name))
                .unwrap()
        };
        let root = find("root");
        let node = find("child");
        let error = |node| {
            format!("Unsupported {{ node: {node}, reason: \"noninitial style not mapped yet\" }}")
        };
        let mut case = serde_json::json!({"id":"index.html", "resources": input.resources,
            "parse_warnings": input.parsed.warnings.iter().map(|w|format!("{w:?}")).collect::<Vec<_>>(),
            "font_sha256": [], "blocks":[{"classification":"unsupported-projection", "root":root, "tag":"div", "error":error(node)}]});
        let report = super::inspect_case(&dir.0, &case).unwrap();
        assert_eq!(report["blocks"][0]["root"], root);
        assert_eq!(report["blocks"][0]["error_node"], node);
        assert_eq!(report["blocks"][0]["node_tag"], "span");
        assert_eq!(
            report["blocks"][0]["differences"][0]["field"],
            "background_color"
        );
        case["blocks"][0]["error"] = error(root).into();
        assert!(
            super::inspect_case(&dir.0, &case).is_err(),
            "known residual rejection must not be cleared by an empty result"
        );
    }

    #[test]
    fn measured_root_and_atomic_dimensions_are_removed_before_residual_diagnosis() {
        let dir = Scratch::new();
        dir.write("index.html", "<style>#root{width:100px;height:50px;background-color:red}#child{display:inline-block;width:20px;height:10px;background-color:blue}</style><div id=root><span id=child>x</span></div>");
        let input = offline::parse_screen(&dir.0, "index.html").unwrap();
        let find = |name| {
            (0..input.parsed.dom.node_count())
                .find(|&id| input.parsed.dom.get_node(id).unwrap().attribute("id") == Some(name))
                .unwrap()
        };
        let root = find("root");
        let child = find("child");
        let block = |node| serde_json::json!({"classification":"unsupported-projection", "root":root,"tag":"div", "error":format!("Unsupported {{ node: {node}, reason: \"noninitial style not mapped yet\" }}")});
        let case = serde_json::json!({"id":"index.html", "resources":input.resources,
            "parse_warnings":input.parsed.warnings.iter().map(|w|format!("{w:?}")).collect::<Vec<_>>(),"font_sha256":[],"blocks":[block(root),block(child)]});
        let report = super::inspect_case(&dir.0, &case).unwrap();
        for block in report["blocks"].as_array().unwrap() {
            let fields: Vec<_> = block["differences"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["field"].as_str().unwrap())
                .collect();
            assert_eq!(fields, ["background_color"]);
        }
        assert_eq!(report["blocks"][0]["input_profile"], "measured-block");
        assert_eq!(report["blocks"][1]["input_profile"], "atomic");
    }

    fn values(css: &str, element: &str) -> ComputedValues {
        let doc = parse_html(
            format!("<style>{css}</style><div id=root><span id=child>x</span></div>").as_bytes(),
            &ParseOptions {
                extra_stylesheets: &[],
                network: None,
                base_url: None,
            },
        )
        .unwrap();
        let id = (0..doc.dom().node_count())
            .find(|&id| doc.dom().get_node(id).unwrap().attribute("id") == Some(element))
            .unwrap();
        doc.cascade().computed[id].clone()
    }

    #[test]
    fn mapped_font_spacing_and_edges_do_not_become_residual_findings() {
        let cv = values(
            "#root{font-family:monospace;font-size:32px;color:red;word-spacing:4px;line-height:2;margin:5px;padding:3px;border:2px solid blue}",
            "root",
        );
        assert!(diagnostic::differences(&cv).is_empty());
    }

    #[test]
    fn actual_descendant_has_all_unmapped_fields_with_values() {
        let css = "#child{background-color:lime;text-decoration-line:underline;position:relative}";
        assert!(diagnostic::differences(&values(css, "root")).is_empty());
        let diffs = diagnostic::differences(&values(css, "child"));
        let background = diffs
            .iter()
            .find(|d| d.field == "background_color")
            .unwrap();
        assert!(background.value.contains("g: 255"));
        assert!(background.initial.contains("a: 0"));
        assert!(
            diffs
                .iter()
                .any(|d| d.field == "text_decoration_line" && d.value.contains("underline: true"))
        );
        assert!(
            diffs
                .iter()
                .any(|d| d.field == "position" && d.value == "Relative")
        );
        assert!(!diffs.iter().any(|d| d.field == "display"));
    }

    #[test]
    fn private_custom_property_environment_is_not_silently_ignored() {
        let diffs = diagnostic::differences(&values("#child{--marker:lime}", "child"));
        assert!(
            diffs
                .iter()
                .any(|d| d.field == "custom_properties" && d.value.contains("--marker"))
        );
        assert!(diffs.iter().any(|d| d.field == "local_custom_properties"));
    }
}

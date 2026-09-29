//! Representative raikiri caller step: split the flow/paint-owned CSS that the
//! frozen S4 style gate rejects into typed per-owner handoffs.
//! Development-only; it does not adopt or change either S4 spike.
#[path = "support/raikiri_style_diffs.rs"]
#[allow(dead_code)]
mod diagnostic;
#[path = "support/offline_wpt.rs"]
#[allow(dead_code)]
mod offline;
#[path = "support/style_handoff.rs"]
#[allow(dead_code)]
mod handoff;

fn main() {
    eprintln!("Run `cargo test -p shodo-raikiri --example style_handoff`.");
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
            .find(|&i| dom.get_node(i).is_some_and(|n| n.attribute("id") == Some("root")))
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
        assert_eq!((p.background_color.r, p.background_color.g, p.background_color.b), (0, 0, 255));
        assert_ne!(p.background_image, initial.background_image);
        assert_ne!(p.background_size, initial.background_size);
        assert_ne!(p.background_repeat, initial.background_repeat);
        assert_ne!(p.background_position, initial.background_position);
        assert_ne!(p.outline, initial.outline);
        assert_ne!(p.overflow, initial.overflow);
        for field in ["background_color", "background_image", "outline", "overflow"] {
            assert!(h.residual.iter().any(|(f, o)| f == field && *o == Owner::BoxPaint), "{field}");
        }
    }

    #[test]
    fn flow_values_are_retained() {
        let h = split("float:left;clear:both").unwrap();
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
        assert!(h.residual.iter().any(|(f, o)| f == "hanging_punctuation" && *o == Owner::HangingPunctuation));
        assert!(h.residual.iter().any(|(f, o)| f == "cssom_writing_mode" && *o == Owner::VerticalText));
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
            "float", "clear", "position", "left", "top", "z_index", "background_color",
            "background_image", "background_position", "background_repeat", "background_size",
            "outline", "outline_offset", "overflow", "text_decoration_line",
            "text_decoration_style", "text_decoration_color", "text_decoration_thickness",
            "text_underline_offset", "hanging_punctuation", "cssom_writing_mode", "text_orientation",
        ] {
            assert!(handoff::owner(field).is_some(), "{field}");
        }
        assert!(handoff::owner("opacity").is_none());
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

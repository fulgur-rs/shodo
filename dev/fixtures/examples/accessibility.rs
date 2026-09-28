//! Read-only AccessKit text/geometry and selection-action integration.
//! Run with --features accesskit; platform adapter/event loop are caller-owned.
use shodo::accessibility::accesskit::{AccessKitAdapter, NodeSemantics, types as ak};
use shodo::accessibility::{AccessibleLayout, AccessibleSelection, SourcePosition};
use shodo::geometry::PhysicalRect;
use shodo::mapping::Affinity;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, WhiteSpaceCollapse};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

const ROOT: ak::NodeId = ak::NodeId(1);

struct Report {
    text: String,
    selected: String,
    bounds: Vec<ak::Rect>,
    anchor: SourcePosition,
    focus: SourcePosition,
}

fn root() -> ak::Node {
    let mut node = ak::Node::new(ak::Role::Document);
    node.set_read_only();
    node.add_action(ak::Action::SetTextSelection);
    node
}

fn demonstrate() -> Result<Report, Box<dyn std::error::Error>> {
    let lines = {
        let limits = Default::default();
        let fonts = load_fonts(&limits)?;
        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: FONTS
                    .iter()
                    .map(|f| FontFamily::Named(f.family.into()))
                    .collect(),
                font_size: 16.0,
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(7),
                offset: 0,
            },
            "ffi سلام 👩‍💻\n次の行",
        );
        builder
            .build(&mut LayoutContext::new(), &fonts.collection)?
            .break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                160.0,
                &AtomicSizes::EMPTY,
            )
    };
    let layout = AccessibleLayout::new(&lines);
    let mut adapter = AccessKitAdapter::new(ROOT);
    let frame = PhysicalRect {
        x: 50.0,
        y: 30.0,
        width: 200.0,
        height: 160.0,
    };
    let mut next_id = 9;
    let mut allocate = || {
        next_id += 1;
        ak::NodeId(next_id)
    };
    let initial = adapter.update(
        &layout,
        root(),
        frame,
        None,
        |_| NodeSemantics::default(),
        &mut allocate,
    )?;
    let mut consumer = accesskit_consumer::Tree::new(initial, true);
    let request = {
        // A real consumer supplies positions. A host platform adapter delivers
        // the same action payload from assistive technology.
        let state = consumer.state();
        let document = state.root();
        ak::ActionRequest {
            action: ak::Action::SetTextSelection,
            target_tree: ak::TreeId::ROOT,
            target_node: ROOT,
            data: Some(ak::ActionData::SetTextSelection(ak::TextSelection {
                anchor: document
                    .text_position_from_global_usv_index(1)
                    .ok_or("missing anchor")?
                    .to_raw(),
                focus: document
                    .text_position_from_global_usv_index(3)
                    .ok_or("missing focus")?
                    .to_raw(),
            })),
        }
    };
    let selection = route_selection(&adapter, request)?;
    let anchor = layout
        .to_source(selection.anchor)
        .ok_or("source mapping disabled")?;
    let focus = layout
        .to_source(selection.focus)
        .ok_or("source mapping disabled")?;
    let update = adapter.update(
        &layout,
        root(),
        frame,
        Some(selection),
        |_| NodeSemantics::default(),
        &mut allocate,
    )?;
    consumer.update_and_process_changes(update, &mut Events);
    let state = consumer.state();
    let document = state.root();
    let selected = document.text_selection().ok_or("selection missing")?;
    Ok(Report {
        text: document.document_range().text(),
        selected: selected.text(),
        bounds: selected.bounding_boxes(),
        anchor,
        focus,
    })
}

fn route_selection(
    adapter: &AccessKitAdapter,
    request: ak::ActionRequest,
) -> Result<AccessibleSelection, Box<dyn std::error::Error>> {
    if request.action != ak::Action::SetTextSelection
        || request.target_tree != ak::TreeId::ROOT
        || request.target_node != ROOT
    {
        return Err("unexpected action target".into());
    }
    let Some(ak::ActionData::SetTextSelection(selection)) = request.data else {
        return Err("missing selection payload".into());
    };
    // AccessKit positions do not encode affinity; the host chooses its policy.
    Ok(AccessibleSelection {
        anchor: adapter
            .from_position(selection.anchor, Affinity::Downstream)
            .ok_or("stale anchor")?,
        focus: adapter
            .from_position(selection.focus, Affinity::Upstream)
            .ok_or("stale focus")?,
    })
}

// An application forwards these changes to its platform accessibility adapter.
struct Events;
impl accesskit_consumer::TreeChangeHandler for Events {
    fn node_added(&mut self, _node: &accesskit_consumer::Node) {}
    fn node_updated(&mut self, _old: &accesskit_consumer::Node, _new: &accesskit_consumer::Node) {}
    fn focus_moved(
        &mut self,
        _old: Option<&accesskit_consumer::Node>,
        _new: Option<&accesskit_consumer::Node>,
    ) {
    }
    fn node_removed(&mut self, _node: &accesskit_consumer::Node) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = demonstrate()?;
    println!("text: {}", report.text);
    println!("selected: {}", report.selected);
    println!("anchor: {:?}, focus: {:?}", report.anchor, report.focus);
    println!("physical selection bounds: {:?}", report.bounds);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shodo::mapping::TextOrigin;

    #[test]
    fn read_only_example_routes_selection_actions_to_sources() {
        let report = demonstrate().unwrap();
        assert_eq!(report.text, "ffi سلام 👩‍💻\n次の行");
        assert_eq!(report.selected, "fi");
        assert_eq!(
            report.anchor.origin,
            TextOrigin::Dom {
                node: NodeId(7),
                offset: 1
            }
        );
        assert_eq!(
            report.focus.origin,
            TextOrigin::Dom {
                node: NodeId(7),
                offset: 3
            }
        );
        assert_eq!(report.bounds.len(), 1);
        assert!((report.bounds[0].x0 - 55.04).abs() < 0.025);
        assert!((report.bounds[0].x1 - 65.136).abs() < 0.025);
    }
}

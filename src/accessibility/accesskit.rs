//! Optional AccessKit data and position bridge. Platform integration is caller-owned.
use super::{AccessibleLayout, AccessiblePosition, AccessibleSelection};
use crate::geometry::PhysicalRect;
use crate::mapping::Affinity;
use crate::node::NodeId;
pub use ::accesskit as types;
use std::collections::HashMap;
mod nodes;
mod positions;
use nodes::{Key, NodeBuilder};
use positions::PositionState;

#[derive(Clone, Debug)]
pub struct NodeSemantics {
    pub role: types::Role,
    pub label: Option<String>,
    pub description: Option<String>,
}
impl Default for NodeSemantics {
    fn default() -> Self {
        Self {
            role: types::Role::GenericContainer,
            label: None,
            description: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKitError {
    CharacterTooLong,
    InvalidFrame,
    InvalidSelection,
    DuplicateNodeId,
}
impl std::fmt::Display for AccessKitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for AccessKitError {}
/// Persistent source-start identities and position tables for the last
/// successful export. Allocation and host/platform events belong to callers.
pub struct AccessKitAdapter {
    root: types::NodeId,
    registry: HashMap<Key, types::NodeId>,
    positions: PositionState,
}
impl AccessKitAdapter {
    pub fn new(root: types::NodeId) -> Self {
        Self {
            root,
            registry: HashMap::new(),
            positions: PositionState::default(),
        }
    }
    /// Return a complete standalone tree update. Errors leave the adapter's
    /// previous tree/position state unchanged. Allocators must never reuse IDs.
    /// For host embedding, adjust root/parent/focus/tree metadata before delivery.
    pub fn update(
        &mut self,
        layout: &AccessibleLayout<'_>,
        mut root_node: types::Node,
        frame: PhysicalRect,
        selection: Option<AccessibleSelection>,
        semantics: impl Fn(NodeId) -> NodeSemantics,
        allocate_id: impl FnMut() -> types::NodeId,
    ) -> Result<types::TreeUpdate, AccessKitError> {
        if ![frame.x, frame.y, frame.width, frame.height]
            .iter()
            .all(|v| v.is_finite())
            || frame.width < 0.0
            || frame.height < 0.0
        {
            return Err(AccessKitError::InvalidFrame);
        }
        if let Some(s) = selection
            && (layout.to_text_position(s.anchor).is_none()
                || layout.to_text_position(s.focus).is_none())
        {
            return Err(AccessKitError::InvalidSelection);
        }
        let mut builder = NodeBuilder::new(self.root, &self.registry, allocate_id);
        builder.build(layout, frame, semantics)?;
        let positions = PositionState::new(layout, builder.spans);
        if let Some(s) = selection {
            root_node.set_text_selection(
                positions
                    .to_selection(s)
                    .ok_or(AccessKitError::InvalidSelection)?,
            );
        } else {
            root_node.clear_text_selection();
        }
        root_node.set_children(builder.children);
        root_node.set_bounds(types::Rect::new(
            f64::from(frame.x),
            f64::from(frame.y),
            f64::from(frame.x) + f64::from(frame.width),
            f64::from(frame.y) + f64::from(frame.height),
        ));
        builder.nodes.push((self.root, root_node));
        let registry = builder.registry;
        let nodes = builder.nodes;
        self.registry = registry;
        self.positions = positions;
        Ok(types::TreeUpdate {
            nodes,
            tree: Some(types::Tree::new(self.root)),
            tree_id: types::TreeId::ROOT,
            focus: self.root,
        })
    }
    /// Convert a caret, normalizing a hard line's end to the break's beginning.
    /// `update` preserves after-break endpoints in nonempty selections instead.
    pub fn to_position(&self, position: AccessiblePosition) -> Option<types::TextPosition> {
        self.positions.to_position(position)
    }
    /// Resolve an exact SDK endpoint, including the end after a hard break.
    pub fn from_position(
        &self,
        position: types::TextPosition,
        affinity: Affinity,
    ) -> Option<AccessiblePosition> {
        self.positions.resolve(position, affinity)
    }
}

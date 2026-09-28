use crate::node::NodeId;
use crate::{Line, Paragraph, RubyVisibility};
use std::ops::Range;

/// Affine map from annotation-line logical coordinates to parent-line logical
/// coordinates. The two offsets are relative to the parent line's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RubyTransform {
    pub inline_inline: f32,
    pub inline_block: f32,
    pub block_inline: f32,
    pub block_block: f32,
    pub inline_offset: f32,
    pub block_offset: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct RubyAnnotationRecord {
    pub(crate) container: NodeId,
    pub(crate) base_nodes: Vec<NodeId>,
    pub(crate) node: Option<NodeId>,
    pub(crate) level: usize,
    pub(crate) base_text: Range<usize>,
    pub(crate) paragraph: Paragraph,
    pub(crate) line: Line,
    pub(crate) visibility: RubyVisibility,
    pub(crate) transform: RubyTransform,
}

/// A retained annotation fragment, including its own source and font owners.
#[derive(Clone, Copy, Debug)]
pub struct RubyAnnotationView<'a> {
    pub(super) record: &'a RubyAnnotationRecord,
}

impl<'a> RubyAnnotationView<'a> {
    pub fn container(&self) -> NodeId {
        self.record.container
    }
    pub fn base_nodes(&self) -> &'a [NodeId] {
        &self.record.base_nodes
    }
    pub fn node(&self) -> Option<NodeId> {
        self.record.node
    }
    pub fn level(&self) -> usize {
        self.record.level
    }
    pub fn base_text_range(&self) -> Range<usize> {
        self.record.base_text.clone()
    }
    pub fn text_range(&self) -> Range<usize> {
        self.record.line.text_range()
    }
    pub fn origin(&self) -> (f32, f32) {
        (
            self.record.transform.inline_offset,
            self.record.transform.block_offset,
        )
    }
    pub fn transform(&self) -> RubyTransform {
        self.record.transform
    }
    pub fn line(&self) -> &'a Line {
        &self.record.line
    }
    pub fn paragraph(&self) -> &'a Paragraph {
        &self.record.paragraph
    }
    pub fn visibility(&self) -> RubyVisibility {
        self.record.visibility
    }
}

impl Line {
    pub fn ruby_annotations(&self) -> impl ExactSizeIterator<Item = RubyAnnotationView<'_>> {
        self.ruby.iter().map(|record| RubyAnnotationView { record })
    }
}

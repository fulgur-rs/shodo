//! Development-owned caller state; inputs change only by replacement.
use super::caller::{self, FontPolicy, Output, PreparedParagraph, ResolvedInput};
use shodo::{
    AtomicSizes, BreakToken, LayoutContext, LineConstraint, LineResult, font::FontCollection,
};

type FontStamp = (u32, u64, Option<(u32, u64)>);

pub struct PagedOutput {
    pub pages: Vec<Output>,
    pub page_tokens: Vec<BreakToken>,
    pub height_retries: usize,
    pub oversize_lines: usize,
}

pub struct RetainedCaller {
    input: ResolvedInput,
    shared: FontCollection,
    document: Option<FontCollection>,
    policy: FontPolicy,
    context: LayoutContext,
    prepared: Option<PreparedParagraph>,
    stamp: Option<FontStamp>,
}
impl RetainedCaller {
    /// `document`, when present, must be a layer created from `shared`.
    /// The caller owns that binding and both live registration generations.
    pub fn new(
        input: ResolvedInput,
        shared: FontCollection,
        document: Option<FontCollection>,
        policy: FontPolicy,
    ) -> Self {
        Self {
            input,
            shared,
            document,
            policy,
            context: LayoutContext::new(),
            prepared: None,
            stamp: None,
        }
    }
    pub fn paragraph_id(&self) -> Option<u64> {
        self.prepared.as_ref().map(|p| p.paragraph.id())
    }
    pub fn replace_input(&mut self, input: ResolvedInput) {
        self.input = input;
        self.prepared = None;
        self.stamp = None;
    }
    pub fn replace_fonts(&mut self, shared: FontCollection, document: Option<FontCollection>) {
        self.shared = shared;
        self.document = document;
        self.prepared = None;
        self.stamp = None;
    }
    pub fn start_token(&mut self) -> Result<BreakToken, String> {
        self.ensure_prepared()?;
        Ok(self.prepared.as_ref().unwrap().paragraph.start_token())
    }
    pub fn next_line(
        &mut self,
        token: BreakToken,
        constraint: &LineConstraint<'_>,
    ) -> Result<LineResult, String> {
        self.ensure_prepared()?;
        Ok(self.prepared.as_ref().unwrap().paragraph.next_line(
            &mut self.context,
            token,
            &Default::default(),
            constraint,
            &AtomicSizes::EMPTY,
        ))
    }
    fn font_stamp(&self) -> FontStamp {
        (
            self.shared.layer_handle().id(),
            self.shared.generation(),
            self.document
                .as_ref()
                .map(|d| (d.layer_handle().id(), d.generation())),
        )
    }
    fn ensure_prepared(&mut self) -> Result<(), String> {
        let stamp = self.font_stamp();
        if self.prepared.is_some() && self.stamp == Some(stamp) {
            return Ok(());
        }
        // Never expose the previous paragraph if rebuilding fails.
        self.prepared = None;
        self.stamp = None;
        let prepared = caller::prepare(
            &self.input,
            &mut self.context,
            self.document.as_ref().unwrap_or(&self.shared),
            self.policy,
        )?;
        let expected = match stamp.2 {
            Some((_, generation)) => (stamp.1, Some(generation)),
            None => (stamp.1, None),
        };
        if prepared.paragraph.font_generations() != expected || self.font_stamp() != stamp {
            return Err("font generations changed during preparation, or document/shared binding is inconsistent".into());
        }
        self.prepared = Some(prepared);
        self.stamp = Some(stamp);
        Ok(())
    }
    /// Fill finite pages; an over-tall first line is accepted without the height
    /// limit so that the original token advances. This caller supports its
    /// existing inline-only DOM projection, not external floats/block children.
    pub fn layout_height(&mut self, width: f32, height: f32) -> Result<PagedOutput, String> {
        self.ensure_prepared()?;
        paginate(
            self.prepared.as_ref().unwrap(),
            &mut self.context,
            width,
            height,
        )
    }
    pub fn paragraph_warnings(&self) -> &[shodo::limits::Warning] {
        self.prepared
            .as_ref()
            .map_or(&[], |p| p.paragraph.warnings())
    }
    pub fn take_warnings(&mut self) -> Vec<shodo::limits::Warning> {
        self.context.take_warnings()
    }
    pub fn layout_width(&mut self, width: f32) -> Result<Output, String> {
        self.ensure_prepared()?;
        let p = self.prepared.as_ref().unwrap();
        let lines = p.paragraph.break_all(
            &mut self.context,
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        p.output(lines)
    }
}

/// Shared height caller for fresh, reused-context and retained-paragraph checks.
pub fn paginate(
    p: &PreparedParagraph,
    context: &mut LayoutContext,
    width: f32,
    height: f32,
) -> Result<PagedOutput, String> {
    if !width.is_finite() || width <= 0.0 || !height.is_finite() || height < 0.0 {
        return Err("page width must be finite/positive and height finite/nonnegative".into());
    }
    let mut token = p.paragraph.start_token();
    let mut pages = Vec::new();
    let mut page_tokens = Vec::new();
    let mut lines = Vec::new();
    let mut offset = 0.0_f32;
    let mut unbounded_retry = false;
    let mut height_retries = 0;
    let mut oversize_lines = 0;
    loop {
        let mut constraint = LineConstraint::new(width);
        constraint.block_offset = offset;
        constraint.max_block_size = if unbounded_retry {
            None
        } else {
            Some((height - offset).max(0.0))
        };
        match p.paragraph.next_line(
            context,
            token,
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                if unbounded_retry {
                    oversize_lines += 1;
                }
                token = line.break_token();
                // The core line iterator accumulates accepted Q26.6 heights.
                offset = ((offset * 64.0).round() + (line.block_size() * 64.0).round()) / 64.0;
                lines.push(line);
                unbounded_retry = false;
            }
            LineResult::BlockSizeExceeded { .. } => {
                height_retries += 1;
                if lines.is_empty() {
                    unbounded_retry = true;
                } else {
                    pages.push(p.output(std::mem::take(&mut lines))?);
                    page_tokens.push(token);
                    offset = 0.0;
                }
                // No accepted line: retry exactly the same token.
            }
            LineResult::Done => {
                if !lines.is_empty() {
                    pages.push(p.output(lines)?);
                    page_tokens.push(token);
                }
                break;
            }
            other => {
                return Err(format!(
                    "unsupported retained caller line result: {other:?}"
                ));
            }
        }
    }
    Ok(PagedOutput {
        pages,
        page_tokens,
        height_retries,
        oversize_lines,
    })
}

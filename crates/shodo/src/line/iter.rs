use crate::style::LineOptions;
use crate::{AtomicSizes, BreakToken, LayoutContext, Line, LineConstraint, LineResult, Paragraph};

impl Paragraph {
    /// Greedy layout at a fixed width, treating floats as zero-width anchors.
    /// Blocks split lines but do not contribute a block extent here.
    pub fn break_all(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        atomics: &AtomicSizes,
    ) -> Vec<Line> {
        self.break_all_with_optional_grapheme_limit(cx, options, width, None, atomics)
    }

    /// Layout by a per-line processed Unicode grapheme count. Normal line
    /// break opportunities, forced breaks, and `width` do not end a line;
    /// a newline and an atomic inline each count as one. A zero limit accepts
    /// one indivisible unit to guarantee progress. `width` still sets the
    /// line box for alignment. Floats are treated as zero-width anchors, as
    /// in [`Self::break_all`]. For caller-managed floats and height limits,
    /// set [`LineConstraint::max_graphemes`] and use [`Self::next_line`].
    pub fn break_all_with_grapheme_limit(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        max_graphemes: usize,
        atomics: &AtomicSizes,
    ) -> Vec<Line> {
        self.break_all_with_optional_grapheme_limit(
            cx,
            options,
            width,
            Some(max_graphemes),
            atomics,
        )
    }

    fn break_all_with_optional_grapheme_limit(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        max_graphemes: Option<usize>,
        atomics: &AtomicSizes,
    ) -> Vec<Line> {
        let mut cursor = None;
        self.lines_with_previous::<false, _>(
            cx,
            self.start_token(),
            options,
            |previous, offset| {
                if let Some(LineResult::FloatEncountered { float_cursor, .. }) = previous {
                    cursor = Some(*float_cursor);
                }
                let mut c = LineConstraint::new(width);
                c.block_offset = offset;
                c.floats_placed_through = cursor;
                c.max_graphemes = max_graphemes;
                c
            },
            atomics,
        )
        .filter_map(|r| match r {
            LineResult::Line(l) => Some(l),
            _ => None,
        })
        .collect()
    }

    /// Iterates accepted lines and block boundaries, ending with `Done` (or
    /// `InvalidToken`) once. The callback receives the previous result and
    /// accumulated line height, initially `None` and zero.
    ///
    /// Float and height results are passed to the callback, not yielded. It
    /// must update the constraint to make progress. In particular, retain
    /// the paragraph's float cursor, roll back external state on withdrawal,
    /// and retry an over-tall first-page line with no height limit. A callback
    /// that repeats an unsatisfiable constraint retries indefinitely.
    pub fn lines<'p, 'cx, F>(
        &'p self,
        cx: &'cx mut LayoutContext,
        token: BreakToken,
        options: &LineOptions,
        constraint_fn: F,
        atomics: &'p AtomicSizes,
    ) -> impl Iterator<Item = LineResult> + 'cx
    where
        'p: 'cx,
        F: FnMut(Option<&LineResult>, f32) -> LineConstraint<'p> + 'cx,
    {
        self.lines_with_previous::<true, _>(cx, token, options, constraint_fn, atomics)
    }

    // Internal fixed-width drivers only inspect float results and the block
    // offset. Keep their state machine identical without copying accepted Lines.
    fn lines_with_previous<'p, 'cx, const KEEP_PREVIOUS_LINE: bool, F>(
        &'p self,
        cx: &'cx mut LayoutContext,
        token: BreakToken,
        options: &LineOptions,
        constraint_fn: F,
        atomics: &'p AtomicSizes,
    ) -> impl Iterator<Item = LineResult> + 'cx
    where
        'p: 'cx,
        F: FnMut(Option<&LineResult>, f32) -> LineConstraint<'p> + 'cx,
    {
        Lines::<_, KEEP_PREVIOUS_LINE> {
            para: self,
            cx,
            token,
            options: *options,
            constraint_fn,
            atomics,
            previous: None,
            offset: 0.0,
            done: false,
        }
    }
}

struct Lines<'p, 'cx, F, const KEEP_PREVIOUS_LINE: bool> {
    para: &'p Paragraph,
    cx: &'cx mut LayoutContext,
    token: BreakToken,
    options: LineOptions,
    constraint_fn: F,
    atomics: &'p AtomicSizes,
    previous: Option<LineResult>,
    offset: f32,
    done: bool,
}

impl<'p, F: FnMut(Option<&LineResult>, f32) -> LineConstraint<'p>, const KEEP_PREVIOUS_LINE: bool>
    Iterator for Lines<'p, '_, F, KEEP_PREVIOUS_LINE>
{
    type Item = LineResult;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            let c = (self.constraint_fn)(self.previous.as_ref(), self.offset);
            let result = self
                .para
                .next_line(self.cx, self.token, &self.options, &c, self.atomics);
            match &result {
                LineResult::FloatEncountered { .. } | LineResult::BlockSizeExceeded { .. } => {
                    self.previous = Some(result);
                    continue;
                }
                LineResult::Line(l) => {
                    self.token = l.break_token();
                    let mut sat = crate::geometry::Saturation::default();
                    self.offset =
                        crate::geometry::LayoutUnit::from_f32_round(self.offset, &mut sat)
                            .add(
                                crate::geometry::LayoutUnit::from_f32_round(
                                    l.block_size(),
                                    &mut sat,
                                ),
                                &mut sat,
                            )
                            .to_f32();
                    self.cx.warnings.record_saturation(&sat);
                    self.previous = KEEP_PREVIOUS_LINE.then(|| LineResult::Line(l.clone()));
                }
                LineResult::BlockInInline { node, token_after } => {
                    self.token = *token_after;
                    self.previous = Some(LineResult::BlockInInline {
                        node: *node,
                        token_after: *token_after,
                    });
                }
                LineResult::Done | LineResult::InvalidToken => {
                    self.done = true;
                    self.previous = None;
                }
            }
            return Some(result);
        }
    }
}

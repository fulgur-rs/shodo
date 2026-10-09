use super::cache::Retention;
use crate::line::fragments::RecordKind;
use crate::style::{BoxDecorationBreak, LineOptions};
use crate::{AtomicSizes, BreakToken, LayoutContext, Line, LineConstraint, LineResult, Paragraph};

#[cfg(test)]
mod summary_tests;

impl Paragraph {
    /// Greedy layout at a fixed width, treating floats as zero-width anchors.
    /// Blocks split lines but do not contribute a block extent here.
    ///
    /// Use [`Self::lines`] or [`Self::next_line`] for changing widths, height
    /// constraints or actual float placement. This method collects accepted
    /// lines into a `Vec`; it does not apply a [`crate::BreakPlan`].
    ///
    /// # Examples
    ///
    /// ```
    /// use shodo::font::{FontCollection, FontOptions};
    /// use shodo::limits::Limits;
    /// use shodo::style::{LineOptions, ParagraphStyle};
    /// use shodo::{AtomicSizes, LayoutContext, RichText};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let style = ParagraphStyle::default();
    /// let fonts = FontCollection::with_options(&Limits::default(), FontOptions {
    ///     system_fonts: false, ..Default::default()
    /// });
    /// let mut cx = LayoutContext::new();
    /// let paragraph = RichText::new(&style)
    ///     .push("Hello world", &style.root)
    ///     .build(&mut cx, &fonts)?;
    /// let lines = paragraph.break_all(
    ///     &mut cx, &LineOptions::default(), 80.0, &AtomicSizes::EMPTY,
    /// );
    /// assert!(!lines.is_empty());
    /// for line in &lines {
    ///     println!("{}", &line.text()[line.text_range()]);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn break_all(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        atomics: &AtomicSizes,
    ) -> Vec<Line> {
        self.break_all_with_optional_grapheme_limit(cx, options, width, None, atomics)
    }

    /// Maximum advance of the lines produced by greedy layout at `width`.
    ///
    /// A line's advance is [`Line::inline_size`] minus [`Line::hang_start`]
    /// plus [`Line::hang_end`], so trailing hanging whitespace and punctuation
    /// are included. Text indent, inline-start offsets, and leading hanging
    /// punctuation are not included. Floats are treated as zero-width anchors,
    /// as in [`Self::break_all`].
    /// Returns zero when layout produces no lines. Unlike [`Self::break_all`],
    /// this method does not collect the lines into a `Vec`.
    pub fn max_inline_size(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        atomics: &AtomicSizes,
    ) -> f32 {
        self.fixed_width_lines(cx, options, width, None, atomics, Retention::LastLine)
            .map(|line| line.inline_size() - line.hang_start() + line.hang_end())
            .reduce(f32::max)
            .unwrap_or(0.0)
    }

    /// Advance of the first line produced by greedy layout at `width`.
    ///
    /// The returned value is [`Line::inline_size`] minus [`Line::hang_start`]
    /// plus [`Line::hang_end`]; text indent, inline-start offsets, and leading
    /// hanging punctuation are not included. Returns zero when layout produces
    /// no lines. Only the first line is laid out; later lines and their warnings
    /// are not observed.
    pub fn first_line_advance(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        atomics: &AtomicSizes,
    ) -> f32 {
        // This iterator stops after one line; its token can still be retried.
        self.fixed_width_lines(cx, options, width, None, atomics, Retention::AllLines)
            .next()
            .map(|line| line.inline_size() - line.hang_start() + line.hang_end())
            .unwrap_or(0.0)
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
        let mut lines: Vec<_> = self
            .fixed_width_lines(
                cx,
                options,
                width,
                max_graphemes,
                atomics,
                Retention::LastLine,
            )
            .collect();
        add_slice_offsets(&mut lines);
        lines
    }

    // Balance needs count and mapped ends, while all post-scan effects remain
    // necessary. Drop each owned Line instead of retaining trial geometry.
    pub(super) fn break_ends(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        atomics: &AtomicSizes,
    ) -> Vec<u32> {
        self.fixed_width_lines(cx, options, width, None, atomics, Retention::LastLine)
            .map(|line| line.break_token().unit)
            .collect()
    }

    fn fixed_width_lines<'p, 'cx>(
        &'p self,
        cx: &'cx mut LayoutContext,
        options: &LineOptions,
        width: f32,
        max_graphemes: Option<usize>,
        atomics: &'p AtomicSizes,
        scan_retention: Retention,
    ) -> impl Iterator<Item = Line> + 'cx
    where
        'p: 'cx,
    {
        let mut cursor = None;
        self.lines_with_previous::<false, _>(
            cx,
            self.start_token(),
            options,
            move |previous, offset| {
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
            scan_retention,
        )
        .filter_map(|r| match r {
            LineResult::Line(l) => Some(l),
            _ => None,
        })
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
    ///
    /// # Progress requirements
    ///
    /// `previous` can be a line, a block boundary, a float request or a height
    /// rejection. A float request must update
    /// [`LineConstraint::floats_placed_through`] and the available strip; a
    /// height rejection must change the available block space. For explicit
    /// page checkpoints and placement rollback, prefer [`Self::next_line`].
    /// The iterator's accumulated height includes accepted lines; the caller
    /// adds external block extents and chooses the constraint's block offset.
    ///
    /// # Examples
    ///
    /// Change the available width after the first accepted line. This input
    /// has no floats, block boundaries or page-height limit:
    ///
    /// ```
    /// use shodo::font::{FontCollection, FontOptions};
    /// use shodo::limits::Limits;
    /// use shodo::style::{LineOptions, ParagraphStyle};
    /// use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, RichText};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let style = ParagraphStyle::default();
    /// let fonts = FontCollection::with_options(&Limits::default(), FontOptions {
    ///     system_fonts: false, ..Default::default()
    /// });
    /// let mut cx = LayoutContext::new();
    /// let paragraph = RichText::new(&style)
    ///     .push("A paragraph with several wrapped lines", &style.root)
    ///     .build(&mut cx, &fonts)?;
    /// let options = LineOptions::default();
    /// let atomics = AtomicSizes::EMPTY;
    /// let results = paragraph.lines(
    ///     &mut cx, paragraph.start_token(), &options,
    ///     |_, height| {
    ///         let width = if height == 0.0 { 80.0 } else { 160.0 };
    ///         let mut constraint = LineConstraint::new(width);
    ///         constraint.block_offset = height;
    ///         constraint
    ///     },
    ///     &atomics,
    /// );
    /// let mut accepted = 0;
    /// for result in results {
    ///     match result {
    ///         LineResult::Line(line) => {
    ///             println!("{}", &line.text()[line.text_range()]);
    ///             accepted += 1;
    ///         }
    ///         LineResult::Done => break,
    ///         other => return Err(format!("unexpected layout result: {other:?}").into()),
    ///     }
    /// }
    /// assert!(accepted > 1);
    /// # Ok(())
    /// # }
    /// ```
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
        self.lines_with_previous::<true, _>(
            cx,
            token,
            options,
            constraint_fn,
            atomics,
            Retention::AllLines,
        )
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
        scan_retention: Retention,
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
            scan_retention,
            previous: None,
            offset: 0.0,
            done: false,
        }
    }
}

/// Assigns the visual-order inline coordinates that slice backgrounds use
/// after the full set of lines has been laid out.
fn add_slice_offsets(lines: &mut [Line]) {
    if lines.is_empty() {
        return;
    }
    let mut offsets: Option<Vec<f32>> = None;
    for line in lines {
        for record in &mut line.fragments {
            let RecordKind::InlineBox {
                box_index,
                start_edge,
                end_edge,
                slice_offset,
                reversed,
                ..
            } = &mut record.kind
            else {
                continue;
            };
            let index = *box_index as usize;
            let info = &line.data.boxes[index];
            if line.data.styles[info.style as usize].box_decoration_break
                != BoxDecorationBreak::Slice
            {
                continue;
            }
            let offsets = offsets.get_or_insert_with(|| vec![0.0; line.data.boxes.len()]);
            *slice_offset = Some(offsets[index]);
            let margin_start = if *start_edge {
                info.edges.margin.inline_start
            } else {
                0.0
            };
            let margin_end = if *end_edge {
                info.edges.margin.inline_end
            } else {
                0.0
            };
            let (lead_margin, trail_margin) = if *reversed {
                (margin_end, margin_start)
            } else {
                (margin_start, margin_end)
            };
            let border_size = record.inline_size.to_f32() - lead_margin - trail_margin;
            offsets[index] += border_size;
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
    scan_retention: Retention,
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
            let result = self.para.next_line_with_retention(
                self.cx,
                self.token,
                &self.options,
                &c,
                self.atomics,
                self.scan_retention,
            );
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

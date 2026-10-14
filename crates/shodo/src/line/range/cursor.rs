//! A scan's measured deviations from the paragraph's indexed advances.
//! Ruby probes can re-enter the paragraph cache; tab and clipped-cluster
//! prefixes therefore belong to this cursor, never to the cache's last query.
use super::{Natural, absorb, ensure_costs, geometry};
use crate::LayoutContext;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::{AtomicSizes, ParagraphData};

#[derive(Clone, Default)]
struct Prefix(Vec<(usize, i64)>);

impl Prefix {
    fn push(&mut self, at: usize, amount: LayoutUnit) {
        let previous = self.0.last().map_or(0, |(_, total)| *total);
        self.0.push((at, previous + i64::from(amount.raw())));
    }

    fn before(&self, end: usize) -> i64 {
        match self.0.partition_point(|(at, _)| *at < end) {
            0 => 0,
            n => self.0[n - 1].1,
        }
    }

    fn bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<(usize, i64)>()
    }
}

#[derive(Clone)]
pub(in crate::line) struct Cursor {
    start: usize,
    origin: LayoutUnit,
    tabs: Prefix,
    corrections: Prefix,
}

impl Cursor {
    pub(in crate::line) fn new(
        data: &ParagraphData,
        start: usize,
        origin: LayoutUnit,
        atomics: &AtomicSizes,
        cx: &mut LayoutContext,
    ) -> Self {
        cx.ruby_ranges.begin(data, atomics);
        ensure_costs(data, atomics, cx);
        Self {
            start,
            origin,
            tabs: Prefix::default(),
            corrections: Prefix::default(),
        }
    }

    pub(in crate::line) fn bytes(&self) -> usize {
        self.tabs.bytes() + self.corrections.bytes()
    }

    /// Charge each unit once. Only position-dependent tabs, invalid atomics
    /// and source-clipped clusters need the original unit producer.
    pub(in crate::line) fn advance(
        &mut self,
        data: &ParagraphData,
        at: usize,
        atomics: &AtomicSizes,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        let key = (data.id, data as *const ParagraphData as usize);
        let costs = &cx.ruby_ranges.sets[&key];
        let indexed = LayoutUnit::from_raw((costs.advances[at + 1] - costs.advances[at]) as i32);
        let effects = costs.saturation(&(at..at + 1));
        let bad = costs.bad_atomics.binary_search(&at).is_ok();
        let unit = &data.units[at];
        let tab = matches!(unit.kind, UnitKind::Tab) && unit.combine.is_none();
        let clipped = unit.shared_cluster.as_ref().is_some_and(|shared| {
            let start = data.units[self.start].text.start;
            start > shared.text.start && start < shared.text.end
        });
        let position = if tab {
            super::super::spacing_summary::raw(
                costs.advances[at] - costs.advances[self.start] + self.tabs.before(at),
                sat,
            )
            .add(self.origin, sat)
            .add(
                super::super::spacing_summary::raw(self.corrections.before(at), sat),
                sat,
            )
            .add(
                super::super::decoration::width(data, self.start, true, sat),
                sat,
            )
            .add(
                costs.spacing.query(self.start..at, Some(data)).width(sat),
                sat,
            )
        } else {
            LayoutUnit::ZERO
        };
        let width = if tab || clipped || bad {
            super::super::scan::unit_width_from(
                data,
                unit,
                data.units[self.start].text.start,
                position,
                atomics,
                cx,
                sat,
            )
            .add(data.unit_spacing[at].word, sat)
        } else {
            absorb(sat, effects);
            indexed
        };
        if tab {
            self.tabs.push(at, width.sub(indexed, sat));
        } else if clipped {
            self.corrections.push(at, width.sub(indexed, sat));
        }
        width
    }

    /// Pure geometry over the indexed advances and the already-measured
    /// deviations. Pulled closing markers can extend `end` beyond the live
    /// cursor; they have no tab or clipped-cluster deviation.
    pub(in crate::line) fn measure(
        &self,
        data: &ParagraphData,
        end: usize,
        leaf: Option<(usize, super::super::spacing_summary::Summary)>,
        cache: &super::RangeCache,
        sat: &mut Saturation,
    ) -> Natural {
        let key = (data.id, data as *const ParagraphData as usize);
        geometry(
            data,
            &cache.sets[&key],
            self.start..end,
            |end| self.tabs.before(end),
            |end, sat| super::super::spacing_summary::raw(self.corrections.before(end), sat),
            leaf,
            sat,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::line) fn selected(
        &self,
        data: &ParagraphData,
        end: usize,
        widths: Vec<LayoutUnit>,
        indent: LayoutUnit,
        flags: u8,
        options: Option<&crate::style::LineOptions>,
        available: LayoutUnit,
        hyphen: Option<usize>,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> super::super::Scan {
        let windows =
            hyphen.and_then(|end| super::super::hyphen::line(data, self.start, end, cx, sat));
        self.selected_windows(
            data, end, widths, indent, flags, options, available, hyphen, windows, cx, sat,
        )
    }

    /// Planned selections measure the hyphen before clipped unit producers,
    /// retaining their original warning order under a small warning cap.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn selected_windows(
        &self,
        data: &ParagraphData,
        end: usize,
        widths: Vec<LayoutUnit>,
        indent: LayoutUnit,
        flags: u8,
        options: Option<&crate::style::LineOptions>,
        available: LayoutUnit,
        hyphen: Option<usize>,
        windows: Option<Vec<super::super::windows::Window>>,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> super::super::Scan {
        let leaf = windows.as_ref().and(hyphen).map(|end| {
            (
                end - 1,
                super::super::spacing::hyphen_unit_summary(data, end - 1, sat),
            )
        });
        let mut natural = self.measure(data, end, leaf, &cx.ruby_ranges, sat);
        natural.widths = widths;
        super::finish_selected(
            data, self.start, end, indent, flags, options, available, windows, natural, cx, sat,
        )
    }
}

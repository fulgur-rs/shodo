//! A scan's visited caller dimensions and measured deviations from immutable
//! paragraph advances. Starting a cursor never indexes or copies its suffix.
//! Ruby probes can re-enter the paragraph cache; tab and clipped-cluster
//! prefixes therefore belong to this cursor, never to the cache's last query.
use super::{Natural, geometry};
use crate::LayoutContext;
use crate::analysis::units::UnitKind;
use crate::context::count;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::{AtomicSizes, ParagraphData};

#[derive(Clone, Debug, Default)]
pub(super) struct Prefix(Vec<(usize, i64)>);

impl Prefix {
    pub(super) fn push(&mut self, at: usize, amount: i64) {
        let previous = self.0.last().map_or(0, |(_, total)| *total);
        self.0.push((at, previous + amount));
    }

    pub(super) fn before(&self, end: usize) -> i64 {
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
    atoms: Prefix,
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
        Self {
            start,
            origin,
            atoms: Prefix::default(),
            tabs: Prefix::default(),
            corrections: Prefix::default(),
        }
    }

    pub(in crate::line) fn bytes(&self) -> usize {
        self.atoms.bytes() + self.tabs.bytes() + self.corrections.bytes()
    }

    /// Charge each unit once. Only position-dependent tabs, caller atomics
    /// and source-clipped clusters need the original unit producer.
    pub(in crate::line) fn advance(
        &mut self,
        data: &ParagraphData,
        at: usize,
        atomics: &AtomicSizes,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        count!(cx.width_unit_visits += 1);
        let costs = data.width_index.as_ref().expect("finalized width index");
        let indexed = LayoutUnit::from_raw((costs.advances[at + 1] - costs.advances[at]) as i32);
        let effects = costs.saturation(&(at..at + 1));
        let atomic = matches!(data.units[at].kind, UnitKind::Atomic { .. });
        let unit = &data.units[at];
        let tab = matches!(unit.kind, UnitKind::Tab) && unit.combine.is_none();
        let clipped = unit.shared_cluster.as_ref().is_some_and(|shared| {
            let start = data.units[self.start].text.start;
            start > shared.text.start && start < shared.text.end
        });
        let position = if tab {
            super::super::spacing_summary::raw(
                costs.advances[at] - costs.advances[self.start] + self.atoms.before(at)
                    - self.atoms.before(self.start)
                    + self.tabs.before(at),
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
        let width = if tab || clipped || atomic {
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
            sat.absorb(effects);
            indexed
        };
        if atomic {
            self.atoms
                .push(at, i64::from(width.raw()) - i64::from(indexed.raw()));
        } else if tab {
            self.tabs.push(at, i64::from(width.sub(indexed, sat).raw()));
        } else if clipped {
            self.corrections
                .push(at, i64::from(width.sub(indexed, sat).raw()));
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
        sat: &mut Saturation,
    ) -> Natural {
        let index = data.width_index.as_ref().expect("finalized width index");
        geometry(
            data,
            index,
            self.start..end,
            |end| index.advances[end] + self.atoms.before(end),
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
        let mut natural = self.measure(data, end, leaf, sat);
        natural.widths = widths;
        super::finish_selected(
            data, self.start, end, indent, flags, options, available, windows, natural, cx, sat,
        )
    }
}

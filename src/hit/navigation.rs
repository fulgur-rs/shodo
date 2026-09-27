use super::{CaretDirection, LineLayout, NavigationOrder, TextPosition};
use crate::mapping::Affinity;
impl LineLayout<'_> {
    /// Move once in source or visual order. Logical movement visits each byte
    /// boundary once; visual movement preserves distinct bidi locations and
    /// skips coincident stops. Passing either terminal returns `None`.
    pub fn move_caret(
        &self,
        position: TextPosition,
        direction: CaretDirection,
        order: NavigationOrder,
    ) -> Option<TextPosition> {
        let current = self.caret(position)?;
        let line = current.position.line;
        let index = &self.index[line];
        let next = match order {
            NavigationOrder::Logical => match direction {
                CaretDirection::Forward => index.stops.get(
                    index
                        .stops
                        .partition_point(|s| s.position.offset <= current.position.offset),
                ),
                CaretDirection::Backward => index
                    .stops
                    .partition_point(|s| s.position.offset < current.position.offset)
                    .checked_sub(1)
                    .and_then(|i| index.stops.get(i)),
            }
            .map(|stop| {
                index
                    .caret(TextPosition {
                        affinity: Affinity::Downstream,
                        ..stop.position
                    })
                    .unwrap()
                    .position
            }),
            NavigationOrder::Visual => {
                let x = current.rect.inline_start;
                let next = match direction {
                    CaretDirection::Forward => index.visual.get(
                        index
                            .visual
                            .partition_point(|i| index.stops[*i].rect.inline_start <= x),
                    ),
                    CaretDirection::Backward => index
                        .visual
                        .partition_point(|i| index.stops[*i].rect.inline_start < x)
                        .checked_sub(1)
                        .and_then(|i| index.visual.get(i)),
                };
                next.map(|i| index.stops[*i].position)
            }
        };
        if next.is_some() {
            return next;
        }
        let line = match direction {
            CaretDirection::Forward => line.checked_add(1)?,
            CaretDirection::Backward => line.checked_sub(1)?,
        };
        let index = self.index.get(line)?;
        match order {
            NavigationOrder::Logical => {
                let stop = match direction {
                    CaretDirection::Forward => index.stops.first()?,
                    CaretDirection::Backward => index.stops.last()?,
                };
                index
                    .caret(TextPosition {
                        affinity: Affinity::Downstream,
                        ..stop.position
                    })
                    .map(|s| s.position)
            }
            NavigationOrder::Visual => {
                let i = match direction {
                    CaretDirection::Forward => index.visual.first()?,
                    CaretDirection::Backward => index.visual.last()?,
                };
                Some(index.stops[*i].position)
            }
        }
    }
}

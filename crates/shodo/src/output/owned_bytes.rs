//! Requested owned storage; shared paragraph/font/run owners are excluded.
use super::Line;
use crate::shape::GlyphStore;

struct Budget {
    remaining: usize,
}
impl Budget {
    fn charge(&mut self, count: usize, size: usize) -> Option<()> {
        self.remaining = self.remaining.checked_sub(count.checked_mul(size)?)?;
        Some(())
    }
    fn vec<T>(&mut self, v: &Vec<T>) -> Option<()> {
        self.charge(v.capacity(), std::mem::size_of::<T>())
    }
    fn slice<T>(&mut self, v: &[T]) -> Option<()> {
        self.charge(v.len(), std::mem::size_of::<T>())
    }
    fn store(&mut self, g: &GlyphStore) -> Option<()> {
        self.vec(&g.id)?;
        self.vec(&g.advance)?;
        self.vec(&g.pen)?;
        self.vec(&g.offset_inline)?;
        self.vec(&g.offset_block)?;
        self.vec(&g.cluster)?;
        self.vec(&g.flags)?;
        if let Some(v) = &g.spacing {
            self.vec(v)?;
        }
        if let Some(v) = &g.leading {
            self.vec(v)?;
        }
        Some(())
    }
    fn line(&mut self, l: &Line, depth: usize) -> Option<()> {
        // Cache eligibility must not introduce unbounded recursive stack use.
        if depth >= 64 {
            return None;
        }
        self.vec(&l.ruby)?;
        self.vec(&l.ruby_caret_gaps)?;
        self.vec(&l.displaced)?;
        self.vec(&l.fragments)?;
        self.vec(&l.block_shifts)?;
        self.vec(&l.tabs)?;
        self.vec(&l.combinations)?;
        if let Some((_, v)) = &l.positions {
            self.vec(v)?;
        }
        if let Some((_, v)) = &l.glyph_spacing {
            self.vec(v)?;
        }
        if let Some(store) = &l.overlay {
            self.charge(1, std::mem::size_of::<GlyphStore>())?;
            self.store(store)?;
        }
        self.slice(&l.overlay_clusters)?;
        self.slice(&l.overlay_runs)?;
        self.vec(&l.pending_overlays)?;
        for overlay in &l.pending_overlays {
            self.store(&overlay.store)?;
            self.vec(&overlay.runs)?;
        }
        for ruby in &l.ruby {
            self.vec(&ruby.base_nodes)?;
            // Ruby record capacity already includes its embedded Line header.
            self.line(&ruby.line, depth + 1)?;
        }
        Some(())
    }
}
impl Line {
    /// Heap only: callers charge the enclosing header/key exactly once.
    pub(crate) fn owned_heap_bytes(&self, limit: usize) -> Option<usize> {
        let mut b = Budget { remaining: limit };
        b.line(self, 0)?;
        Some(limit - b.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glyph_capacity_charges_unused_storage_and_both_optional_spacing_vectors() {
        let store = GlyphStore {
            id: Vec::with_capacity(3),
            advance: Vec::with_capacity(5),
            pen: Vec::with_capacity(7),
            offset_inline: Vec::with_capacity(11),
            offset_block: Vec::with_capacity(13),
            cluster: Vec::with_capacity(17),
            flags: Vec::with_capacity(19),
            spacing: Some(Vec::with_capacity(23)),
            leading: Some(Vec::with_capacity(29)),
            ..Default::default()
        };
        // Independent requested backing sizes: eight4byte vectors and19flags.
        let mut b = Budget { remaining: 451 };
        assert!(b.store(&store).is_some());
        assert_eq!(b.remaining, 0);
        assert!(Budget { remaining: 450 }.store(&store).is_none());
        assert!(
            Budget {
                remaining: usize::MAX
            }
            .charge(usize::MAX, 2)
            .is_none()
        );
    }
}

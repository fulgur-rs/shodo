use crate::{BenchError, Run};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use shodo::Fragment;
use shodo_fixtures::FixtureFonts;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Digest {
    pub sha256: String,
    pub lines: usize,
    pub glyphs: usize,
    pub runs: usize,
    pub synthetic_glyphs: usize,
    pub float_reports: usize,
    pub height_retries: usize,
    pub intrinsic_measurements: usize,
}
pub fn digest(run: &Run, fonts: &FixtureFonts) -> Result<Digest, BenchError> {
    let mut hash = Sha256::new();
    let mut glyphs = 0;
    let mut runs = 0;
    let integer = |hash: &mut Sha256, n: usize| hash.update((n as u64).to_le_bytes());
    let scalar = |hash: &mut Sha256, n: f32| -> Result<(), BenchError> {
        if !n.is_finite() {
            return Err(BenchError("nonfinite output".into()));
        }
        hash.update(n.to_bits().to_le_bytes());
        Ok(())
    };
    for line in &run.lines {
        integer(&mut hash, line.text_range().start);
        integer(&mut hash, line.text_range().end);
        scalar(&mut hash, line.inline_size())?;
        scalar(&mut hash, line.block_size())?;
        for f in line.fragments() {
            match f {
                Fragment::GlyphRun(r) => {
                    runs += 1;
                    let face = fonts
                        .ids
                        .iter()
                        .position(|id| *id == r.font())
                        .ok_or_else(|| BenchError("glyph uses foreign/synthetic font".into()))?;
                    integer(&mut hash, face);
                    integer(&mut hash, r.bidi_level() as usize);
                    for g in r.glyphs() {
                        if g.id == 0 && g.advance > 0.0 {
                            return Err(BenchError("missing fixed-face glyph".into()));
                        }
                        glyphs += 1;
                        integer(&mut hash, g.id as usize);
                        integer(&mut hash, g.cluster as usize);
                        scalar(&mut hash, g.inline_position)?;
                        scalar(&mut hash, g.block_offset)?;
                        scalar(&mut hash, g.advance)?;
                    }
                }
                Fragment::Atomic(a) => {
                    hash.update(b"atomic");
                    scalar(&mut hash, a.border_rect.inline_start)?;
                    scalar(&mut hash, a.border_rect.inline_size)?;
                    scalar(&mut hash, a.border_rect.block_size)?;
                    scalar(&mut hash, a.baseline)?;
                }
                Fragment::InlineBox(b) => {
                    hash.update(b"box");
                    scalar(&mut hash, b.rect.inline_start)?;
                    scalar(&mut hash, b.rect.inline_size)?;
                }
                Fragment::OutOfFlowAnchor(a) => {
                    hash.update(b"anchor");
                    scalar(&mut hash, a.inline_position)?;
                }
            }
        }
    }
    for i in &run.intrinsics {
        scalar(&mut hash, i.min_content)?;
        scalar(&mut hash, i.max_content)?;
    }
    integer(&mut hash, run.float_reports);
    integer(&mut hash, run.height_retries);
    Ok(Digest {
        sha256: format!("{:x}", hash.finalize()),
        lines: run.lines.len(),
        glyphs,
        runs,
        synthetic_glyphs: 0,
        float_reports: run.float_reports,
        height_retries: run.height_retries,
        intrinsic_measurements: run.intrinsics.len(),
    })
}

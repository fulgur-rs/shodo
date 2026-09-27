//! Criterion drops batched outputs after its clock stops. Validate actual
//! results here so discarded work cannot win, without timing hashes/cleanup.
use crate::{Digest, Operation, Run, Workload, digest, layout};
use shodo::{LayoutContext, Paragraph, limits::Limits};
use shodo_fixtures::FixtureFonts;

pub struct CheckedRun<'a> {
    run: Run,
    fonts: &'a FixtureFonts,
    expected: &'a Digest,
}
impl<'a> CheckedRun<'a> {
    pub fn new(run: Run, fonts: &'a FixtureFonts, expected: &'a Digest) -> Self {
        Self {
            run,
            fonts,
            expected,
        }
    }
}
impl Drop for CheckedRun<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(
                &digest(&self.run, self.fonts).expect("timed output invalid"),
                self.expected,
                "timed output differs from validated workload"
            );
        }
    }
}
pub struct CheckedBuild<'a> {
    paragraphs: Vec<Paragraph>,
    workload: &'a Workload,
    fonts: &'a FixtureFonts,
    limits: &'a Limits,
    expected: &'a Digest,
}
impl<'a> CheckedBuild<'a> {
    pub fn new(
        paragraphs: Vec<Paragraph>,
        workload: &'a Workload,
        fonts: &'a FixtureFonts,
        limits: &'a Limits,
        expected: &'a Digest,
    ) -> Self {
        Self {
            paragraphs,
            workload,
            fonts,
            limits,
            expected,
        }
    }
}
impl Drop for CheckedBuild<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let run = layout(
                self.workload,
                &self.paragraphs,
                &mut LayoutContext::new(),
                self.fonts,
                self.limits,
                Operation::AllLines,
            )
            .expect("timed build output invalid");
            assert_eq!(
                &digest(&run, self.fonts).expect("timed build glyphs invalid"),
                self.expected,
                "timed build differs from validated workload"
            );
        }
    }
}

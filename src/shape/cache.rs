//! Per-thread, font-qualified shape plan LRU. Returned Arc survives eviction.
use crate::font::FontId;
use std::collections::VecDeque;
use std::sync::Arc;
#[derive(Default)]
pub(crate) struct PlanCache {
    entries: VecDeque<(FontId, Arc<harfrust::ShapePlan>)>,
}
impl std::fmt::Debug for PlanCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanCache")
            .field("entries", &self.entries.len())
            .finish()
    }
}
impl PlanCache {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn get(
        &mut self,
        id: FontId,
        shaper: &harfrust::Shaper<'_>,
        buffer: &harfrust::UnicodeBuffer,
        instance: Option<&harfrust::ShaperInstance>,
        features: &[harfrust::Feature],
    ) -> Arc<harfrust::ShapePlan> {
        let language = buffer.language();
        let script = Some(buffer.script());
        let key = harfrust::ShapePlanKey::new(script, buffer.direction())
            .language(language.as_ref())
            .instance(instance)
            .features(features);
        if let Some(index) = self
            .entries
            .iter()
            .position(|(font, plan)| *font == id && key.matches(plan))
        {
            let entry = self.entries.remove(index).unwrap();
            let result = Arc::clone(&entry.1);
            self.entries.push_back(entry);
            return result;
        }
        let plan = Arc::new(harfrust::ShapePlan::new(
            shaper,
            buffer.direction(),
            script,
            language.as_ref(),
            features,
        ));
        if self.entries.len() == 64 {
            self.entries.pop_front();
        }
        self.entries.push_back((id, Arc::clone(&plan)));
        plan
    }
}

#[cfg(test)]
mod tests {
    use crate::LayoutContext;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use std::sync::Arc;

    #[test]
    fn plans_reuse_are_bounded_and_release_on_shrink() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let id = fonts
            .register(include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec())
            .unwrap();
        let data = fonts.font_data(id).unwrap();
        let font = harfrust::FontRef::from_index(data.data.as_ref(), 0).unwrap();
        let shared = fonts.shaper_data(id).unwrap();
        let shaper = shared.shaper(&font).build();
        let mut cx = LayoutContext::new();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.set_script(harfrust::script::LATIN);
        buffer.set_direction(harfrust::Direction::LeftToRight);
        let a = cx.plans.get(id, &shaper, &buffer, None, &[]);
        let b = cx.plans.get(id, &shaper, &buffer, None, &[]);
        assert!(Arc::ptr_eq(&a, &b));
        for n in 0..80 {
            let features = [harfrust::Feature::new(harfrust::Tag::new(b"liga"), n, ..)];
            cx.plans.get(id, &shaper, &buffer, None, &features);
        }
        assert!(cx.plans.len() <= 64);
        cx.scratch = Some(harfrust::UnicodeBuffer::new());
        cx.scratch.as_mut().unwrap().push_str("retained");
        cx.shrink_to(0);
        assert_eq!(cx.plans.len(), 0);
        assert!(cx.scratch.is_none());
        assert_eq!(a.direction(), harfrust::Direction::LeftToRight);
    }
}

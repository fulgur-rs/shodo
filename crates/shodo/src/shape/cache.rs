//! Per-thread, font-qualified shape plan LRU. Returned Arc survives eviction.
use crate::font::FontId;
use std::collections::VecDeque;
use std::sync::Arc;
#[cfg(test)]
std::thread_local! { static PLAN_SEARCH_COMPARISONS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)}; }
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
        self.entries = VecDeque::new();
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
        if let Some(index) = self.entries.iter().rposition(|(font, plan)| {
            #[cfg(test)]
            PLAN_SEARCH_COMPARISONS.with(|count| count.set(count.get() + 1));
            *font == id && key.matches(plan)
        }) {
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
    use super::PLAN_SEARCH_COMPARISONS;
    use crate::LayoutContext;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use std::sync::Arc;

    #[test]
    fn recent_plan_hit_inspects_only_one_entry() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let id = fonts
            .register(crate::test_support::fonts::LATIN.to_vec())
            .unwrap();
        let data = fonts.font_data(id).unwrap();
        let font = harfrust::FontRef::from_index(data.data.as_ref(), 0).unwrap();
        let shared = fonts.shaper_data(id).unwrap();
        let shaper = shared.shaper(&font).build();
        let mut cx = LayoutContext::new();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.set_script(harfrust::script::LATIN);
        buffer.set_direction(harfrust::Direction::LeftToRight);
        let features = |n| [harfrust::Feature::new(harfrust::Tag::new(b"liga"), n, ..)];
        for n in 0..31 {
            cx.plans.get(id, &shaper, &buffer, None, &features(n));
        }
        let last = cx.plans.get(id, &shaper, &buffer, None, &features(31));
        assert_eq!(cx.plans.len(), 32);
        PLAN_SEARCH_COMPARISONS.with(|count| count.set(0));
        let hit = cx.plans.get(id, &shaper, &buffer, None, &features(31));
        assert!(Arc::ptr_eq(&last, &hit));
        assert_eq!(hit.script(), Some(harfrust::script::LATIN));
        assert_eq!(hit.direction(), harfrust::Direction::LeftToRight);
        assert_eq!(
            PLAN_SEARCH_COMPARISONS.with(|count| count.get()),
            1,
            "an MRU hit must inspect one actual plan entry"
        );
    }

    #[test]
    fn plan_keys_remain_font_qualified_and_distinguish_properties() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let a = fonts
            .register(crate::test_support::fonts::LATIN.to_vec())
            .unwrap();
        let b = fonts
            .register(crate::test_support::fonts::LATIN.to_vec())
            .unwrap();
        let data = fonts.font_data(a).unwrap();
        let font = harfrust::FontRef::from_index(data.data.as_ref(), 0).unwrap();
        let shared = fonts.shaper_data(a).unwrap();
        let shaper = shared.shaper(&font).build();
        let mut cx = LayoutContext::new();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.set_script(harfrust::script::LATIN);
        buffer.set_direction(harfrust::Direction::LeftToRight);
        buffer.set_language("en".parse().unwrap());
        let original = cx.plans.get(a, &shaper, &buffer, None, &[]);
        let other_font = cx.plans.get(b, &shaper, &buffer, None, &[]);
        assert!(!Arc::ptr_eq(&original, &other_font));
        let zero = [harfrust::Feature::new(harfrust::Tag::new(b"liga"), 0, ..)];
        let one = [harfrust::Feature::new(harfrust::Tag::new(b"liga"), 1, ..)];
        let local = [harfrust::Feature::new(harfrust::Tag::new(b"liga"), 1, 0..1)];
        let disabled = cx.plans.get(a, &shaper, &buffer, None, &zero);
        let enabled = cx.plans.get(a, &shaper, &buffer, None, &one);
        let ranged = cx.plans.get(a, &shaper, &buffer, None, &local);
        assert!(!Arc::ptr_eq(&original, &disabled));
        assert!(!Arc::ptr_eq(&disabled, &enabled));
        assert!(!Arc::ptr_eq(&enabled, &ranged));
        buffer.set_direction(harfrust::Direction::RightToLeft);
        let rtl = cx.plans.get(a, &shaper, &buffer, None, &[]);
        assert!(!Arc::ptr_eq(&original, &rtl));
        buffer.set_direction(harfrust::Direction::LeftToRight);
        buffer.set_script(harfrust::script::GREEK);
        let greek = cx.plans.get(a, &shaper, &buffer, None, &[]);
        assert!(!Arc::ptr_eq(&original, &greek));
        buffer.set_script(harfrust::script::LATIN);
        buffer.set_language("ar".parse().unwrap());
        let arabic_lang = cx.plans.get(a, &shaper, &buffer, None, &[]);
        assert!(!Arc::ptr_eq(&original, &arabic_lang));
        buffer.set_language("en".parse().unwrap());
        assert!(Arc::ptr_eq(
            &original,
            &cx.plans.get(a, &shaper, &buffer, None, &[])
        ));
    }

    #[test]
    fn plan_hits_promote_lru_and_external_owners_survive_eviction_and_shrink() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let id = fonts
            .register(crate::test_support::fonts::LATIN.to_vec())
            .unwrap();
        let data = fonts.font_data(id).unwrap();
        let font = harfrust::FontRef::from_index(data.data.as_ref(), 0).unwrap();
        let shared = fonts.shaper_data(id).unwrap();
        let shaper = shared.shaper(&font).build();
        let mut cx = LayoutContext::new();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.set_script(harfrust::script::LATIN);
        buffer.set_direction(harfrust::Direction::LeftToRight);
        let features = |n| [harfrust::Feature::new(harfrust::Tag::new(b"liga"), n, ..)];
        let first = cx.plans.get(id, &shaper, &buffer, None, &features(0));
        let victim = Arc::downgrade(&cx.plans.get(id, &shaper, &buffer, None, &features(1)));
        for n in 2..64 {
            cx.plans.get(id, &shaper, &buffer, None, &features(n));
        }
        assert_eq!(cx.plans.len(), 64);
        assert!(victim.upgrade().is_some());
        assert!(Arc::ptr_eq(
            &first,
            &cx.plans.get(id, &shaper, &buffer, None, &features(0))
        ));
        assert!(Arc::ptr_eq(&first, &cx.plans.entries.back().unwrap().1));
        cx.plans.get(id, &shaper, &buffer, None, &features(64));
        assert_eq!(cx.plans.len(), 64);
        assert!(victim.upgrade().is_none());
        for n in 65..129 {
            cx.plans.get(id, &shaper, &buffer, None, &features(n));
        }
        assert_eq!(Arc::strong_count(&first), 1);
        assert_eq!(first.direction(), harfrust::Direction::LeftToRight);
        cx.shrink_to(0);
        assert_eq!(cx.plans.len(), 0);
        assert_eq!(cx.plans.entries.capacity(), 0);
        assert_eq!(first.script(), Some(harfrust::script::LATIN));
    }

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
            .register(crate::test_support::fonts::LATIN.to_vec())
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
        assert_eq!(
            cx.plans.entries.capacity(),
            0,
            "no retained plan allocation"
        );
        assert!(cx.scratch.is_none());
        assert_eq!(a.direction(), harfrust::Direction::LeftToRight);
    }
}

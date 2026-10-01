use shodo::font::{FontCollection, FontOptions};
use shodo::limits::Limits;

#[test]
fn document_policy_follows_the_shared_root_across_forks_and_clones() {
    let limits = Limits::default();
    for (system_fonts, bundled_only) in [(false, true), (true, false)] {
        let shared = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts,
                ..Default::default()
            },
        );
        let document = FontCollection::for_document(&shared, &limits);
        let sibling = FontCollection::for_document(&document, &limits);
        for fonts in [shared.clone(), document, sibling] {
            assert_eq!(fonts.is_bundled_only(), bundled_only);
        }
    }
}

#[test]
fn default_policy_follows_the_system_fonts_feature() {
    let fonts = FontCollection::new(&Limits::default());
    assert_eq!(fonts.is_bundled_only(), !cfg!(feature = "system-fonts"));
}

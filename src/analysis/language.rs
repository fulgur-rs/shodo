//! BCP47 syntax and pinned IANA primary-language recognition.
const REGISTERED: &[u8] = include_bytes!("languages.dat");

pub(crate) fn recognized(tag: &str) -> bool {
    let Ok(locale) = tag.parse::<icu_locale_core::Locale>() else {
        return false;
    };
    let language = locale.id.language.as_str().as_bytes();
    if language.len() == 3 && &b"qaa"[..] <= language && language <= &b"qtz"[..] {
        return true;
    }
    if !(2..=3).contains(&language.len()) {
        return false;
    }
    let mut key = [b' '; 3];
    key[..language.len()].copy_from_slice(language);
    let mut low = 0;
    let mut high = REGISTERED.len() / 3;
    while low < high {
        let mid = low + (high - low) / 2;
        match REGISTERED[mid * 3..mid * 3 + 3].cmp(&key) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}

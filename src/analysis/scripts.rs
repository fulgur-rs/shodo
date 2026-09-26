//! Contextual script resolution using Unicode script extensions and brackets.
use icu_properties::{props::Script, script::ScriptWithExtensions};
use std::collections::BTreeMap;
use unicode_bidi::{BidiDataSource, HardcodedBidiData};

fn neutral(script: Script) -> bool {
    matches!(script, Script::Common | Script::Inherited | Script::Unknown)
}

pub(super) fn resolve(chars: impl Iterator<Item = char>) -> Vec<Script> {
    let data = ScriptWithExtensions::new();
    let chars: Vec<_> = chars.collect();
    let mut scripts: Vec<_> = chars.iter().map(|c| data.get_script_val(*c)).collect();
    let permits = |c, script| {
        let ext = data.get_script_extensions_val(c);
        !ext.iter().any(|s| !neutral(s)) || ext.contains(&script)
    };
    // Index each bracket class separately. Removing unmatched inner openings
    // when a closer matches amortizes over all openings, avoiding deep scans.
    let mut openings = Vec::new();
    let mut by_class: BTreeMap<char, Vec<usize>> = BTreeMap::new();
    let mut last = Script::Common;
    for (i, c) in chars.iter().enumerate() {
        if neutral(scripts[i]) && permits(*c, last) {
            scripts[i] = last;
        }
        if let Some(bracket) = HardcodedBidiData.bidi_matched_opening_bracket(*c) {
            if bracket.is_open {
                by_class
                    .entry(bracket.opening)
                    .or_default()
                    .push(openings.len());
                openings.push((bracket.opening, i));
            } else if let Some(at) = by_class
                .get(&bracket.opening)
                .and_then(|v| v.last())
                .copied()
            {
                let opening_script = scripts[openings[at].1];
                if !neutral(opening_script) {
                    scripts[i] = opening_script;
                }
                for (class, _) in openings.drain(at..) {
                    by_class.get_mut(&class).expect("indexed opening").pop();
                }
            }
        }
        if !neutral(scripts[i]) {
            last = scripts[i];
        }
    }
    last = Script::Latin;
    for i in (0..scripts.len()).rev() {
        if neutral(scripts[i]) {
            let ext = data.get_script_extensions_val(chars[i]);
            scripts[i] = if permits(chars[i], last) {
                last
            } else {
                ext.iter().find(|s| !neutral(*s)).unwrap_or(Script::Latin)
            };
        }
        last = scripts[i];
    }
    scripts
}

use raikiri_style::ComputedValues;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct Difference {
    pub field: String,
    pub value: String,
    pub initial: String,
}

fn clone_value<T: Clone>(value: &T) -> T {
    value.clone()
}

// Only accessor names and the frozen S4 reset-field profile are generated.
// No CSS parser or layout implementation is copied here.
include!("raikiri_style_fields.rs");

#[derive(Clone, Copy)]
pub enum InputProfile {
    Plain,
    MeasuredBlock,
    Atomic,
}
impl InputProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain-inline",
            Self::MeasuredBlock => "measured-block",
            Self::Atomic => "atomic",
        }
    }
}

/// The original measured WPT caller preprocesses root/atomic sizing before
/// entering inline_style. Apply those resets so already-owned dimensions do
/// not get incorrectly reported as causes of the residual rejection.
pub fn prepare_input(values: &ComputedValues, profile: InputProfile) -> ComputedValues {
    let mut input = values.clone();
    let initial = ComputedValues::initial();
    match profile {
        InputProfile::Plain => {}
        InputProfile::MeasuredBlock => reset_measured(&mut input, &initial),
        InputProfile::Atomic => reset_atomic(&mut input, &initial),
    }
    input
}

/// Reproduce the residual equality check after the frozen profile's 50 resets.
/// This diagnoses that check, not the earlier checks in the S4 style converter.
pub fn differences(values: &ComputedValues) -> Vec<Difference> {
    let initial = ComputedValues::initial();
    let mut remaining = values.clone();
    reset_mapped(&mut remaining, &initial);
    if remaining == initial {
        return Vec::new();
    }
    let mut diffs = public_differences(&remaining, &initial);
    let mut opaque = remaining.clone();
    reset_public(&mut opaque, &initial);
    if opaque != initial {
        // This pin's two private custom-property environments participate in
        // PartialEq. Their bounded Debug omits ancestor bindings. First prove
        // an actual private inequality; has_parent alone can differ even when
        // the effective bindings are equal, and must not cause a false finding.
        let actual = debug_fields(&format!("{opaque:#?}"));
        let default = debug_fields(&format!("{initial:#?}"));
        for field in ["custom_properties", "local_custom_properties"] {
            if let (Some(value), Some(initial)) = (actual.get(field), default.get(field))
                && value != initial
            {
                diffs.push(Difference {
                    field: field.into(),
                    value: value.clone(),
                    initial: initial.clone(),
                });
            }
        }
    }
    if diffs.is_empty() {
        // An unreported private/format change is evidence of a gap, never an
        // empty diagnostic that falsely clears a known residual inequality.
        diffs.push(Difference {
            field: "unreported_residual".into(),
            value: format!("{remaining:#?}"),
            initial: format!("{initial:#?}"),
        });
    }
    diffs.sort_by(|a, b| a.field.cmp(&b.field));
    diffs
}

/// Top-level fields of this pinned derive(Debug) output. This is only used for
/// private environment diagnostics; public comparisons are typed PartialEq.
fn debug_fields(text: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines().skip(1).take_while(|line| *line != "}") {
        let field = line
            .strip_prefix("    ")
            .and_then(|line| line.split_once(": "))
            .filter(|(name, _)| {
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            });
        if let Some((name, value)) = field {
            if let Some((name, value)) = current.take() {
                fields.insert(name, value.trim_end_matches(',').into());
            }
            current = Some((name.into(), value.into()));
        } else if let Some((_, value)) = &mut current {
            value.push('\n');
            value.push_str(line);
        }
    }
    if let Some((name, value)) = current {
        fields.insert(name, value.trim_end_matches(',').into());
    }
    fields
}

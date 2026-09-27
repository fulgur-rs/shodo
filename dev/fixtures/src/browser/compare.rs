use super::capture::{AtomicObservation, BrowserCapture};
use super::input::{BrowserCase, BuiltCase, build, source_end};
use crate::FixtureFonts;
use serde::Deserialize;
use shodo::LayoutContext;

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Mismatch {
    pub font_ids: Vec<String>,
    pub font_size: f32,
    pub id: String,
    pub seed: u32,
    pub width_subpixels: u32,
    pub expected: usize,
    pub actual: usize,
    pub text: String,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} seed={} width={}/64={}px expected={} actual={} source UTF-8 bytes font={:?} size={}px input={:?}",
            self.id,
            self.seed,
            self.width_subpixels,
            self.width_subpixels as f64 / 64.0,
            self.expected,
            self.actual,
            self.font_ids,
            self.font_size,
            self.text
        )
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Comparison {
    pub samples: usize,
    pub mismatches: Vec<Mismatch>,
}

/// Measure the unmodified browser width through the public pure line API.
pub fn first_end(
    case: &BrowserCase,
    built: &BuiltCase,
    cx: &mut LayoutContext,
    width_subpixels: u32,
) -> Result<usize, String> {
    if width_subpixels == 0 {
        return Err("probe width must be positive".into());
    }
    match built.paragraph.next_line(
        cx,
        built.paragraph.start_token(),
        &Default::default(),
        &shodo::LineConstraint::new(width_subpixels as f32 / 64.0),
        &built.atomics,
    ) {
        shodo::LineResult::Line(line) => source_end(case, &line),
        other => Err(format!(
            "{}: unexpected first line result {other:?}",
            case.id
        )),
    }
}

/// Compare validated capture records, or an explicitly selected diagnostic subset.
pub fn compare(
    data: &BrowserCapture,
    cases: &[BrowserCase],
    cx: &mut LayoutContext,
    fonts: &FixtureFonts,
) -> Result<Comparison, String> {
    let mut result = Comparison {
        samples: 0,
        mismatches: Vec::new(),
    };
    for case in cases {
        let record = data
            .records
            .iter()
            .find(|r| r.id == case.id)
            .ok_or_else(|| format!("{}: missing browser record", case.id))?;
        let built = build(case, cx, fonts)?;
        for sample in &record.samples {
            let actual = first_end(case, &built, cx, sample.width_subpixels)?;
            result.samples += 1;
            if actual != sample.end_utf8 {
                result.mismatches.push(Mismatch {
                    font_ids: case.font_ids.clone(),
                    font_size: case.font_size,
                    id: case.id.clone(),
                    seed: case.seed,
                    width_subpixels: sample.width_subpixels,
                    expected: sample.end_utf8,
                    actual,
                    text: case.text(),
                });
            }
        }
    }
    Ok(result)
}

#[derive(Debug, serde::Serialize, Deserialize, PartialEq, Eq)]
pub struct Transition {
    pub id: String,
    pub target: usize,
    pub browser_subpixels: u32,
    pub shodo_subpixels: u32,
    pub shodo_at: usize,
    pub delta_subpixels: i64,
}

/// Minimum raw width reaching the browser target. `shodo_at > target` means
/// this target is skipped, so a width drift cannot explain that discrepancy.
pub fn transitions(
    data: &BrowserCapture,
    cases: &[BrowserCase],
    cx: &mut LayoutContext,
    fonts: &FixtureFonts,
) -> Result<Vec<Transition>, String> {
    let mut result = Vec::new();
    for case in cases {
        let record = data
            .records
            .iter()
            .find(|r| r.id == case.id)
            .ok_or_else(|| format!("{}: missing browser record", case.id))?;
        let Some(browser_width) = record.boundary_subpixels else {
            continue;
        };
        let target = record.initial.end_utf8;
        let built = build(case, cx, fonts)?;
        let mut lo = 1;
        if first_end(case, &built, cx, lo)? >= target {
            result.push(Transition {
                id: case.id.clone(),
                target,
                browser_subpixels: browser_width,
                shodo_subpixels: lo,
                shodo_at: first_end(case, &built, cx, lo)?,
                delta_subpixels: i64::from(lo) - i64::from(browser_width),
            });
            continue;
        }
        let mut hi = case.width_subpixels;
        while first_end(case, &built, cx, hi)? < target {
            hi = hi
                .checked_mul(2)
                .filter(|&v| v <= 1_048_576)
                .ok_or_else(|| format!("{}: transition not reachable", case.id))?;
        }
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if first_end(case, &built, cx, mid)? >= target {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        result.push(Transition {
            id: case.id.clone(),
            target,
            browser_subpixels: browser_width,
            shodo_subpixels: hi,
            shodo_at: first_end(case, &built, cx, hi)?,
            delta_subpixels: i64::from(hi) - i64::from(browser_width),
        });
    }
    Ok(result)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownDifference {
    pub id: String,
    pub width_subpixels: u32,
    pub expected: usize,
    pub actual: usize,
    pub category: String,
    pub evidence: String,
    pub issue: Option<String>,
}

/// Exact observations only. Improvement is also a stale exception, requiring
/// deliberate review; duplicate, unknown and changed differences all fail.
pub fn check_differences(result: &Comparison, entries: &[KnownDifference]) -> Result<(), String> {
    let mut remaining = std::collections::BTreeMap::new();
    for e in entries {
        if e.expected == e.actual
            || e.category.is_empty()
            || e.evidence.is_empty()
            || remaining.insert((&e.id, e.width_subpixels), e).is_some()
        {
            return Err("invalid/duplicate known difference".into());
        }
    }
    for m in &result.mismatches {
        match remaining.remove(&(&m.id, m.width_subpixels)) {
            Some(e) if e.expected == m.expected && e.actual == m.actual => {}
            _ => return Err(format!("unknown or changed difference: {m}")),
        }
    }
    if let Some(e) = remaining.values().next() {
        return Err(format!(
            "stale difference (improvement/input change): {} at {}/64px",
            e.id, e.width_subpixels
        ));
    }
    Ok(())
}

/// Full line layout is needed when the atomic wraps after the first line.
pub fn atomic_observation(
    built: &BuiltCase,
    cx: &mut LayoutContext,
    width_subpixels: u32,
) -> Result<Option<AtomicObservation>, String> {
    if width_subpixels == 0 {
        return Err("probe width must be positive".into());
    }
    for line in built.paragraph.break_all(
        cx,
        &Default::default(),
        width_subpixels as f32 / 64.0,
        &built.atomics,
    ) {
        for fragment in line.fragments() {
            if let shodo::Fragment::Atomic(a) = fragment {
                let r = a.border_rect;
                let top = f64::from(line.block_offset() + r.block_start);
                return Ok(Some(AtomicObservation {
                    top,
                    bottom: top + f64::from(r.block_size),
                    width: f64::from(r.inline_size),
                    height: f64::from(r.block_size),
                }));
            }
        }
    }
    Ok(None)
}

#[derive(Debug, serde::Serialize, Deserialize, PartialEq)]
pub struct GeometryComparison {
    pub id: String,
    pub width_subpixels: u32,
    pub browser: Option<AtomicObservation>,
    pub shodo: Option<AtomicObservation>,
}

pub fn atomic_comparisons(
    data: &BrowserCapture,
    cases: &[BrowserCase],
    cx: &mut LayoutContext,
    fonts: &FixtureFonts,
) -> Result<Vec<GeometryComparison>, String> {
    let mut result = Vec::new();
    for case in cases
        .iter()
        .filter(|c| c.parts.iter().any(|p| p.atomic_width.is_some()))
    {
        let record = data
            .records
            .iter()
            .find(|r| r.id == case.id)
            .ok_or_else(|| format!("{}: missing browser record", case.id))?;
        let built = build(case, cx, fonts)?;
        for sample in &record.samples {
            result.push(GeometryComparison {
                id: case.id.clone(),
                width_subpixels: sample.width_subpixels,
                browser: sample.atomic.clone(),
                shodo: atomic_observation(&built, cx, sample.width_subpixels)?,
            });
        }
    }
    Ok(result)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DifferenceLedger {
    pub format_version: u32,
    pub inputs_sha256: String,
    pub capture_sha256: String,
    pub notes: Vec<String>,
    pub transitions: Vec<Transition>,
    pub atomic_geometry: Vec<GeometryComparison>,
    pub entries: Vec<KnownDifference>,
}

pub fn difference_ledger() -> Result<DifferenceLedger, String> {
    serde_json::from_str(include_str!("../../assets/browser/differences.json"))
        .map_err(|e| e.to_string())
}

pub fn check_all(
    data: &BrowserCapture,
    cases: &[BrowserCase],
    cx: &mut LayoutContext,
    fonts: &FixtureFonts,
    ledger: &DifferenceLedger,
) -> Result<Comparison, String> {
    use sha2::{Digest, Sha256};
    data.validate(cases)?;
    if ledger.format_version != 1
        || ledger.notes.is_empty()
        || data.metadata["inputs_sha256"] != ledger.inputs_sha256
        || ledger.capture_sha256
            != format!(
                "{:x}",
                Sha256::digest(include_bytes!("../../assets/browser/chromium.json"))
            )
    {
        return Err("known difference ledger has wrong capture/input metadata".into());
    }
    let result = compare(data, cases, cx, fonts)?;
    check_differences(&result, &ledger.entries)?;
    let actual_transitions = transitions(data, cases, cx, fonts)?;
    if actual_transitions != ledger.transitions {
        return Err(format!(
            "raw transition thresholds changed; inspect --transitions: {actual_transitions:?}"
        ));
    }
    let geometry = atomic_comparisons(data, cases, cx, fonts)?;
    if geometry != ledger.atomic_geometry {
        return Err(format!(
            "inline-block baseline geometry changed; inspect --atomics: {geometry:?}"
        ));
    }
    Ok(result)
}

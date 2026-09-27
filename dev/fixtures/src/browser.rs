//! Shared fixed inputs for numeric browser comparison and future snapshots.
use std::sync::OnceLock;

use serde::Deserialize;
use shodo::geometry::Direction;
use shodo::mapping::{Affinity, TextOrigin};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineHeight, ParagraphStyle, WhiteSpaceCollapse};
use shodo::{AtomicSize, AtomicSizes, LayoutContext, Line, Paragraph, ParagraphBuilder};

use crate::{FixtureFonts, font};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPart {
    pub text: String,
    pub depth: usize,
    pub color: Option<String>,
    pub atomic_width: Option<f32>,
    pub atomic_height: Option<f32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCase {
    pub id: String,
    pub seed: u32,
    pub font_ids: Vec<String>,
    pub font_size: f32,
    pub width_subpixels: u32,
    pub direction: String,
    pub lang: String,
    pub white_space: String,
    pub parts: Vec<BrowserPart>,
}

impl BrowserCase {
    pub fn text(&self) -> String {
        self.parts.iter().map(|p| p.text.as_str()).collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.font_ids.is_empty()
            || self.font_ids.iter().any(|id| font(id).is_none())
            || !self.font_size.is_finite()
            || !(1.0..=128.0).contains(&self.font_size)
            || self.width_subpixels == 0
            || self.width_subpixels > 1_048_576
            || !matches!(self.direction.as_str(), "ltr" | "rtl")
            || !matches!(self.white_space.as_str(), "normal" | "pre-wrap")
            || self.parts.is_empty()
            || self.text().is_empty()
        {
            return Err(format!("{}: invalid fixed browser input", self.id));
        }
        if self
            .parts
            .iter()
            .filter(|p| p.atomic_width.is_some())
            .count()
            > 1
        {
            return Err(format!(
                "{}: fixed recorder supports only one atomic",
                self.id
            ));
        }
        for part in &self.parts {
            if part.depth > 8 || part.text.is_empty() {
                return Err(format!("{}: invalid part", self.id));
            }
            match (part.atomic_width, part.atomic_height) {
                (None, None) if !part.text.contains('\u{fffc}') => {}
                (Some(w), Some(h))
                    if part.text == "\u{fffc}"
                        && w.is_finite()
                        && h.is_finite()
                        && w > 0.0
                        && h > 0.0 => {}
                _ => return Err(format!("{}: invalid atomic input", self.id)),
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFile {
    format_version: u32,
    generator: String,
    cases: Vec<BrowserCase>,
}

pub fn cases() -> &'static [BrowserCase] {
    static CASES: OnceLock<Vec<BrowserCase>> = OnceLock::new();
    CASES.get_or_init(|| {
        let input: InputFile = serde_json::from_str(include_str!("../assets/browser-inputs.json"))
            .expect("checked-in browser inputs parse");
        assert_eq!(input.format_version, 1);
        assert!(!input.generator.is_empty());
        for case in &input.cases {
            case.validate().expect("valid fixed browser input");
        }
        input.cases
    })
}

pub struct BuiltCase {
    pub paragraph: Paragraph,
    pub atomics: AtomicSizes,
}

pub fn build(
    case: &BrowserCase,
    cx: &mut LayoutContext,
    fonts: &FixtureFonts,
) -> Result<BuiltCase, String> {
    case.validate()?;
    let direction = if case.direction == "rtl" {
        Direction::Rtl
    } else {
        Direction::Ltr
    };
    let inline = InlineStyle {
        font_families: case
            .font_ids
            .iter()
            .map(|id| FontFamily::Named(font(id).unwrap().family.into()))
            .collect(),
        font_size: case.font_size,
        lang: Some(case.lang.clone()),
        direction,
        line_height: LineHeight::Px(40.0),
        white_space_collapse: if case.white_space == "pre-wrap" {
            WhiteSpaceCollapse::Preserve
        } else {
            WhiteSpaceCollapse::Collapse
        },
        line_break: shodo::style::LineBreak::Normal,
        ..Default::default()
    };
    let style = ParagraphStyle {
        root: inline.clone(),
        direction,
        ..Default::default()
    };
    let limits = Default::default();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.with_offset_mapping(true);
    let mut depth = 0;
    let mut atomics = AtomicSizes::new();
    for (index, part) in case.parts.iter().enumerate() {
        while depth > part.depth {
            b.close_inline();
            depth -= 1;
        }
        while depth < part.depth {
            b.open_inline(
                NodeId(10000 + index as u64 * 10 + depth as u64),
                &inline,
                Default::default(),
            );
            depth += 1;
        }
        let node = NodeId(100 + index as u64);
        if let (Some(w), Some(h)) = (part.atomic_width, part.atomic_height) {
            b.push_atomic(node, &inline, Default::default());
            atomics.insert(
                node,
                AtomicSize {
                    inline_size: w,
                    block_size: h,
                    baseline: None,
                    ..Default::default()
                },
            );
        } else {
            b.push_text(TextSource::Dom { node, offset: 0 }, &part.text);
        }
    }
    while depth > 0 {
        b.close_inline();
        depth -= 1;
    }
    let paragraph = b
        .build(cx, &fonts.collection)
        .map_err(|e| format!("{}: {e:?}", case.id))?;
    Ok(BuiltCase { paragraph, atomics })
}

/// Original concatenated-source endpoint, not a processed UTF-8 offset.
pub fn source_end(case: &BrowserCase, line: &Line) -> Result<usize, String> {
    let range = line.text_range();
    if range.end == 0 {
        return Ok(0);
    }
    let origin = line
        .offset_mapping()
        .and_then(|m| m.text_to_dom(range.end as u32, Affinity::Upstream))
        .ok_or_else(|| format!("{}: missing endpoint mapping at {}", case.id, range.end))?;
    let (node, offset) = match origin {
        TextOrigin::Dom { node, offset } => (node, Some(offset as usize)),
        TextOrigin::Generated { node } => (node, None),
    };
    let index = node
        .0
        .checked_sub(100)
        .and_then(|i| usize::try_from(i).ok())
        .filter(|&i| i < case.parts.len())
        .ok_or_else(|| format!("{}: unexpected source node {node:?}", case.id))?;
    let offset = offset.unwrap_or(case.parts[index].text.len());
    if !case.parts[index].text.is_char_boundary(offset) {
        return Err(format!("{}: invalid source endpoint", case.id));
    }
    Ok(case.parts[..index]
        .iter()
        .map(|p| p.text.len())
        .sum::<usize>()
        + offset)
}

/// Convert UTF-16 code units only at scalar boundaries; surrogate interiors fail.
pub fn utf16_to_utf8(text: &str, units: usize) -> Option<usize> {
    let mut used = 0;
    for (byte, ch) in text.char_indices() {
        if used == units {
            return Some(byte);
        }
        used += ch.len_utf16();
        if used > units {
            return None;
        }
    }
    (used == units).then_some(text.len())
}

pub fn utf8_to_utf16(text: &str, byte: usize) -> Option<usize> {
    text.get(..byte).map(|prefix| prefix.encode_utf16().count())
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BrowserSample {
    pub width_subpixels: u32,
    pub end_utf16: usize,
    pub end_utf8: usize,
    pub atomic: Option<AtomicObservation>,
}

#[derive(Clone, Debug, serde::Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AtomicObservation {
    pub top: f64,
    pub bottom: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserRecord {
    pub id: String,
    pub seed: u32,
    pub text: String,
    pub initial: BrowserSample,
    pub boundary_subpixels: Option<u32>,
    pub samples: Vec<BrowserSample>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureFont {
    pub id: String,
    pub sha256: String,
    pub face_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCapture {
    pub format_version: u32,
    pub user_agent: String,
    pub fonts: Vec<CaptureFont>,
    pub records: Vec<BrowserRecord>,
    pub metadata: serde_json::Value,
}

pub fn capture() -> Result<BrowserCapture, String> {
    serde_json::from_str(include_str!("../assets/browser/chromium.json")).map_err(|e| e.to_string())
}

impl BrowserCapture {
    pub fn validate(&self, cases: &[BrowserCase]) -> Result<(), String> {
        if self.format_version != 1
            || self.user_agent.is_empty()
            || self.records.len() != cases.len()
            || self.fonts.len() != crate::FONTS.len()
            || self.metadata["subpixels_per_px"] != 64
            || self.metadata["line_height"] != 40
            || self.metadata["browser_version"]
                .as_str()
                .is_none_or(str::is_empty)
        {
            return Err("invalid browser capture metadata/count".into());
        }
        for (actual, font) in self.fonts.iter().zip(crate::FONTS) {
            if actual.id != font.id
                || actual.sha256 != font.sha256
                || actual.face_index != font.face_index
            {
                return Err("browser fixture font differs".into());
            }
        }
        use sha2::{Digest, Sha256};
        for (key, bytes) in [
            (
                "inputs_sha256",
                include_bytes!("../assets/browser-inputs.json").as_slice(),
            ),
            (
                "corpus_sha256",
                include_bytes!("../assets/cases.json").as_slice(),
            ),
            (
                "recorder_sha256",
                include_bytes!("../tools/browser_recorder.js").as_slice(),
            ),
        ] {
            if self.metadata[key] != format!("{:x}", Sha256::digest(bytes)) {
                return Err(format!("browser {key} differs; recollect explicitly"));
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for (r, c) in self.records.iter().zip(cases) {
            c.validate()?;
            if !ids.insert(&r.id)
                || r.id != c.id
                || r.seed != c.seed
                || r.text != c.text()
                || r.initial.width_subpixels != c.width_subpixels
                || !r.samples.contains(&r.initial)
                || r.samples.first().map(|s| s.width_subpixels) != Some(1)
            {
                return Err(format!(
                    "{}: mismatched or missing browser input/sample",
                    c.id
                ));
            }
            let mut prev = 0;
            for s in &r.samples {
                if s.width_subpixels <= prev
                    || utf16_to_utf8(&r.text, s.end_utf16) != Some(s.end_utf8)
                {
                    return Err(format!("{}: invalid probe width/source endpoint", c.id));
                }
                prev = s.width_subpixels;
                let atomic_part = c.parts.iter().find(|p| p.atomic_width.is_some());
                if s.atomic.is_some() != atomic_part.is_some() {
                    return Err(format!("{}: missing/unexpected atomic observation", c.id));
                }
                if let Some(a) = &s.atomic {
                    let part = atomic_part.unwrap();
                    if a.width != f64::from(part.atomic_width.unwrap())
                        || a.height != f64::from(part.atomic_height.unwrap())
                    {
                        return Err(format!("{}: atomic dimensions differ", c.id));
                    }
                    if ![a.top, a.bottom, a.width, a.height]
                        .iter()
                        .all(|x| x.is_finite())
                        || a.width <= 0.0
                        || a.height <= 0.0
                        || (a.bottom - a.top - a.height).abs() > 0.001
                    {
                        return Err(format!("{}: invalid atomic observation", c.id));
                    }
                }
            }
            if let Some(width) = r.boundary_subpixels {
                let before = width
                    .checked_sub(1)
                    .and_then(|w| r.samples.iter().find(|s| s.width_subpixels == w));
                let at = r.samples.iter().find(|s| s.width_subpixels == width);
                if width <= 1
                    || width > c.width_subpixels
                    || !before.zip(at).is_some_and(|(b, a)| {
                        b.end_utf8 < r.initial.end_utf8 && a.end_utf8 >= r.initial.end_utf8
                    })
                {
                    return Err(format!("{}: invalid transition bracket", c.id));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct Mismatch {
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
            "{} seed={} width={}/64={}px expected={} actual={} source UTF-8 bytes input={:?}",
            self.id,
            self.seed,
            self.width_subpixels,
            self.width_subpixels as f64 / 64.0,
            self.expected,
            self.actual,
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
    serde_json::from_str(include_str!("../assets/browser/differences.json"))
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
                Sha256::digest(include_bytes!("../assets/browser/chromium.json"))
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

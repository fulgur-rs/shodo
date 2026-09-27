use super::input::{BrowserCase, utf16_to_utf8};
use serde::Deserialize;

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
    serde_json::from_str(include_str!("../../assets/browser/chromium.json"))
        .map_err(|e| e.to_string())
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
                include_bytes!("../../assets/browser-inputs.json").as_slice(),
            ),
            (
                "corpus_sha256",
                include_bytes!("../../assets/cases.json").as_slice(),
            ),
            (
                "recorder_sha256",
                include_bytes!("../../tools/browser_recorder.js").as_slice(),
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

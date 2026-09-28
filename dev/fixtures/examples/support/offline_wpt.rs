use raikiri_html::UncascadedDocument;
use raikiri_style::CascadeResult;
use raikiri_traits::{
    Body, FetchOutcome, FetchedResource, Method, NetworkError, NetworkProvider, Request,
    ResourceKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceRecord {
    pub url: String,
    pub kind: String,
    pub bytes: Option<usize>,
    pub sha256: Option<String>,
    pub error: Option<String>,
}
pub struct ScreenInput {
    pub parsed: UncascadedDocument,
    pub cascade: CascadeResult,
    pub resources: Vec<ResourceRecord>,
}

/// Offline original-resource adapter. Its origin and 32MiB resource boundary
/// match the frozen S4 replay, without importing its layout/paint crate.
struct Resources {
    root: PathBuf,
    records: Mutex<Vec<ResourceRecord>>,
}
impl Resources {
    fn new(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        if !root.is_dir() {
            return Err("WPT root must be a directory".into());
        }
        Ok(Self {
            root,
            records: Mutex::new(Vec::new()),
        })
    }
    fn read(&self, request: &Request) -> Result<FetchedResource, NetworkError> {
        let url = &request.url;
        if request.signal.as_ref().is_some_and(|s| s.is_aborted()) {
            return Err(NetworkError::Aborted);
        }
        if request.method != Method::Get
            || url.scheme() != "http"
            || url.host_str() != Some("web-platform.test")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
        {
            return Err(NetworkError::Other(
                "unsupported offline WPT origin/method/query".into(),
            ));
        }
        let decoded = percent_encoding::percent_decode_str(url.path())
            .decode_utf8()
            .map_err(|e| NetworkError::Other(e.to_string()))?;
        let path = self
            .root
            .join(decoded.trim_start_matches('/'))
            .canonicalize()
            .map_err(NetworkError::Io)?;
        if !path.starts_with(&self.root) || !path.is_file() {
            return Err(NetworkError::Other(
                "resource outside regular WPT files".into(),
            ));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(NetworkError::Io)?
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(NetworkError::Io)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(NetworkError::Other("WPT resource exceeds 32MiB".into()));
        }
        let content_type = match path.extension().and_then(|e| e.to_str()) {
            Some("css") => Some("text/css"),
            Some("html" | "htm") => Some("text/html"),
            Some("ttf") => Some("font/ttf"),
            Some("otf") => Some("font/otf"),
            Some("woff") => Some("font/woff"),
            Some("woff2") => Some("font/woff2"),
            _ => None,
        };
        Ok(FetchedResource {
            bytes: bytes.into(),
            content_type: content_type.map(str::to_owned),
            final_url: url.clone(),
            encoding: None,
        })
    }
    fn load(&self, url: Url) -> Result<FetchedResource, String> {
        self.fetch(Request {
            url,
            method: Method::Get,
            content_type: None,
            headers: Vec::new(),
            body: Body::Empty,
            signal: None,
            kind: ResourceKind::Other,
        })
        .map_err(|e| e.to_string())
    }
}
impl NetworkProvider for Resources {
    fn fetch_one_hop(&self, request: Request) -> Result<FetchOutcome, NetworkError> {
        let result = self.read(&request);
        let record = match &result {
            Ok(resource) => ResourceRecord {
                url: request.url.to_string(),
                kind: format!("{:?}", request.kind),
                bytes: Some(resource.bytes.len()),
                sha256: Some(format!("{:x}", Sha256::digest(&resource.bytes))),
                error: None,
            },
            Err(error) => ResourceRecord {
                url: request.url.to_string(),
                kind: format!("{:?}", request.kind),
                bytes: None,
                sha256: None,
                error: Some(error.to_string()),
            },
        };
        self.records
            .lock()
            .map_err(|_| NetworkError::Other("resource trace poisoned".into()))?
            .push(record);
        result.map(FetchOutcome::Body)
    }
}

pub fn parse_screen(root: &Path, id: &str) -> Result<ScreenInput, String> {
    let resources = Resources::new(root)?;
    let url = Url::parse("http://web-platform.test/")
        .unwrap()
        .join(id)
        .map_err(|e| e.to_string())?;
    let resource = resources.load(url)?;
    let parsed = raikiri_html::parse(
        resource.bytes.as_ref(),
        &raikiri_html::ParseOptions {
            extra_stylesheets: &[],
            network: Some(&resources),
            base_url: Some(resource.final_url),
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let records = resources
        .records
        .into_inner()
        .map_err(|_| "resource trace poisoned")?;
    if let Some(failed) = records.iter().find(|r| r.error.is_some()) {
        return Err(format!(
            "original WPT resource failed: {}: {}",
            failed.url,
            failed.error.as_deref().unwrap()
        ));
    }
    let cascade = raikiri_html::build_cascaded_with_media_context(
        &parsed,
        &raikiri_style::MediaContext::screen(),
    );
    Ok(ScreenInput {
        parsed,
        cascade,
        resources: records,
    })
}

/// Verify all resources retained by the original replay, including fonts not
/// needed by this cascade-only diagnostic. Never substitute fixture assets.
pub fn verify_original_resources(root: &Path, expected: &[ResourceRecord]) -> Result<(), String> {
    if expected.is_empty() {
        return Err("original resource evidence is missing".into());
    }
    let resources = Resources::new(root)?;
    for record in expected {
        if record.error.is_some() || record.bytes.is_none() || record.sha256.is_none() {
            return Err(format!(
                "original resource evidence is incomplete: {}",
                record.url
            ));
        }
        let resource = resources.load(Url::parse(&record.url).map_err(|e| e.to_string())?)?;
        if record.bytes != Some(resource.bytes.len())
            || record.sha256.as_deref()
                != Some(format!("{:x}", Sha256::digest(&resource.bytes)).as_str())
        {
            return Err(format!("original resource bytes changed: {}", record.url));
        }
    }
    Ok(())
}

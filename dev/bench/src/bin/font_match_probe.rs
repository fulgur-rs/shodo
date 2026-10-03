use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions, FontQuery};
use shodo::limits::Limits;
use shodo::style::FontFamily;
use shodo_fixtures::font;
use std::error::Error;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const SIZES: [usize; 6] = [1, 4, 16, 64, 128, 240];

#[derive(Clone, Copy)]
enum Workload {
    Registered,
    Native,
}

impl Workload {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "registered" => Ok(Self::Registered),
            "native" => Ok(Self::Native),
            _ => Err("workload must be `registered` or `native`".into()),
        }
    }

    fn cluster(self) -> &'static str {
        match self {
            Self::Registered => "水", // Miss every fixed Latin face after named lookup.
            Self::Native => "a",
        }
    }

    fn query(self, families: &[String]) -> FontQuery {
        let families = match self {
            Self::Registered => families.iter().cloned().map(FontFamily::Named).collect(),
            Self::Native => vec![FontFamily::Named(
                font("latin")
                    .expect("fixed Latin fixture exists")
                    .family
                    .into(),
            )],
        };
        FontQuery {
            families,
            ..Default::default()
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Native => "native",
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--describe"] {
        println!(
            "{}",
            serde_json::json!({
                "workloads": ["registered", "native"],
                "face_counts": SIZES,
                "registered_cluster": "水",
                "native_cluster": "a"
            })
        );
        return Ok(());
    }
    if args.len() != 3 {
        return Err("usage: shodo-font-match-probe --cold|--memory registered|native FACES".into());
    }
    let mode = args[0].as_str();
    if !["--cold", "--memory"].contains(&mode) {
        return Err("unknown probe mode".into());
    }
    if (mode == "--cold" && cfg!(feature = "allocation-counting"))
        || (mode == "--memory" && !cfg!(feature = "allocation-counting"))
    {
        return Err("cold timing and allocator measurements require separate builds".into());
    }
    let workload = Workload::parse(&args[1])?;
    let face_count: usize = args[2].parse()?;
    if !SIZES.contains(&face_count) {
        return Err("face count must be one of 1, 4, 16, 64, 128 or 240".into());
    }

    let fixture = font("latin").ok_or("fixed Latin fixture is missing")?;
    let family_names: Vec<_> = (0..face_count)
        .map(|index| format!("Shodo Probe Face {index}"))
        .collect();
    let face_bytes: Vec<_> = (0..face_count).map(|_| fixture.bytes.to_vec()).collect();
    let query = workload.query(&family_names);
    let cluster = workload.cluster();
    let limits = Limits::default();

    #[cfg(not(feature = "allocation-counting"))]
    let report = cold(
        workload,
        &family_names,
        face_bytes,
        &query,
        cluster,
        &limits,
    )?;
    #[cfg(feature = "allocation-counting")]
    let report = memory(
        workload,
        &family_names,
        face_bytes,
        &query,
        cluster,
        &limits,
    )?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn build_collection(
    workload: Workload,
    family_names: &[String],
    face_bytes: Vec<Vec<u8>>,
    limits: &Limits,
    match_cache_entries: usize,
) -> Result<(FontCollection, u128), Box<dyn Error>> {
    let start = Instant::now();
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            match_cache_entries,
            ..Default::default()
        },
    );
    for (index, bytes) in face_bytes.into_iter().enumerate() {
        match workload {
            Workload::Registered => {
                fonts.register_face(
                    bytes,
                    0,
                    FontFaceDescriptor {
                        family: family_names[index].clone(),
                        ..Default::default()
                    },
                )?;
            }
            Workload::Native => {
                fonts.register(bytes)?;
            }
        }
    }
    Ok((fonts, start.elapsed().as_nanos()))
}

fn match_face(fonts: &FontCollection, query: &FontQuery, cluster: &str) -> Option<u32> {
    fonts
        .match_cluster(query, cluster)
        .map(|found| found.id.index())
}

#[cfg(not(feature = "allocation-counting"))]
fn cold(
    workload: Workload,
    family_names: &[String],
    face_bytes: Vec<Vec<u8>>,
    query: &FontQuery,
    cluster: &str,
    limits: &Limits,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let (fonts, registration_ns) = build_collection(workload, family_names, face_bytes, limits, 0)?;
    let start = Instant::now();
    let face_slot = match_face(&fonts, query, cluster);
    let match_ns = start.elapsed().as_nanos();
    Ok(serde_json::json!({
        "schema": 1,
        "mode": "cold",
        "settings": {
            "workload": workload.id(),
            "face_count": family_names.len(),
            "query_family_count": query.families.len(),
            "cluster": cluster,
            "font": "Shodo Fixture Latin"
        },
        "durations_ns": {
            "registration": registration_ns,
            "match_cache_miss": match_ns
        },
        "matched_face_slot": face_slot
    }))
}

#[cfg(feature = "allocation-counting")]
fn measured<T>(
    f: impl FnOnce() -> Result<T, Box<dyn Error>>,
) -> Result<(T, shodo_bench::allocator::AllocationCounts), Box<dyn Error>> {
    let scope = ALLOCATOR.begin()?;
    let result = f()?;
    let counts = scope.finish();
    Ok((result, counts))
}

#[cfg(feature = "allocation-counting")]
fn memory(
    workload: Workload,
    family_names: &[String],
    face_bytes: Vec<Vec<u8>>,
    query: &FontQuery,
    cluster: &str,
    limits: &Limits,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let ((fonts, registration_ns), registration) = measured(|| {
        Ok(build_collection(
            workload,
            family_names,
            face_bytes,
            limits,
            8,
        )?)
    })?;
    let (cold_result, cold_match) = measured(|| Ok(match_face(&fonts, query, cluster)))?;
    let (warm_query, warm_cluster) = match workload {
        Workload::Registered => (
            FontQuery {
                families: vec![FontFamily::Named(
                    family_names.last().ok_or("no family names")?.clone(),
                )],
                ..Default::default()
            },
            "a",
        ),
        Workload::Native => (query.clone(), "a"),
    };
    let primed_result = match_face(&fonts, &warm_query, warm_cluster);
    if primed_result.is_none() {
        return Err("warm-hit workload did not find its fixed Latin face".into());
    }
    let (warm_result, warm_hit) = measured(|| Ok(match_face(&fonts, &warm_query, warm_cluster)))?;
    if primed_result != warm_result {
        return Err("warm result differs from primed match".into());
    }
    Ok(serde_json::json!({
        "schema": 1,
        "mode": "memory",
        "settings": {
            "workload": workload.id(),
            "face_count": family_names.len(),
            "query_family_count": query.families.len(),
            "cluster": cluster,
            "font": "Shodo Fixture Latin"
        },
        "durations_ns": {"registration": registration_ns},
        "matched_face_slot": cold_result,
        "warm_match_face_slot": warm_result,
        "scopes": {
            "registration": registration,
            "match_cache_miss": cold_match,
            "warm_cache_hit": warm_hit
        },
        "scope_notes": "Family/query strings and font byte copies are prepared outside allocation scopes. Registration includes FontCollection and index storage. The cold match is measured separately. The warm-hit scope uses a one-family key to stay within the bounded match cache. Counts are requested Rust allocator blocks, not RSS."
    }))
}

//! Isolated eviction policies: scan|heap|clock CAP cold|mixed ITERATIONS SAMPLES.
//! No font lookup, query interning or locks occur in this probe.
#[path = "match_eviction_algorithms/eviction.rs"]
mod eviction;
use std::{
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Instant,
};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

enum Policy {
    Scan,
    Heap(eviction::Eviction),
    Clock { next: usize, seen: Vec<u64> },
}
impl Policy {
    fn new(name: &str, cap: usize) -> Self {
        match name {
            "scan" => Self::Scan,
            "heap" => {
                let mut h = eviction::Eviction::default();
                for i in 0..cap {
                    h.push(i as u32, i as u64 + 1);
                }
                Self::Heap(h)
            }
            "clock" => Self::Clock {
                next: 0,
                seen: (1..=cap as u64).collect(),
            },
            _ => panic!("scan|heap|clock"),
        }
    }
    fn victim<const COUNT: bool>(&mut self, used: &[AtomicU64], probes: &mut u64) -> usize {
        let mut load = |slot: usize| {
            if COUNT {
                *probes += 1;
            }
            used[slot].load(Relaxed)
        };
        match self {
            Self::Scan => (0..used.len()).min_by_key(|i| load(*i)).unwrap(),
            Self::Heap(h) => h.victim(|i| load(i as usize)).unwrap() as usize,
            Self::Clock { next, seen } => loop {
                let at = *next;
                *next = (at + 1) % used.len();
                let stamp = load(at);
                if seen[at] == stamp {
                    break at;
                }
                seen[at] = stamp;
            },
        }
    }
    fn inserted(&mut self, slot: usize, stamp: u64) {
        match self {
            Self::Heap(h) => h.update(slot as u32, stamp),
            Self::Clock { seen, .. } => seen[slot] = stamp,
            Self::Scan => {}
        }
    }
}
fn run<const COUNT: bool>(
    policy: &mut Policy,
    used: &[AtomicU64],
    mixed: bool,
    n: usize,
) -> (u64, u64) {
    let mut stamp = used.len() as u64;
    let mut probes = 0;
    let mut digest = 0u64;
    let mut random = 17u64;
    for _ in 0..n {
        if mixed {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            stamp += 1;
            used[random as usize % used.len()].store(stamp, Relaxed);
        }
        let slot = policy.victim::<COUNT>(used, &mut probes);
        stamp += 1;
        used[slot].store(stamp, Relaxed);
        policy.inserted(slot, stamp);
        digest = digest.wrapping_add(slot as u64);
    }
    (black_box(digest), probes)
}
fn main() {
    let a: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(a.len(), 5);
    let cap: usize = a[1].parse().unwrap();
    let n: usize = a[3].parse().unwrap();
    let samples: usize = a[4].parse().unwrap();
    assert!(cap > 0 && cap <= u32::MAX as usize && n > 0 && samples > 0);
    assert!(["cold", "mixed"].contains(&a[2].as_str()));
    let fresh = || (1..=cap as u64).map(AtomicU64::new).collect::<Vec<_>>();
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOC.begin().unwrap();
    let policy = Policy::new(&a[0], cap);
    #[cfg(feature = "allocation-counting")]
    let retained = scope.finish();
    drop(policy);
    let mut ns = Vec::with_capacity(samples);
    let mut digests = Vec::with_capacity(samples);
    for _ in 0..samples {
        let used = fresh();
        let mut policy = Policy::new(&a[0], cap);
        let start = Instant::now();
        let (digest, _) = run::<false>(&mut policy, &used, a[2] == "mixed", n);
        ns.push(start.elapsed().as_nanos());
        digests.push(digest);
    }
    let (_, probes) = run::<true>(&mut Policy::new(&a[0], cap), &fresh(), a[2] == "mixed", n);
    let report = serde_json::json!({"policy":a[0],"cap":cap,"case":a[2],"iterations":n,"samples":samples,"elapsed_ns":ns,"digests":digests,"stamp_loads":probes});
    #[cfg(feature = "allocation-counting")]
    let mut report = report;
    #[cfg(feature = "allocation-counting")]
    {
        report["retained_policy"] = serde_json::to_value(retained).unwrap();
    }
    println!("{report}");
}

"""Pure helpers for the shodo-jt1 investigation: paired warm-median ratios over
probe time reports, and flat perf attribution by module. No I/O beyond text."""
import re
import statistics

WINDOWS = {
    "pipeline": "parse_cascade_layout",
    "layout": "layout",
    "isolated": "initial_text_pipeline",
}


MEMORY_WINDOWS = {"pipeline": "parse_cascade_layout", "layout": "layout"}
MEMORY_FIELDS = ("allocated_bytes", "calls", "peak_extra_bytes", "net_bytes")


def window_memory(report, operation):
    """Median over warm samples of the allocation counters of one memory-mode
    window: requested bytes (allocated_bytes), allocation calls, the peak of
    live bytes above the window start (peak_extra_bytes) and retained net
    bytes (net_bytes, may be negative). These are allocator-requested heap
    accounting, not RSS or resident pages. Sample 0 (first call) is skipped."""
    key = MEMORY_WINDOWS[operation]
    samples = report["samples"]
    if len(samples) < 2 or samples[0].get("state") != "first-call-in-process":
        raise ValueError("expected a first-call-in-process sample followed by warm samples")
    if any(s.get("state") != "warm-process" for s in samples[1:]):
        raise ValueError("samples after the first call must be warm-process")
    result = {f: statistics.median(s[key]["counts"][f] for s in samples[1:]) for f in MEMORY_FIELDS}
    result["warm_samples"] = len(samples) - 1
    return result


def warm_samples_ns(report, operation):
    """Warm-call durations of one time-mode report. Sample 0 is the first call
    in the process (not cold startup) and is never included."""
    key = WINDOWS[operation]
    samples = report["samples"]
    if len(samples) < 2 or samples[0].get("state") != "first-call-in-process":
        raise ValueError("expected a first-call-in-process sample followed by warm samples")
    if any(s.get("state") != "warm-process" for s in samples[1:]):
        raise ValueError("samples after the first call must be warm-process")
    return [s[key]["duration_ns"] for s in samples[1:]]


def process_median_ns(report, operation):
    return statistics.median(warm_samples_ns(report, operation))


def paired_summary(native, candidate):
    """Pair values by position (the same repeat), never by sorted order."""
    if not native or len(native) != len(candidate):
        raise ValueError("native and candidate need the same non-zero number of pairs")
    ratios = [c / n for n, c in zip(native, candidate)]
    return {
        "pairs": len(ratios),
        "warm_process_median_ns": {
            "native": statistics.median(native),
            "candidate": statistics.median(candidate),
        },
        "paired_ratio_median": statistics.median(ratios),
        "paired_ratio_min": min(ratios),
        "paired_ratio_max": max(ratios),
        "pairs_candidate_slower": sum(1 for r in ratios if r > 1),
        "paired_ratios": ratios,
    }


_ROW = re.compile(r"^\s*(\d+)\s+\[\.\]\s+(.+?)\s*$")


def perf_samples(text):
    """(sample count, symbol) rows of `perf report --stdio --no-children`
    user-space entries; every other line is ignored."""
    rows = []
    for line in text.splitlines():
        match = _ROW.match(line)
        if match:
            rows.append((int(match.group(1)), match.group(2)))
    return rows


KNOWN_CRATES = [
    "parley", "harfrust", "fontique", "skrifa", "read_fonts", "raikiri_html",
    "raikiri_style", "raikiri_dom", "raikiri_traits", "cssparser", "html5ever",
    "selectors", "icu_segmenter", "icu_properties", "icu_normalizer", "icu_casemap",
    "icu_collections", "icu_provider", "smol_str", "taffy",
]
_RUNTIME = re.compile(r"\b(core|alloc|std)::|^(memcpy|memmove|memset|malloc|free|realloc|calloc|cfree|_int_\w+|__\w+)")


def bucket(symbol):
    """Coarse owner of a symbol: shodo::<module>, a known crate, runtime, or other.
    Generic instantiations attribute to the first shodo path in the symbol, so
    this is an approximation, not a call-graph attribution."""
    match = re.search(r"\bshodo::(\w+)", symbol)
    if match:
        return f"shodo::{match.group(1)}"
    for crate in KNOWN_CRATES:
        if re.search(rf"\b{crate}::", symbol):
            return crate
    if _RUNTIME.search(symbol):
        return "runtime"
    return "other"


def aggregate(rows):
    totals = {}
    for count, symbol in rows:
        name = bucket(symbol)
        totals[name] = totals.get(name, 0) + count
    return totals

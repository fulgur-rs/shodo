"""Summarize shodo-b7d samples: per-round medians, deltas and doubling ratios.

Usage: summarize.py <samples.jsonl> > summary.json
"""
import collections, json, statistics, sys

rows = [json.loads(line) for line in open(sys.argv[1])]
groups = collections.defaultdict(list)
digests = collections.defaultdict(set)
reps = {}
for r in rows:
    groups[(r["case"], r["size"], r["order"], r["round"], r["label"])].append(r["break_ns_per_call"])
    digests[(r["case"], r["size"])].add((r["output_sha256"], r["warning_sha256"]))
    reps[(r["case"], r["size"])] = r["reps"]
incomplete = [k for k, v in groups.items() if len(v) != 2]
if incomplete:
    sys.exit(f"expected 2 samples per label per round, got {[(k, len(groups[k])) for k in sorted(incomplete)]}")
for (case, size, order, n) in {k[:4] for k in groups}:
    for label in ("baseline", "candidate"):
        if (case, size, order, n, label) not in groups:
            sys.exit(f"missing {label} samples for {case} {size} {order} round {n}")
order_of_cases = ["nested", "nestedtab", "siblings", "siblingstab", "plain"]
summary = {
    "digest_mismatch": [f"{c}:{s}" for (c, s), d in sorted(digests.items()) if len(d) != 1],
    "cases": [],
}
for case, size in sorted({(r["case"], r["size"]) for r in rows}, key=lambda k: (order_of_cases.index(k[0]), k[1])):
    entry = {"case": case, "size": size, "reps": reps[(case, size)]}
    for order in ("ABBA", "BAAB"):
        rounds = sorted({k[3] for k in groups if k[:3] == (case, size, order)})
        base = [statistics.mean(groups[(case, size, order, n, "baseline")]) for n in rounds]
        cand = [statistics.mean(groups[(case, size, order, n, "candidate")]) for n in rounds]
        bm, cm = statistics.median(base), statistics.median(cand)
        entry[order] = {
            "rounds": len(rounds),
            "baseline_median_ns": bm,
            "candidate_median_ns": cm,
            "delta_pct": (cm - bm) / bm * 100,
            "candidate_faster_rounds": sum(c < b for b, c in zip(base, cand)),
            "candidate_slower_rounds": sum(c > b for b, c in zip(base, cand)),
            "ratio_median": statistics.median(c / b for b, c in zip(base, cand)),
            "ratio_max": max(c / b for b, c in zip(base, cand)),
        }
    summary["cases"].append(entry)
for case in ("nested", "nestedtab", "siblings", "siblingstab"):
    for label in ("baseline", "candidate"):
        for order in ("ABBA", "BAAB"):
            v = {e["size"]: e[order][f"{label}_median_ns"] for e in summary["cases"] if e["case"] == case}
            sizes = sorted(v)
            summary[f"{case}_{label}_{order}_doubling_ratios"] = [v[b] / v[a] for a, b in zip(sizes, sizes[1:])]
json.dump(summary, sys.stdout, indent=2)

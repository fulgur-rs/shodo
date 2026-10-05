"""Print the shodo-2j6 record's time and doubling tables from summary.json.

Usage: tables.py <summary.json>
"""
import json, sys

def fmt(ns):
    if ns >= 1e9:
        return f"{ns / 1e9:.2f} s"
    if ns >= 1e6:
        return f"{ns / 1e6:.2f} ms"
    return f"{ns / 1e3:.1f} µs"

def pct(value):
    return f"{value:+.1f}%".replace("-", "−")

s = json.load(open(sys.argv[1]))
print("| ケース | size | reps | baseline (ABBA) | candidate (ABBA) | ABBA Δ | ABBA 速い比較 | baseline (BAAB) | candidate (BAAB) | BAAB Δ | BAAB 速い比較 |")
print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
for e in s["cases"]:
    cells = [e["case"], str(e["size"]), str(e["reps"])]
    for order in ("ABBA", "BAAB"):
        o = e[order]
        cells += [fmt(o["baseline_median_ns"]), fmt(o["candidate_median_ns"]), pct(o["delta_pct"]), f"{o['candidate_faster_rounds']}/{o['rounds']}"]
    print("| " + " | ".join(cells) + " |")
print()
print("| ケース | 側 | ABBA | BAAB |")
print("| --- | --- | --- | --- |")
for case in ("siblings", "outer", "valign", "rtl"):
    for label in ("baseline", "candidate"):
        a = s[f"{case}_{label}_ABBA_doubling_ratios"]
        b = s[f"{case}_{label}_BAAB_doubling_ratios"]
        print(f"| {case} | {label} | {' / '.join(f'{r:.2f}' for r in a)} | {' / '.join(f'{r:.2f}' for r in b)} |")

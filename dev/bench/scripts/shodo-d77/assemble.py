"""Assemble the shodo-d77 record JSON.

Usage: assemble.py <decision> <candidate commit> <measure dir> <baseline worktree> <output json>
<measure dir> holds binaries.txt (`sha256sum` output of both binaries), summary.json
(from summarize.py), counts.jsonl (from the d77_operation_counts_report test) and
samples.jsonl (from run.sh).
"""
import json, subprocess, sys

decision, candidate_commit, measure, baseline_worktree, output = sys.argv[1:6]
def run(*cmd):
    return subprocess.run(cmd, capture_output=True, text=True, check=True).stdout.strip()

binaries = dict(reversed(line.split()) for line in open(f"{measure}/binaries.txt"))
summary = json.load(open(f"{measure}/summary.json"))
data = {
    "issue": "shodo-d77",
    "decision": decision,
    "baseline_commit": run("git", "-C", baseline_worktree, "rev-parse", "HEAD"),
    "candidate_commit": candidate_commit,
    "candidate_change": "Adjustment-only ruby candidates memoize the container core per operation keyed by (dataset, atomic revision, start, look-ahead endpoint) and reuse it only when replaying its recorded saturation and aggregated reshape-budget charges is exact; the look-ahead walk resumes for growing ends; one candidate shares its selected line profile across containers; metric and neighbor indexes use value slots.",
    "binaries": {
        "baseline_sha256": next(v for k, v in binaries.items() if "baseline" in k),
        "candidate_sha256": next(v for k, v in binaries.items() if "candidate" in k),
        "note": "Separate release builds with separate target directories; the baseline worktree is a93a359 plus the probe example and its [[example]] entry.",
    },
    "environment": {
        "rustc": run("rustc", "--version"),
        "cpu": run("sh", "-c", "lscpu | sed -n 's/^Model name: *//p'"),
        "kernel": run("uname", "-sr"),
        "governor": run("cat", "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
    },
    "summary": summary,
    "operation_counts": [json.loads(line) for line in open(f"{measure}/counts.jsonl") if line.startswith("{")],
    "samples": [json.loads(line) for line in open(f"{measure}/samples.jsonl")],
}
json.dump(data, open(output, "w"), indent=2, ensure_ascii=False)

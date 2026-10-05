"""Assemble the shodo-b7d record JSON.

Usage: assemble.py <decision> <candidate commit> <measure dir> <baseline worktree> <output json>
<measure dir> holds binaries.txt (`sha256sum` output of both binaries, which sit in
directories named exactly `baseline` and `candidate`), summary.json
(from summarize.py), counts.jsonl (from the b7d_operation_counts_report test) and
samples.jsonl (from run.sh).
"""
import json, os, subprocess, sys

decision, candidate_commit, measure, baseline_worktree, output = sys.argv[1:6]
def run(*cmd):
    return subprocess.run(cmd, capture_output=True, text=True, check=True).stdout.strip()

binaries = {}
for line in open(f"{measure}/binaries.txt"):
    digest, path = line.split(maxsplit=1)
    label = os.path.basename(os.path.dirname(path.strip()))
    if label in binaries:
        sys.exit(f"binaries.txt: more than one {label} binary")
    binaries[label] = digest
if set(binaries) != {"baseline", "candidate"}:
    sys.exit(f"binaries.txt: expected baseline/ and candidate/ binaries, got {sorted(binaries)}")
summary = json.load(open(f"{measure}/summary.json"))
data = {
    "issue": "shodo-b7d",
    "decision": decision,
    "baseline_commit": run("git", "-C", baseline_worktree, "rev-parse", "HEAD"),
    "candidate_commit": candidate_commit,
    "candidate_change": "The line::range::width tab prefix is sparse (only tab steps are stored) and the range cache generation moves only when a tab step changes Saturation (fill when computing it, invalidate when discarding a prefix that holds one); tab charging is unchanged, so a new range start no longer invalidates the cache epoch.",
    "binaries": {
        "baseline_sha256": binaries["baseline"],
        "candidate_sha256": binaries["candidate"],
        "note": "Separate release builds with separate target directories; the baseline worktree is 1b2e1b7 plus the probe example and its [[example]] entry.",
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

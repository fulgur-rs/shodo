#!/usr/bin/env python3
"""Exercise the real original-input paged caller, never a synthetic paragraph.

The four original 16px lines must survive actual height rejection and page
movement. Returning static layout, losing rejected source, or ignoring body
margin would break the literal candidate fragment expectations below.
"""
import argparse
import json
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("mode", choices=["time", "memory"])
    parser.add_argument("wpt", type=Path)
    parser.add_argument("selection", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    case = "css/css-text/hyphens/reference/hyphens-none-shy-on-2nd-line-001-ref.html"
    runs = []
    for engine in ["candidate", "native"]:
        output = args.output / f"{engine}.json"
        argv = [str(args.binary), args.mode, engine, str(args.wpt),
                str(args.selection), case, str(output), "pagination-check"]
        run = subprocess.run(argv, capture_output=True, text=True)
        (args.output / f"{engine}.log").write_text(run.stdout + run.stderr)
        assert run.returncode == 0, (engine, run.returncode, run.stderr)
        report = json.loads(output.read_text())
        assert report["operation"] == "provisional-complete-caller-pagination"
        assert report["correctness_only"] is True
        assert report["fresh_fragment_outputs_verified"] is True
        assert report["height_sequence_css_px"] == [128, 64, 32, 128]
        assert len(report["samples"]) == 10
        if args.mode == "memory":
            def net_bytes(value):
                if isinstance(value, dict):
                    if set(value) == {"counts"}:
                        return value["counts"]["net_bytes"]
                    return sum(net_bytes(item) for item in value.values())
                if isinstance(value, list):
                    return sum(net_bytes(item) for item in value)
                return 0
            # Missing cache preconditioning or freeing an owner allocated
            # outside its measured window makes the real lifecycle unbalanced.
            assert net_bytes(report) == 0, (engine, "unbalanced measured owners", net_bytes(report))
        for sample in report["samples"]:
            constrained = sample["complete_caller_height_reentry"][1]["output"]
            assert constrained["source_partition_complete"] is True
            assert constrained["page_count"] >= 2
            if engine == "candidate":
                assert constrained["actual_height_rejections"] > 0
                assert constrained["actual_lookahead_rejections"] > 0
                assert constrained["partition"] == [
                    {"root": 7, "page": 1, "line_start": 0, "line_end": 2},
                    {"root": 7, "page": 2, "line_start": 2, "line_end": 4},
                ]
        runs.append({"engine": engine, "argv": argv, "returncode": run.returncode})
    (args.output / "manifest.json").write_text(json.dumps({"runs": runs}, indent=2) + "\n")


if __name__ == "__main__":
    main()

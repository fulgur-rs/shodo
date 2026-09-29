#!/usr/bin/env python3
"""Require real fresh library construction, unchanged output and owner balance."""
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
    parser.add_argument("references", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    case = "css/css-text/white-space/lone-cr-001-ref.html"
    runs = []
    for engine in ["candidate", "native"]:
        destination = args.output / f"{engine}.json"
        argv = [str(args.binary), args.mode, engine, str(args.wpt), str(args.selection), case, str(destination)]
        run = subprocess.run(argv, capture_output=True, text=True)
        destination.with_suffix(".log").write_text(run.stdout + run.stderr)
        assert run.returncode == 0, (engine, run.stderr)
        value = json.loads(destination.read_text())
        assert value["operation"] == "initial-library-construction-phases", "initial library work is not separated from DOM/CSS preparation"
        reference = json.loads((args.references / f"isolated-{engine}-096.json").read_text())
        assert value["initial_output"] == reference["initial_output"]
        assert len(value["samples"]) == 10
        for sample in value["samples"]:
            paired = [row for row in sample["exclusive_phases"] if row["kind"] == "library" and row["paired_root"] == 7]
            assert len(paired) == 1, "real original paired library call missing/duplicated"
            assert paired[0]["input_bytes"] > 0
            assert sample["output"] == value["initial_output"]
        if args.mode == "memory":
            def net(v):
                if isinstance(v, dict):
                    if set(v) == {"counts"}: return v["counts"]["net_bytes"]
                    return sum(net(x) for x in v.values())
                return sum(net(x) for x in v) if isinstance(v, list) else 0
            assert net(value) == 0, (engine, "unbalanced real library/preparation/cache owners", net(value))
        runs.append({"engine": engine, "argv": argv, "returncode": run.returncode})
    (args.output / "manifest.json").write_text(json.dumps({"runs": runs}, indent=2) + "\n")


if __name__ == "__main__":
    main()

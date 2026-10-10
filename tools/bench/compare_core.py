#!/usr/bin/env python3
"""Measure the fixed-font core example and retain reproducible provenance."""
import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import run as runner


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    if args.output.exists():
        raise FileExistsError(args.output)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=args.output.name + ".stage-", dir=args.output.parent))
    try:
        env = os.environ.copy()
        runner.require(not any(env.get(key) for key in ("RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")),
                       "compiler overrides are unsupported; unset RUSTC and compiler wrappers")
        env.update(CARGO_BUILD_JOBS="1", CARGO_PROFILE_RELEASE_OPT_LEVEL="3",
                   CARGO_PROFILE_RELEASE_DEBUG="0", CARGO_PROFILE_RELEASE_INCREMENTAL="false",
                   CARGO_INCREMENTAL="0")
        paths = [runner.ROOT / "dev/raikiri/examples/core_compare.rs",
                 runner.ROOT / "dev/raikiri/Cargo.toml", Path(__file__).resolve()]
        paths += list((runner.ROOT / "dev/fixtures/assets/fonts").glob("*"))
        before = dict(**runner.source_hashes(), comparison_hash=runner.tree_hash(paths))
        configuration = runner.build_configuration(runner.ROOT, env)
        runner.ensure_lock(runner.ROOT, stage / "resolve.log", env)
        lock_hash = runner.file_hash(runner.ROOT / "Cargo.lock")
        profiles = {}
        output = runner.execute(["cargo", "+stable", "build", "--locked", "--release",
                                 "-p", "shodo-raikiri", "--example", "core_compare",
                                 "--features", "shodo/complex-scripts", "--message-format=json"],
                                stage / "build.log", env=env)
        binary = stage / "core_compare"
        shutil.copy2(runner.executable_from_cargo(output, "core_compare", "example", profiles=profiles), binary)
        runner.execute([binary, stage / "results.json"], stage / "measurement.log", env=env)
        after = dict(**runner.source_hashes(), comparison_hash=runner.tree_hash(paths))
        runner.require(before == after, "comparison source changed during measurement")
        runner.require(configuration == runner.build_configuration(runner.ROOT, env), "build configuration changed")
        runner.require(lock_hash == runner.file_hash(runner.ROOT / "Cargo.lock"), "lockfile changed")
        report = json.loads((stage / "results.json").read_text())
        runner.require(len(report["cases"]) == 12, "missing fixture cases")
        report["metadata"] = dict(
            revision=runner.execute(["git", "rev-parse", "HEAD"], stage / "revision.log").strip(),
            dirty_status=runner.execute(["git", "status", "--porcelain"], stage / "status.log").splitlines(),
            **before, binary_sha256=runner.file_hash(binary), lock_sha256=lock_hash,
            source_fingerprint_version=runner.SOURCE_FINGERPRINT_VERSION,
            features=["shodo/complex-scripts"],
            rustc=runner.execute(["rustc", "+stable", "-Vv"], stage / "rustc.log").strip(),
            cargo=runner.execute(["cargo", "+stable", "-V"], stage / "cargo.log").strip(),
            os=platform.platform(), affinity=sorted(os.sched_getaffinity(0)),
            cpu_models=sorted({line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")}),
            profiles=profiles, build_configuration=configuration,
            versions=dict(shodo=tomllib.loads((runner.ROOT / "crates/shodo/Cargo.toml").read_text())["package"]["version"],
                          parley=next(package["version"] for package in tomllib.loads((runner.ROOT / "Cargo.lock").read_text())["package"] if package["name"] == "parley")),
        )
        (stage / "results.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
        shutil.copy2(runner.ROOT / "Cargo.lock", stage / "Cargo.lock")
        stage.rename(args.output)
        print(args.output)
    except BaseException as error:
        if isinstance(error, subprocess.CalledProcessError):
            print(error.stdout or "", file=sys.stderr)
            print(error.stderr or "", file=sys.stderr)
        shutil.rmtree(stage)
        raise


if __name__ == "__main__":
    main()

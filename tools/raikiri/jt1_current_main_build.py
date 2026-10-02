#!/usr/bin/env python3
"""Build hash-pinned current-main probes from the saved S4 caller archive."""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tomllib
from pathlib import Path

SAVED_S4_REVISION = "fe67a281210fbc52d22032911ac3584405aa8198"


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tree_sha256(root):
    digest = hashlib.sha256()
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(bytes.fromhex(sha256(path)))
    return digest.hexdigest()


def run(argv, env):
    subprocess.run(argv, check=True, env=env)


def patch_once(path, before, after):
    source = path.read_text()
    if source.count(before) != 1:
        raise ValueError(f"expected one patch target in {path}: {before}")
    path.write_text(source.replace(before, after, 1))


def locked_raikiri_revision(path):
    lock = tomllib.loads(Path(path).read_text())
    revisions = set()
    for package in lock.get("package", []):
        if package["name"].startswith("raikiri-"):
            source = package.get("source", "")
            if "github.com/fulgur-rs/raikiri.git" not in source:
                raise ValueError(f"unexpected source for {package['name']} in {path}")
            revisions.add(source.rsplit("#", 1)[-1])
    if not revisions:
        raise ValueError(f"no raikiri packages found in {path}")
    if len(revisions) != 1:
        raise ValueError(f"raikiri packages use multiple revisions in {path}: {sorted(revisions)}")
    return revisions.pop()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-archive", type=Path, required=True)
    parser.add_argument("--source-provenance", type=Path, required=True)
    parser.add_argument("--expected-source-tree-sha256", required=True)
    parser.add_argument("--shodo-worktree", type=Path, required=True)
    parser.add_argument("--shodo-revision", required=True)
    parser.add_argument("--raikiri-revision", required=True)
    parser.add_argument("--current-selection", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--rustc", default="rustc")
    parser.add_argument("--toolchain", default="1.96.0")
    args = parser.parse_args()

    worktree = args.shodo_worktree.resolve()
    source = args.source_archive.resolve()
    output_dir = args.output_dir.resolve()
    target_dir = args.target_dir.resolve()
    source_info = json.loads(args.source_provenance.read_text())
    if source_info.get("revision") != SAVED_S4_REVISION:
        raise SystemExit("saved S4 archive revision differs from the expected pin")
    source_cargo_lock = source_info.get("original_lock_sha256")
    if sha256(source / "s4" / "Cargo.lock") != source_cargo_lock:
        raise SystemExit("saved S4 source lockfile differs from its archive provenance")
    source_tree_sha = tree_sha256(source)
    if source_tree_sha != args.expected_source_tree_sha256:
        raise SystemExit("saved S4 source tree differs from the expected pin")

    head = subprocess.run(
        ["git", "-C", str(worktree), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if head != args.shodo_revision:
        # Permit later documentation-only commits while requiring the Shodo
        # source and dependency lock to remain byte-for-byte equal to the pin.
        subprocess.run(
            ["git", "-C", str(worktree), "diff", "--quiet", args.shodo_revision, "HEAD", "--", "Cargo.toml", "Cargo.lock", "crates/shodo"],
            check=True,
        )
    dirty = subprocess.run(
        ["git", "-C", str(worktree), "status", "--short", "--", "Cargo.toml", "Cargo.lock", "crates/shodo"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if dirty:
        raise SystemExit("Shodo source or workspace lock has uncommitted changes")
    subprocess.run(["git", "-C", str(worktree), "cat-file", "-e", f"{args.shodo_revision}^{{commit}}"], check=True)

    output_dir.mkdir(parents=True, exist_ok=True)
    if any(output_dir.iterdir()):
        raise SystemExit(f"output directory must be empty: {output_dir}")
    target_dir.mkdir(parents=True, exist_ok=True)
    build_root = output_dir
    shutil.copytree(source, build_root, dirs_exist_ok=True)
    shodo_path = (worktree / "crates" / "shodo").as_posix()

    patch_once(
        build_root / "Cargo.toml",
        'shodo = { path = "s4",',
        f'shodo = {{ path = "{shodo_path}",',
    )
    for manifest in (
        build_root / "s4" / "dev" / "bench" / "Cargo.toml",
        build_root / "s4" / "dev" / "fixtures" / "Cargo.toml",
        build_root / "s4" / "dev" / "raikiri" / "Cargo.toml",
    ):
        patch_once(
            manifest,
            'shodo = { path = "../..",',
            f'shodo = {{ path = "{shodo_path}",',
        )

    manifest = build_root / "Cargo.toml"
    manifest_text = manifest.read_text()
    for old, filename in (
        ("layout_check", "layout_check.rs"),
        ("core_contract_check", "core_contract_check.rs"),
        ("main", "main.rs"),
    ):
        pattern = rf'(?m)^path = "[^"]*/dev/bench/raikiri_probe/{re.escape(filename)}"$'
        replacement = f'path = "{(worktree / "dev" / "raikiri" / "probe" / filename).as_posix()}"'
        manifest_text, count = re.subn(pattern, replacement, manifest_text)
        if count != 1:
            raise SystemExit(f"expected one {old} probe source path in {manifest}")
    manifest.write_text(manifest_text)

    candidate_lock = build_root / "Cargo.lock"
    locked_revision = locked_raikiri_revision(candidate_lock)
    if locked_revision != args.raikiri_revision:
        raise SystemExit(f"candidate lock uses raikiri {locked_revision}, expected {args.raikiri_revision}")

    env = os.environ.copy()
    cleared_environment = {"CARGO_BUILD_TARGET", "CARGO_ENCODED_RUSTFLAGS"}
    for key in list(env):
        if key.startswith("CARGO_PROFILE_") or key in cleared_environment:
            env.pop(key)
    env.update(
        {
            "CARGO_TARGET_DIR": str(target_dir),
            "RUSTC": str(Path(args.rustc).resolve()) if Path(args.rustc).exists() else args.rustc,
            "RUSTUP_TOOLCHAIN": args.toolchain,
            "RUSTFLAGS": "-D warnings",
        }
    )
    common = [
        args.cargo,
        f"+{args.toolchain}",
        "build",
        "--offline",
        "--manifest-path",
        str(manifest),
        "--bin",
        "measurement-probe",
        "--release",
        "--config",
        "profile.release.opt-level=3",
        "--config",
        "profile.release.debug=0",
    ]
    run(common, env)
    time_binary = target_dir / "release" / "measurement-probe"
    time_sha = sha256(time_binary)
    saved_time = output_dir / "measurement-probe-time"
    shutil.copy2(time_binary, saved_time)

    run(common + ["--features", "allocation-counting"], env)
    memory_sha = sha256(time_binary)
    saved_memory = output_dir / "measurement-probe-memory"
    shutil.copy2(time_binary, saved_memory)
    candidate_lock = build_root / "Cargo.lock"
    shutil.copy2(candidate_lock, output_dir / "current-main-Cargo.lock")

    versions = {
        "cargo": subprocess.run([args.cargo, f"+{args.toolchain}", "--version"], check=True, capture_output=True, text=True, env=env).stdout.strip(),
        "rustc": subprocess.run([env["RUSTC"], "--version"], check=True, capture_output=True, text=True, env=env).stdout.strip(),
    }
    source_hashes = {
        path.relative_to(worktree).as_posix(): sha256(path)
        for path in sorted((worktree / "dev" / "raikiri" / "probe").glob("*.rs"))
    }
    patched_manifest_hashes = {
        path.relative_to(build_root).as_posix(): sha256(path)
        for path in (
            build_root / "Cargo.toml",
            build_root / "s4" / "dev" / "bench" / "Cargo.toml",
            build_root / "s4" / "dev" / "fixtures" / "Cargo.toml",
            build_root / "s4" / "dev" / "raikiri" / "Cargo.toml",
        )
    }
    provenance = {
        "source_archive_tree_sha256": source_tree_sha,
        "saved_s4_archive_sha256": source_info["archive_sha256"],
        "saved_s4_source_lock_sha256": source_cargo_lock,
        "saved_s4_shodo_revision": source_info["revision"],
        "current_shodo_revision": args.shodo_revision,
        "current_shodo_head_at_build": head,
        "current_shodo_feature": "default-features=false,complex-scripts",
        "raikiri_revision": args.raikiri_revision,
        "current_shodo_workspace_lock_sha256": sha256(worktree / "Cargo.lock"),
        "current_candidate_lock_sha256": sha256(candidate_lock),
        "current_candidate_manifest_hashes": patched_manifest_hashes,
        "current_selection_sha256": sha256(args.current_selection),
        "current_time_binary_sha256": time_sha,
        "current_memory_binary_sha256": memory_sha,
        "probe_source_hashes": source_hashes,
        "toolchain": versions,
        "release_profile": {
            "opt_level": "3",
            "debug": "0",
            "rustflags": "-D warnings",
        },
        "build_environment": {
            "rustup_toolchain": args.toolchain,
            "rustflags": "-D warnings",
            "cleared_environment_variables": [
                "CARGO_BUILD_TARGET",
                "CARGO_ENCODED_RUSTFLAGS",
                "CARGO_PROFILE_*",
            ],
        },
        "measurement_features": {
            "time": ["complex-scripts"],
            "memory": ["complex-scripts", "allocation-counting"],
        },
    }
    (output_dir / "current-main-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    print(f"built time probe {time_sha}")
    print(f"built memory probe {memory_sha}")
    print(f"wrote provenance to {output_dir / 'current-main-provenance.json'}")


if __name__ == "__main__":
    main()

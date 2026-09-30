"""Install the same release-mode Cursor probe into the two historical trees.

This is for shodo-8u4 measurements only. It keeps Cursor's test-only visit
counter out of the probe binary, so that the timed code matches production.
"""

from pathlib import Path
import shutil
import subprocess
import sys


ALLOWED_REVISIONS = {
    "c4e34bd9216464b52d85c46c2bee463121a80ae0",
    "edaebb3ac0c23b98cc0a061d0b4a2ad46df0beee",
}


def install(tree: Path) -> None:
    revision = subprocess.check_output(
        ["git", "-C", str(tree), "rev-parse", "HEAD"], text=True
    ).strip()
    if revision not in ALLOWED_REVISIONS:
        raise ValueError(f"unexpected historical revision: {revision}")
    manifest = tree / "Cargo.toml"
    cargo = manifest.read_text()
    if "bench-no-visits" in cargo:
        raise ValueError("probe is already installed")
    cargo = cargo.replace("[features]\n", "[features]\nbench-no-visits = []\n", 1)
    cargo = cargo.replace(
        "[dev-dependencies]\n", '[dev-dependencies]\nserde_json = "1"\n', 1
    )
    source = tree / "src/line/spacing_summary.rs"
    text = source.read_text()
    start = text.index("pub(super) struct Cursor")
    end = text.index("/// A balanced range index", start)
    cursor = text[start:end]
    if cursor.count("#[cfg(test)]") not in (6, 7):
        raise ValueError("unexpected Cursor test instrumentation")
    cursor = cursor.replace(
        "#[cfg(test)]", '#[cfg(all(test, not(feature = "bench-no-visits")))]'
    )
    text = text[:start] + cursor + text[end:]
    test_marker = "#[cfg(test)]\nmod tests"
    if text.count(test_marker) != 1:
        raise ValueError("unexpected test module layout")
    text = text.replace(
        test_marker,
        '#[cfg(all(test, not(feature = "bench-no-visits")))]\nmod tests',
        1,
    )
    text += '\n#[cfg(all(test, feature = "bench-no-visits"))]\nmod probe;\n'
    probe_dir = source.parent / "spacing_summary"
    probe_dir.mkdir(exist_ok=True)
    shutil.copyfile(Path(__file__).with_name("spacing_summary_probe.rs"), probe_dir / "probe.rs")
    manifest.write_text(cargo)
    source.write_text(text)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: install_spacing_summary_probe.py HISTORICAL_WORKTREE")
    install(Path(sys.argv[1]).resolve())

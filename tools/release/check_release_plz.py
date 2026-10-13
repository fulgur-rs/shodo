"""Exercise the installed release tool's default changelog without credentials."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def check(binary):
    binary = str(Path(binary).resolve())
    env = os.environ.copy()
    for name in ['GITHUB_TOKEN', 'GH_TOKEN', 'GIT_TOKEN', 'GIT_CLIFF_CONFIG']:
        env.pop(name, None)
    env.update(CARGO_NET_OFFLINE='true', GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')
    with tempfile.TemporaryDirectory(prefix='shodo-release-smoke-',
                                     dir=os.environ.get('TMPDIR')) as directory:
        root = Path(directory)
        env['XDG_CONFIG_HOME'] = str(root / 'config')
        current = root / 'current'
        baseline = root / 'baseline'
        (current / 'src').mkdir(parents=True)
        (current / 'Cargo.toml').write_text(
            '[package]\nname = "release-tool-smoke-test"\nversion = "0.0.1"\n'
            'edition = "2021"\n', encoding='utf-8')
        (current / 'src/lib.rs').write_text('pub fn answer() -> u8 { 1 }\n', encoding='utf-8')
        (current / 'release-plz.toml').write_text('[workspace]\nsemver_check = false\n', encoding='utf-8')

        def run(args, cwd=current):
            return subprocess.run(args, cwd=cwd, env=env, check=True,
                                  capture_output=True, text=True).stdout

        def git(*args):
            return run(['git', '-c', 'user.name=Release fixture',
                        '-c', 'user.email=release-fixture@example.invalid', *args])

        git('init', '--initial-branch=main')
        git('remote', 'add', 'origin', 'https://github.com/fulgur-rs/shodo')
        git('add', '.')
        git('commit', '-m', 'chore: initial package')
        run(['git', 'clone', '--local', '--no-hardlinks', str(current), str(baseline)], cwd=root)
        (current / 'src/lib.rs').write_text('pub fn answer() -> u8 { 2 }\n', encoding='utf-8')
        git('add', 'src/lib.rs')
        git('commit', '-m', 'fix: exercise release changelog')
        try:
            run([binary, 'update', '--registry-manifest-path', str(baseline / 'Cargo.toml'),
                 '--repo-url', 'https://github.com/fulgur-rs/shodo',
                 '--release-date', '2026-01-01'])
        except subprocess.CalledProcessError as error:
            raise RuntimeError(error.stdout + error.stderr) from error
        changelog = (current / 'CHANGELOG.md').read_text(encoding='utf-8')
        if '0.0.2' not in changelog or 'exercise release changelog' not in changelog:
            raise RuntimeError('release tool did not generate the expected default changelog')
    print('Pinned release tool generated its default changelog without credentials.')


if __name__ == '__main__':
    check(sys.argv[1])

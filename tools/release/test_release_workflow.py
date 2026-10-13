import hashlib
import io
import json
import os
from pathlib import Path
import shlex
import subprocess
import tarfile
import tempfile
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / '.github/workflows/release-plz.yml'
INSTALLER = Path(__file__).with_name('install-release-plz.sh')
VERSION = '0.3.172'
ARCHIVE_SHA256 = 'db58a4668ce9ef0b6431e96a665715745e9a5d7f0b0b28a610034d94de3970df'
SOURCE_URL = ('https://github.com/release-plz/release-plz/releases/download/'
              f'release-plz-v{VERSION}/release-plz-x86_64-unknown-linux-musl.tar.gz')


def action_steps(job):
    return {step['uses'].split('@')[0]: step for step in job['steps'] if 'uses' in step}


class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self):
        # Preserve GitHub's scalar spelling, including its YAML `on` key.
        self.workflow = yaml.load(WORKFLOW.read_text(), Loader=yaml.BaseLoader)

    def test_all_external_actions_in_both_jobs_use_full_commit_shas(self):
        for name in ['release', 'release-pr']:
            steps = [s for s in self.workflow['jobs'][name]['steps'] if 'uses' in s]
            self.assertTrue(steps)
            for step in steps:
                with self.subTest(job=name, action=step['uses']):
                    self.assertRegex(step['uses'], r'^[\w.-]+/[\w./-]+@[0-9a-f]{40}$')

    def test_both_jobs_use_the_same_reviewed_action_set(self):
        expected = {
            'actions/checkout', 'dtolnay/rust-toolchain',
            'actions/create-github-app-token', 'taiki-e/install-action',
            'release-plz/git-config',
        }
        jobs = self.workflow['jobs']
        for name in ['release', 'release-pr']:
            with self.subTest(job=name):
                actions = action_steps(jobs[name])
                self.assertEqual(set(actions), expected)
                self.assertEqual(len(actions), len([s for s in jobs[name]['steps'] if 'uses' in s]))
                for repo, step in actions.items():
                    self.assertEqual(step['uses'], action_steps(jobs['release'])[repo]['uses'])

    def test_tool_installation_is_exact_checked_and_has_no_fallback(self):
        for name in ['release', 'release-pr']:
            with self.subTest(job=name):
                job = self.workflow['jobs'][name]
                actions = action_steps(job)
                self.assertIn('taiki-e/install-action', actions)
                self.assertEqual(actions['taiki-e/install-action']['with'], {
                    'tool': 'cargo-semver-checks@0.51.0', 'checksum': 'true', 'fallback': 'none',
                })
                self.assertEqual(actions['dtolnay/rust-toolchain']['with']['toolchain'], 'stable')
                setup = [s for s in job['steps'] if s.get('run') == 'bash tools/release/install-release-plz.sh']
                self.assertEqual(len(setup), 1)
                token_index = next(i for i, s in enumerate(job['steps']) if s.get('id') == 'app-token')
                self.assertLess(job['steps'].index(setup[0]), token_index)

    def test_cli_preserves_commands_tokens_forge_and_repository(self):
        for name, title in [('release', 'Publish crates and GitHub releases'),
                            ('release-pr', 'Create or update release PR')]:
            with self.subTest(job=name):
                step = next(s for s in self.workflow['jobs'][name]['steps'] if s.get('name') == title)
                self.assertIn('run', step)
                expected = ['release-plz', name, '--git-token', '${GITHUB_TOKEN}', '--forge', 'github']
                if name == 'release-pr':
                    expected += ['--repo-url', '${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY}']
                expected += ['-o', 'json']
                self.assertEqual(shlex.split(step['run']), expected)
                self.assertEqual(step['env']['GITHUB_TOKEN'], '${{ steps.app-token.outputs.token }}')
                config = action_steps(self.workflow['jobs'][name])['release-plz/git-config']
                self.assertEqual(config['env']['GITHUB_TOKEN'], step['env']['GITHUB_TOKEN'])

    def test_existing_release_permissions_environment_and_checkout_are_preserved(self):
        jobs = self.workflow['jobs']
        self.assertEqual(self.workflow['permissions'], {'contents': 'read'})
        self.assertEqual(set(self.workflow['on']), {'push', 'workflow_dispatch'})
        self.assertEqual(self.workflow['on']['push']['branches'], ['main'])
        self.assertEqual(jobs['release']['environment'], 'release')
        self.assertEqual(jobs['release']['needs'], 'check-release-commit')
        self.assertEqual(jobs['release']['if'], "needs.check-release-commit.outputs.is_release == 'true'")
        self.assertEqual(jobs['release']['permissions'], {'contents': 'read', 'id-token': 'write'})
        self.assertNotIn('permissions', jobs['release-pr'])
        self.assertEqual(jobs['release-pr']['concurrency'], {
            'group': 'release-plz-pr-${{ github.ref }}', 'cancel-in-progress': 'false',
        })
        for name, pr_permission in [('release', 'read'), ('release-pr', 'write')]:
            actions = action_steps(jobs[name])
            self.assertEqual(actions['actions/checkout']['with'], {
                'fetch-depth': '0', 'persist-credentials': 'false',
            })
            self.assertEqual(actions['actions/create-github-app-token']['with'], {
                'client-id': '${{ vars.RELEASE_PLZ_APP_CLIENT_ID }}',
                'private-key': '${{ secrets.RELEASE_PLZ_APP_PRIVATE_KEY }}',
                'permission-contents': 'write', 'permission-pull-requests': pr_permission,
            })


class ReleaseInstallerTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(
            prefix='shodo-release-test ', dir=os.environ.get('TMPDIR'))
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.runner = self.root / 'runner temp'
        self.runner.mkdir()
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.path_file = self.root / 'github path'
        self.path_file.write_text('previous-path\n')
        self.curl_log = self.root / 'curl.json'
        self.executed = self.root / 'binary-executed'
        self.archive = self.root / 'source.tar.gz'
        self.script = INSTALLER.read_text()
        self.env = os.environ.copy()
        self.env.update(PATH=f'{self.bin}{os.pathsep}{self.env["PATH"]}',
                        RUNNER_TEMP=str(self.runner), GITHUB_PATH=str(self.path_file),
                        SOURCE_ARCHIVE=str(self.archive), CURL_LOG=str(self.curl_log),
                        BINARY_EXECUTED=str(self.executed))
        curl = self.bin / 'curl'
        curl.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
pathlib.Path(os.environ['CURL_LOG']).write_text(json.dumps(sys.argv[1:]))
if os.environ.get('CURL_FAIL'):
    sys.exit(22)
shutil.copyfile(os.environ['SOURCE_ARCHIVE'], sys.argv[sys.argv.index('--output') + 1])
''')
        curl.chmod(0o755)

    def make_archive(self, version=VERSION, member='release-plz', exit_status=0):
        data = (f'#!/bin/sh\nprintf executed > "$BINARY_EXECUTED"\n'
                f'printf "release-plz {version}\\n"\nexit {exit_status}\n').encode()
        with tarfile.open(self.archive, 'w:gz') as archive:
            entry = tarfile.TarInfo(member)
            entry.size = len(data)
            entry.mode = 0o755
            archive.addfile(entry, io.BytesIO(data))
        return hashlib.sha256(self.archive.read_bytes()).hexdigest()

    def run_installer(self, fixture_hash=None):
        script = self.script
        if fixture_hash:
            # Exercise the real install code using a small known test binary;
            # production has no URL/version/hash override or test-only input.
            self.assertEqual(script.count(ARCHIVE_SHA256), 1)
            script = script.replace(ARCHIVE_SHA256, fixture_hash)
        copy = self.root / 'install.sh'
        copy.write_text(script)
        return subprocess.run(['bash', str(copy)], env=self.env, capture_output=True, text=True)

    def assert_failed_without_publication(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.path_file.read_text(), 'previous-path\n')
        self.assertEqual(list(self.runner.iterdir()), [])

    def test_matching_archive_is_checked_and_added_to_existing_path(self):
        result = self.run_installer(self.make_archive())
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue(self.executed.exists())
        lines = self.path_file.read_text().splitlines()
        self.assertEqual(lines[0], 'previous-path')
        self.assertEqual(len(lines), 2)
        installed = Path(lines[1])
        self.assertEqual(installed.parent, self.runner)
        self.assertEqual([p.name for p in installed.iterdir()], ['release-plz'])
        args = json.loads(self.curl_log.read_text())
        self.assertEqual(args[-1], SOURCE_URL)
        self.assertIn('--fail', args)
        self.assertEqual(args[args.index('--proto') + 1], '=https')
        self.assertEqual(args[args.index('--proto-redir') + 1], '=https')

    def test_replaced_archive_is_rejected_before_binary_execution(self):
        self.make_archive()
        result = self.run_installer()
        self.assert_failed_without_publication(result)
        self.assertFalse(self.executed.exists())

    def test_truncated_archive_is_rejected_before_extraction_or_execution(self):
        digest = self.make_archive()
        self.archive.write_bytes(self.archive.read_bytes()[:10])
        self.assert_failed_without_publication(self.run_installer(digest))
        self.assertFalse(self.executed.exists())

    def test_download_failure_cleans_staging_without_execution(self):
        self.env['CURL_FAIL'] = '1'
        self.assert_failed_without_publication(self.run_installer())
        self.assertFalse(self.executed.exists())

    def test_validated_archive_missing_binary_fails_without_path_publication(self):
        self.assert_failed_without_publication(self.run_installer(self.make_archive(member='other')))
        self.assertFalse(self.executed.exists())

    def test_wrong_binary_version_fails_without_path_publication(self):
        self.assert_failed_without_publication(self.run_installer(self.make_archive(version='0.3.171')))
        self.assertTrue(self.executed.exists())

    def test_version_command_failure_fails_even_if_its_output_matches(self):
        self.assert_failed_without_publication(self.run_installer(self.make_archive(exit_status=37)))
        self.assertTrue(self.executed.exists())

    def test_missing_runner_environment_is_rejected_before_download(self):
        for key in ['RUNNER_TEMP', 'GITHUB_PATH']:
            with self.subTest(key=key):
                value = self.env.pop(key)
                try:
                    self.assert_failed_without_publication(self.run_installer())
                    self.assertFalse(self.curl_log.exists())
                finally:
                    self.env[key] = value


if __name__ == '__main__':
    unittest.main()

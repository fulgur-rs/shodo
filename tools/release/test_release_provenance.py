import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]
REPO = 'fulgur-rs/shodo'
MERGE_SHA = 'a' * 40
HEAD_SHA = 'b' * 40


def pull(repo=REPO, ref='release-plz-2026-10-14', number=1):
    return dict(number=number, user=dict(login='release-bot'),
                head=dict(sha=HEAD_SHA, ref=ref, label=f'fulgur-rs:{ref}',
                          repo=dict(full_name=repo, name='shodo', fork=False)),
                base=dict(ref='main', repo=dict(full_name=REPO)))


# Run the actual workflow shell, replacing only its read/write API transport.
# --jq uses jq's equivalent subset and raw scalar output like gh. Pagination
# emits separate JSON pages, exactly as gh api --paginate does without --slurp.
GH_STUB = '''#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys
args = sys.argv[1:]
assert args[0] == 'api'
with open(os.environ['CALLS_FILE'], 'a') as output:
    output.write(json.dumps(args) + '\\n')
data = json.loads(pathlib.Path(os.environ['DATA_FILE']).read_text())
endpoint = next(a for a in args if a.startswith('repos/'))
if '-X' in args and args[args.index('-X') + 1] == 'POST':
    assert '/statuses/' in endpoint
    fields = dict(a.split('=', 1) for a in args if '=' in a)
    with open(os.environ['STATUS_FILE'], 'a') as output:
        output.write(json.dumps(fields) + '\\n')
    sys.exit(0)
if '/commits/' in endpoint:
    pages = data['associated']
    failure = data.get('associated_error')
elif '/pulls?' in endpoint:
    pages = data['open']
    failure = data.get('open_error')
elif '/reviews?' in endpoint:
    pages = [data.get('reviews', [])]
    failure = False
elif '/collaborators/' in endpoint:
    pages = [{'permission': 'write'}]
    failure = False
else:
    raise AssertionError('unexpected API route: ' + endpoint)
if failure:
    sys.exit(22)
if '--paginate' not in args:
    pages = pages[:1]
for page in pages:
    body = '{' if data.get('malformed') else json.dumps(page)
    if '--jq' in args:
        result = subprocess.run(['jq', '-r', args[args.index('--jq') + 1]],
                                input=body, text=True, capture_output=True)
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        if result.returncode:
            sys.exit(result.returncode)
    else:
        print(body)
'''


class ReleaseProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='shodo-provenance-',
                                                      dir=os.environ.get('TMPDIR'))
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / 'bin').mkdir()
        stub = self.root / 'bin/gh'
        stub.write_text(GH_STUB)
        stub.chmod(0o755)
        self.publisher = yaml.load((ROOT / '.github/workflows/release-plz.yml').read_text(),
                                   Loader=yaml.BaseLoader)
        self.gate = yaml.load((ROOT / '.github/workflows/release-pr-approval-gate.yml').read_text(),
                              Loader=yaml.BaseLoader)
        self.publish_script = self.publisher['jobs']['check-release-commit']['steps'][0]['run']
        self.gate_script = self.gate['jobs']['gate']['steps'][0]['run']

    def execute(self, script, associated, open_pages=None, **overrides):
        reviews = [dict(id=1, user=dict(type='User', login='maintainer'),
                        state='APPROVED', commit_id=HEAD_SHA, author_association='OWNER')]
        data = dict(associated=associated, open=open_pages or associated, reviews=reviews)
        data.update(overrides)
        files = {name: self.root / name for name in ['data', 'calls', 'statuses', 'output']}
        files['data'].write_text(json.dumps(data))
        for name in ['calls', 'statuses', 'output']:
            files[name].write_text('')
        env = os.environ.copy()
        env.update(PATH=f'{self.root / "bin"}{os.pathsep}{env["PATH"]}',
                   REPO=REPO, SHA=MERGE_SHA, PR='1', RESOLVED_SHA=HEAD_SHA,
                   GH_TOKEN='fixture-token', DATA_FILE=str(files['data']),
                   CALLS_FILE=str(files['calls']), STATUS_FILE=str(files['statuses']),
                   GITHUB_OUTPUT=str(files['output']))
        result = subprocess.run(['bash', '-c', script], env=env, capture_output=True, text=True)
        statuses = [json.loads(line) for line in files['statuses'].read_text().splitlines()]
        calls = [json.loads(line) for line in files['calls'].read_text().splitlines()]
        return result, files['output'].read_text(), statuses, calls

    def publication(self, pages, expected, **overrides):
        result, output, statuses, calls = self.execute(self.publish_script, pages, **overrides)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, f'is_release={str(expected).lower()}\n')
        self.assertEqual(statuses, [])
        self.assertTrue(any(f'repos/{REPO}/commits/{MERGE_SHA}/pulls' in c for c in calls))

    def approval(self, pages, gated, **overrides):
        result, output, statuses, _ = self.execute(self.gate_script, [[]], pages, **overrides)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, '')
        self.assertEqual(statuses[0]['state'], 'pending')
        self.assertEqual(statuses[-1]['state'], 'success')
        self.assertEqual(statuses[-1]['description'],
                         'Approved by a maintainer on the current commit' if gated
                         else 'Not a release PR - not gated')

    def test_fork_release_branch_is_not_a_publication_or_release_approval_candidate(self):
        rows = [[pull(repo='attacker/shodo')]]
        self.publication(rows, False)
        self.approval(rows, False)

    def test_canonical_release_keeps_approval_and_accepts_a_distinct_merge_sha(self):
        rows = [[pull()]]
        self.assertNotEqual(rows[0][0]['head']['sha'], MERGE_SHA)
        self.publication(rows, True)
        self.approval(rows, True)

    def test_normal_canonical_and_fork_branches_remain_ordinary(self):
        for repo in [REPO, 'attacker/shodo']:
            for ref in ['feature', 'feature/release-plz-2026', 'Release-plz-2026']:
                with self.subTest(repo=repo, ref=ref):
                    rows = [[pull(repo=repo, ref=ref)]]
                    self.publication(rows, False)
                    self.approval(rows, False)

    def test_base_repo_label_name_or_fork_flag_cannot_replace_head_provenance(self):
        for repo in ['attacker/shodo', 'fulgur-rs/shodo-copy', 'shodo', 'Fulgur-rs/shodo']:
            with self.subTest(repo=repo):
                row = pull(repo=repo)
                self.assertEqual(row['base']['repo']['full_name'], REPO)
                self.assertEqual(row['head']['repo']['name'], 'shodo')
                self.assertFalse(row['head']['repo']['fork'])
                self.publication([[row]], False)
                self.approval([[row]], False)

    def test_unknown_head_repository_is_rejected_even_with_release_prefix(self):
        for mode in ['null_repo', 'missing_repo', 'null_name', 'missing_name']:
            with self.subTest(mode=mode):
                row = pull()
                if mode == 'null_repo':
                    row['head']['repo'] = None
                elif mode == 'missing_repo':
                    del row['head']['repo']
                elif mode == 'null_name':
                    row['head']['repo']['full_name'] = None
                else:
                    del row['head']['repo']['full_name']
                self.publication([[row]], False)
                self.approval([[row]], False)

    def test_unknown_head_ref_remains_non_release_in_both_workflows(self):
        for mode in ['null', 'missing']:
            with self.subTest(mode=mode):
                row = pull()
                if mode == 'null':
                    row['head']['ref'] = None
                else:
                    del row['head']['ref']
                self.publication([[row]], False)
                self.approval([[row]], False)

    def test_mixed_release_candidates_cannot_feed_a_fork_to_the_downstream_selector(self):
        canonical = pull()
        fork = pull(repo='attacker/shodo', number=2)
        for rows in [[fork, canonical], [canonical, fork]]:
            with self.subTest(first=rows[0]['head']['repo']['full_name']):
                self.publication([rows], False)
                self.approval([rows], True)

    def test_ordinary_fork_does_not_block_canonical_release_candidates(self):
        rows = [[pull(repo='attacker/shodo', ref='feature', number=2), pull()]]
        self.publication(rows, True)
        self.approval(rows, True)
        self.publication([[pull(number=1), pull(number=2)]], True)

    def test_page_behavior_matches_each_existing_consumer(self):
        # release-plz 0.3.172 reads one associated-PR response; publication
        # preserves that boundary. Approval must inspect all open/main pages.
        pages = [[pull(ref='feature')], [pull()]]
        self.publication(pages, False)
        self.approval(pages, True)
        self.publication([[pull()], [pull(repo='attacker/shodo')]], True)

    def test_api_or_json_failure_cannot_emit_publication_permission(self):
        for failure in [dict(associated_error=True), dict(malformed=True)]:
            with self.subTest(failure=failure):
                result, output, statuses, _ = self.execute(self.publish_script, [[pull()]], **failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, '')
                self.assertEqual(statuses, [])

    def test_gate_api_failure_remains_pending_and_unapproved_release_still_waits(self):
        for overrides in [dict(open_error=True), dict(reviews=[])]:
            with self.subTest(overrides=overrides):
                _, _, statuses, _ = self.execute(self.gate_script, [[]], [[pull()]], **overrides)
                self.assertTrue(statuses)
                self.assertTrue(all(s['state'] == 'pending' for s in statuses))

    def test_metadata_is_data_even_when_fork_branch_contains_shell_syntax(self):
        marker = self.root / 'must-not-exist'
        row = pull(repo='attacker/shodo', ref=f'release-plz-$(touch {marker})')
        self.publication([[row]], False)
        self.assertFalse(marker.exists())

    def test_workflow_keeps_protected_environment_main_guard_and_literal_boolean_sink(self):
        check = self.publisher['jobs']['check-release-commit']
        self.assertEqual(check['if'], "github.repository == 'fulgur-rs/shodo' && github.ref == 'refs/heads/main'")
        release = self.publisher['jobs']['release']
        self.assertEqual(release['needs'], 'check-release-commit')
        self.assertEqual(release['if'], "needs.check-release-commit.outputs.is_release == 'true'")
        self.assertEqual(release['environment'], 'release')
        self.assertEqual(release['permissions'], {'contents': 'read', 'id-token': 'write'})
        for script in [self.publish_script, self.gate_script]:
            result = subprocess.run(['bash', '-n'], input=script, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()

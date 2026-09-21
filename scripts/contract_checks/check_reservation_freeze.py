"""Read-only separate reservation freeze audit; never rewrites a reviewed file.

Approval pins live outside the candidate package and the legacy registry. All
negative probes use in-memory copies. No network, Git or authoring mode is needed.
"""
import copy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FREEZE = 'contracts/freezes/reservation-settlement-1.json'
REVIEW = 'contracts/candidates/reservation-settlement-v1/review-manifest.json'
STATUS = 'RESERVATION-SETTLEMENT-FREEZE.md'
CHECKER = 'scripts/contract_checks/check_reservation_freeze.py'
AGGREGATE = 'scripts/check.sh'
LEGACY = 'contracts/freeze.json'
AMENDMENT_REPORT = 'docs/reviews/reservation-settlement-1/amendment-review.md'
CONFORMANCE_REPORT = 'docs/reviews/reservation-settlement-1/conformance-review.md'
CANDIDATE = 'd311d9622b527863c79fd5e5820f5b890ab2c6b1'
PHASE2 = '6194376a053b8a27887a9b09459054a7af3a1769'
PROFILE = 'reservation-settlement/1'
REVIEW_SHA = '90d1c6b73d6692f6c366735bfc2188d9e8c50d5038c7fb6f5629c85ffa3d8b22'
LEGACY_SHA = 'e19f512e99e28e7ff74e5d20673ae4bde0bcd5721a103f00db34aa91f2300567'
ORACLE_SHA = 'a97c2a4cd60eddd8ec8675ed91132059dc7eff266ddbd18e55a9f736ea475e0d'
STATUS_SHA = '3135194c59b24648cfabea0766400c7774841de80416902d4d646fe20c9aaa27'
AGGREGATE_BEFORE_SHA = 'e84e435c91fbd6fe5a250aa37ae847f17e511c73523cb804d28ec2d0c24af1aa'
APPEND = (b'# Separately frozen reservation amendment; legacy contract gates stay unchanged.\n'
          b'python3 scripts/contract_checks/check_reservation_freeze.py\n'
          b'sh scripts/check-reservation-settlement.sh\n')
APPROVAL = {
    'reviewer_task': '01a0c585-75f4-7cd0-ad22-f01dd8efb86d',
    'verdict': 'PASS', 'scope': 'exact successor safe for contract-only freeze',
    'report': {'path': AMENDMENT_REPORT, 'bytes': 7586,
               'sha256': 'a42e07398b9ca33827fc645a3c464a64c51a6d3d8922f125d37cfd06c94dcd93'},
}
CONFORMANCE = {
    'reviewer_task': '01a0c551-f10d-73c3-871c-7242c663e851',
    'verdict': 'PASS', 'r1': 'closed',
    'review_commit': '6a81c93f15211b89bbc9812c5e39645026ba19d6',
    'source_path': 'crates/ledgerlab-testkit/oracle/phase3/reviews/d311d96/REVIEW.md',
    'report': {'path': CONFORMANCE_REPORT, 'bytes': 6263,
               'sha256': '9aa74852750cf70ece429a9575deaba0adb99c16ee0193a0a58297015336190f'},
}
AUTHORIZATION = {
    'owner_task': '01a0bf74-3b70-72b2-a1b0-ee6fb5f99dfb',
    'scope': 'reviewed contract-only freeze',
    'allowed': ['separate approval/freeze manifest', 'status overlay',
                'read-only manifest checker', 'aggregate offline check wiring'],
    'prohibited': ['reviewed candidate edits', 'legacy registry or validator edits',
                   'SQL', 'runtime', 'production ports', 'main merge', 'push', 'tag',
                   'publication', 'deployment', 'service registration', 'spending'],
}
CONTROLS = {STATUS, CHECKER, AGGREGATE, AMENDMENT_REPORT, CONFORMANCE_REPORT}
TOP_LEVEL = {'schema', 'status', 'profile', 'reviewed_commit', 'phase2_base',
             'approval', 'conformance', 'oracle', 'authorization', 'review_manifest',
             'legacy', 'files', 'controls', 'aggregate', 'historical_labels', 'change_policy'}
LABEL_RULE = 'Historical candidate labels remain unchanged; this separate manifest and status overlay establish current status.'
CHANGE_RULE = 'Normative, schema, ID/hash-domain, vector or semantic changes require fresh independent review; no runtime implementation is authorized.'


def require(condition, code):
    if not condition:
        raise ValueError(code)


def strict(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'DUPLICATE_METADATA_KEY')
            result[key] = value
        return result
    return json.loads(raw.decode('utf-8'), object_pairs_hook=pairs)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def descriptor(raw):
    return {'bytes': len(raw), 'sha256': sha(raw)}


def verify(manifest=None, overrides=None):
    overrides = {} if overrides is None else overrides
    def read(path):
        return overrides[path] if path in overrides else (ROOT / path).read_bytes()

    manifest = strict(read(FREEZE)) if manifest is None else manifest
    require(set(manifest) == TOP_LEVEL, 'FREEZE_METADATA_SHAPE')
    require(manifest['schema'] == 'ledger-reservation-contract-freeze/1', 'FREEZE_SCHEMA')
    require(manifest['status'] == 'frozen-unreleased', 'FREEZE_STATUS')
    require(manifest['profile'] == PROFILE and manifest['reviewed_commit'] == CANDIDATE,
            'EXACT_REVIEWED_CANDIDATE')
    require(manifest['phase2_base'] == PHASE2, 'PHASE2_BASE')
    require(manifest['approval'] == APPROVAL, 'INDEPENDENT_APPROVAL')
    require(manifest['conformance'] == CONFORMANCE, 'CONFORMANCE_PROVENANCE')
    require(manifest['authorization'] == AUTHORIZATION, 'FREEZE_ONLY_AUTHORIZATION')
    require(manifest['historical_labels'] == LABEL_RULE, 'HISTORICAL_LABEL_RULE')
    require(manifest['change_policy'] == CHANGE_RULE, 'CHANGE_POLICY')
    require(sha(json.dumps(manifest['oracle'], sort_keys=True,
                           separators=(',', ':')).encode()) == ORACLE_SHA, 'ORACLE_PROVENANCE')

    review_raw = read(REVIEW)
    require(sha(review_raw) == REVIEW_SHA, 'REVIEWED_INVENTORY_CHANGED')
    require(manifest['review_manifest'] == {'path': REVIEW, **descriptor(review_raw)},
            'REVIEW_MANIFEST_BINDING')
    review = strict(review_raw)
    require(review['status'] == 'candidate-not-frozen', 'HISTORICAL_STATUS_CHANGED')
    expected = {**review['files'], REVIEW: descriptor(review_raw)}
    require(len(expected) == 13 and manifest['files'] == expected, 'EXACT_REVIEWED_MEMBERSHIP')
    for path, info in expected.items():
        require(descriptor(read(path)) == info, 'REVIEWED_BYTES_CHANGED:' + path)
    # No new candidate asset or checker can silently join the reviewed package.
    package = ROOT / 'contracts/candidates/reservation-settlement-v1'
    actual = {str(p.relative_to(ROOT)) for p in package.rglob('*') if p.is_file()}
    require(actual == {p for p in expected if p.startswith(str(package.relative_to(ROOT)) + '/')},
            'CANDIDATE_DIRECTORY_MEMBERSHIP')
    scripts = ROOT / 'scripts/reservation_settlement'
    actual = {str(p.relative_to(ROOT)) for p in scripts.iterdir() if p.suffix in ('.py', '.mjs')}
    require(actual == {p for p in expected if p.startswith('scripts/reservation_settlement/')},
            'REVIEWED_CHECKER_MEMBERSHIP')

    legacy_raw = read(LEGACY)
    require(sha(legacy_raw) == LEGACY_SHA, 'LEGACY_REGISTRY_CHANGED')
    require(manifest['legacy'] == {'path': LEGACY, **descriptor(legacy_raw), 'frozen_files': 159},
            'LEGACY_BINDING')
    legacy = strict(legacy_raw)
    require(len(legacy['files']) == 159, 'LEGACY_COUNT')
    for path, digest in legacy['files'].items():
        require(sha(read(path)) == digest, 'LEGACY_BYTES_CHANGED:' + path)

    require(set(manifest['controls']) == CONTROLS, 'FREEZE_CONTROL_MEMBERSHIP')
    for path, info in manifest['controls'].items():
        require(descriptor(read(path)) == info, 'FREEZE_CONTROL_BYTES:' + path)
    for evidence in (APPROVAL, CONFORMANCE):
        report = evidence['report']
        require(descriptor(read(report['path'])) == {k: report[k] for k in ('bytes', 'sha256')},
                'REVIEW_REPORT_CHANGED')
    require(sha(read(STATUS)) == STATUS_SHA, 'STATUS_OVERLAY_CHANGED')
    aggregate = read(AGGREGATE)
    require(aggregate.endswith(APPEND), 'AGGREGATE_HOOK')
    before = aggregate[:-len(APPEND)]
    require(len(before) == 501 and sha(before) == AGGREGATE_BEFORE_SHA,
            'LEGACY_AGGREGATE_CHANGED')
    require(manifest['aggregate'] == {'path': AGGREGATE,
            'reviewed_prefix': descriptor(before), 'append_utf8': APPEND.decode()},
            'AGGREGATE_BINDING')
    return {'status': 'frozen-unreleased', 'profile': PROFILE, 'reviewed_commit': CANDIDATE,
            'reviewed_files': len(expected), 'legacy_frozen_files': len(legacy['files']),
            'control_artifacts': len(CONTROLS), 'old_registry_unchanged': True}


def metadata_probes():
    baseline = strict((ROOT / FREEZE).read_bytes())
    probes = []
    def field(name, path, value):
        altered = copy.deepcopy(baseline)
        target = altered
        for key in path[:-1]:
            target = target[key]
        target[path[-1]] = value
        probes.append((name, altered, {}))
    field('wrong-status', ['status'], 'candidate-not-frozen')
    field('wrong-reviewed-commit', ['reviewed_commit'], '0' * 40)
    field('wrong-base', ['phase2_base'], '0' * 40)
    field('wrong-verdict', ['approval', 'verdict'], 'FAIL')
    field('wrong-reviewer', ['approval', 'reviewer_task'], 'unreviewed')
    field('reopened-r1', ['conformance', 'r1'], 'open')
    field('broaden-authorization', ['authorization', 'scope'], 'runtime implementation')
    field('wrong-oracle', ['oracle', 'commit'], '0' * 40)
    field('changed-oracle-pin', ['oracle', 'files', next(iter(baseline['oracle']['files'])), 'sha256'], '0' * 64)
    field('extra-top-level-field', ['exceptions'], ['any'])
    altered = copy.deepcopy(baseline)
    del altered['files'][REVIEW]
    probes.append(('removed-reviewed-pin', altered, {}))
    field('added-unreviewed-pin', ['files', 'unreviewed.txt'], {'bytes': 0, 'sha256': sha(b'')})
    altered = copy.deepcopy(baseline)
    del altered['controls'][CHECKER]
    probes.append(('removed-checker-pin', altered, {}))
    # Coherent metadata edits cannot repin altered reviewed bytes or old assets.
    path = 'contracts/candidates/reservation-settlement-v1/README.md'
    raw = (ROOT / path).read_bytes() + b'\nChanged normative meaning.\n'
    altered = copy.deepcopy(baseline); altered['files'][path] = descriptor(raw)
    probes.append(('repinned-reviewed-content', altered, {path: raw}))
    raw = (ROOT / REVIEW).read_bytes().replace(b'candidate-not-frozen', b'frozen-unreleased')
    altered = copy.deepcopy(baseline)
    altered['review_manifest'] = {'path': REVIEW, **descriptor(raw)}
    altered['files'][REVIEW] = descriptor(raw)
    probes.append(('repinned-historical-label', altered, {REVIEW: raw}))
    raw = (ROOT / LEGACY).read_bytes() + b'\n'
    altered = copy.deepcopy(baseline)
    altered['legacy'] = {'path': LEGACY, **descriptor(raw), 'frozen_files': 159}
    probes.append(('repinned-legacy-registry', altered, {LEGACY: raw}))
    raw = (ROOT / AGGREGATE).read_bytes()[:-len(APPEND)]
    altered = copy.deepcopy(baseline); altered['controls'][AGGREGATE] = descriptor(raw)
    probes.append(('removed-aggregate-hook', altered, {AGGREGATE: raw}))
    raw = b'# Runtime is authorized\n'
    altered = copy.deepcopy(baseline); altered['controls'][STATUS] = descriptor(raw)
    probes.append(('repinned-status-scope', altered, {STATUS: raw}))
    raw = (ROOT / AMENDMENT_REPORT).read_bytes() + b'\nAmended verdict.\n'
    altered = copy.deepcopy(baseline)
    altered['approval']['report'] = {'path': AMENDMENT_REPORT, **descriptor(raw)}
    altered['controls'][AMENDMENT_REPORT] = descriptor(raw)
    probes.append(('repinned-review-verdict', altered, {AMENDMENT_REPORT: raw}))
    for name, altered, overrides in probes:
        try:
            verify(altered, overrides)
        except ValueError:
            continue
        raise AssertionError('FREEZE_ATTACK_ACCEPTED:' + name)
    try:
        strict(b'{"status":"frozen-unreleased","status":"candidate-not-frozen"}')
    except ValueError:
        pass
    else:
        raise AssertionError('DUPLICATE_METADATA_ACCEPTED')
    return len(probes) + 1


def main():
    result = verify()
    print(json.dumps({**result, 'freeze_metadata_negative_checks': metadata_probes()}))


if __name__ == '__main__':
    main()

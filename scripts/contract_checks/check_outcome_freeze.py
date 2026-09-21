"""Read-only Phase 1 freeze/status audit; no economic or codec changes.

The reviewed package remains byte-for-byte at its original paths. This binds it
to the approved commit and the established contracts/freeze.json inventory.
"""
import copy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REVIEWED = '429aa027a696a09cfeb8dc1eada8420e732dda6b'
SEMANTIC = '1e0ba3f886788c08f427d3aae1d916b341187e76'
PROFILE = '2-candidate.4'
REVIEW = 'contracts/candidates/v2/review-manifest.json'
FREEZE = 'contracts/freezes/outcomes-2-candidate.4.json'
AUDIT = 'scripts/contract_checks/v2_candidate/audit.py'
REPORT = 'PHASE-1-FREEZE.md'
REVIEW_SHA = 'e02e02f30c09335445571de3fb3451cd4f1f5c232920d3b4ec32d228f5b579ad'
V1_INVENTORY_SHA = 'e7caa7946966871e7deca4425c143fdc4c07debe1eb5be868b840db75e9488e9'
V1_FILES_SHA = '471ca695eb59e9bd1be0e507dff91067405f75b8baf6c15bc20802b366f15aff'
AUDIT_SEMANTICS_SHA = '562817350d3697bbedc646aa47d37081380c3ada5ac6f413e5b9ef6c7cfe3ccf'
OVERLAYS = {'ROADMAP.md', AUDIT}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def descriptor(raw):
    return {'bytes': len(raw), 'sha256': sha(raw)}


def audit_semantics(raw):
    # Exclude only the package-status check and its final diagnostic print.
    # All parsing, reconstruction, semantic checks and test calls stay reviewed.
    code = raw.decode()
    start = code.index('def main():')
    body = code.index('    constructed=files();', start)
    end = code.index("    print(json.dumps({'status':'passed'", body)
    after = code.index('\n', end)
    return sha((code[:start] + code[body:end] + code[after:]).encode())


def verify(root=ROOT, *, manifest=None, registry=None):
    raw_review = (root / REVIEW).read_bytes()
    assert sha(raw_review) == REVIEW_SHA, 'REVIEWED_INVENTORY_CHANGED'
    review = json.loads(raw_review)
    manifest = json.loads((root / FREEZE).read_bytes()) if manifest is None else manifest
    registry = json.loads((root / 'contracts/freeze.json').read_bytes()) if registry is None else registry
    assert manifest['schema'] == 'ledger-contract-freeze/1'
    assert manifest['status'] == 'frozen-unreleased' and manifest['profile'] == PROFILE, 'FREEZE_STATUS'
    assert manifest['reviewed_commit'] == REVIEWED and manifest['semantic_commit'] == SEMANTIC, 'APPROVED_COMMITS'
    assert manifest['approval']['verdict'] == 'PASS TO FREEZE', 'INDEPENDENT_APPROVAL'
    assert manifest['approval']['reviewer_task'] == '01a0c4c8-ca3d-7e71-b9be-3682e6555f4d'
    assert manifest['authorization']['owner_task'] == '01a0bf74-3b70-72b2-a1b0-ee6fb5f99dfb'
    assert manifest['authorization']['scope'] == 'Phase 1 contract freeze only'
    assert manifest['review_manifest'] == {'path': REVIEW, **descriptor(raw_review)}
    assert set(manifest['status_overlays']) == OVERLAYS, 'STATUS_OVERLAY_SCOPE'
    expected = {p: d for p, d in review['files'].items() if p not in OVERLAYS}
    expected[REVIEW] = descriptor(raw_review)
    assert manifest['files'] == expected, 'REVIEWED_FREEZE_MEMBERSHIP'
    for path, original in expected.items():
        assert descriptor((root / path).read_bytes()) == original, 'REVIEWED_BYTES_CHANGED:' + path
    for path, overlay in manifest['status_overlays'].items():
        assert overlay['reviewed'] == review['files'][path], 'OVERLAY_REVIEW_ORIGIN'
        assert overlay['frozen'] == descriptor((root / path).read_bytes()), 'STATUS_METADATA_CHANGED:' + path
    assert audit_semantics((root / AUDIT).read_bytes()) == AUDIT_SEMANTICS_SHA, 'REVIEWED_AUDIT_SEMANTICS'
    # No unreviewed file may join the original contract/validator package.
    actual = {str(p.relative_to(root)) for p in (root / 'contracts/candidates/v2').rglob('*') if p.is_file()}
    actual.update(str(p.relative_to(root)) for p in (root / 'scripts/contract_checks/v2_candidate').iterdir() if p.suffix in ('.py', '.mjs', '.rs'))
    expected_scope = {p for p in review['files'] if p.startswith(('contracts/candidates/v2/', 'scripts/contract_checks/v2_candidate/'))} | {REVIEW}
    assert actual == expected_scope, 'REVIEWED_PACKAGE_MEMBERSHIP'
    assert registry['schema'] == 'ledger-contract-freeze/1' and registry['revision'] == 2
    assert registry['status'] == 'phase-1-contracts-frozen-unreleased'
    assert registry['extensions'] == [{'profile': PROFILE, 'manifest': FREEZE, 'reviewed_commit': REVIEWED, 'semantic_commit': SEMANTIC}], 'FREEZE_REGISTRATION'
    legacy = registry['v1_baseline']
    assert legacy['inventory_sha256'] == V1_INVENTORY_SHA and len(legacy['file_paths']) == len(set(legacy['file_paths'])) == 99
    legacy_files = {p: registry['files'][p] for p in legacy['file_paths']}
    assert sha(json.dumps(legacy_files, sort_keys=True, separators=(',', ':')).encode()) == V1_FILES_SHA, 'V1_PIN_CHANGED'
    controls = {FREEZE, REPORT, 'scripts/contract_checks/check_outcome_freeze.py'}
    assert set(registry['files']) == set(legacy_files) | set(expected) | OVERLAYS | controls, 'FROZEN_INVENTORY_CLOSURE'
    for path, digest in registry['files'].items():
        assert sha((root / path).read_bytes()) == digest, 'FROZEN_BYTES_CHANGED:' + path
    result = dict(status='frozen-unreleased', profile=PROFILE, reviewed_commit=REVIEWED,
                  semantic_commit=SEMANTIC, frozen_files=len(registry['files']),
                  frozen_v1_files=99, unchanged_reviewed_files=len(expected), status_overlays=len(OVERLAYS))
    return result


def main():
    result = verify()
    # Metadata attacks cannot remove pins, alter approval, or add an exception
    # for economic bytes. All probes are in memory; frozen files are read-only.
    baseline = json.loads((ROOT / FREEZE).read_bytes())
    for field, value in [('status', 'candidate-not-frozen'), ('reviewed_commit', '0' * 40)]:
        altered = copy.deepcopy(baseline); altered[field] = value
        try: verify(manifest=altered)
        except AssertionError: pass
        else: raise AssertionError('freeze metadata mutation accepted: ' + field)
    altered = copy.deepcopy(baseline); altered['files'].pop(next(iter(altered['files'])))
    try: verify(manifest=altered)
    except AssertionError: pass
    else: raise AssertionError('missing reviewed pin accepted')
    altered = copy.deepcopy(baseline); altered['status_overlays'][REVIEW] = {}
    try: verify(manifest=altered)
    except AssertionError: pass
    else: raise AssertionError('unreviewed status exception accepted')
    print(json.dumps({**result, 'freeze_metadata_negative_checks': 4}))


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Compile the actual private read port against minimal inert observation types.

This verifies trait capabilities, not database enforcement. The concrete-reader
lanes separately prove query-only snapshots and no physical state changes.
"""
from pathlib import Path
import subprocess
import tempfile
import re

root = Path(__file__).resolve().parents[1]
port = root / 'crates/ledgerlab/src/store/comparison.rs'
deps = root / 'target/debug/deps'
tokio = sorted(deps.glob('libtokio-*.rlib'))
assert tokio, 'Run cargo test -p ledgerlab --lib --locked --offline first'
source = '''#![allow(dead_code, unused_imports)]
mod store {
 mod outcomes {
  #[derive(Clone, Debug)] pub(crate) struct ScopedRecordRef;
  #[derive(Clone, Debug)] pub(crate) struct StoredCompositeDelivery;
  pub(crate) type ObservedOutcomeHeads = Vec<()>;
 }
 #[path = "''' + str(port) + '''"] pub(crate) mod comparison;
}
use store::comparison::*;
'''
with tempfile.TemporaryDirectory(prefix='comparison-capability-') as tmp:
    tmp = Path(tmp)
    def compile(extra):
        path = tmp / 'probe.rs'
        path.write_text(source + extra)
        return subprocess.run(['rustc', '--edition=2024', '--crate-type=lib',
            '--emit=metadata', '-L', 'dependency='+str(deps), '--extern',
            'tokio='+str(tokio[-1]), '-o', str(tmp/'probe.rmeta'), str(path)],
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    good = compile('fn read_capability<T: ComparisonReadTx>() {}')
    assert good.returncode == 0, good.stderr
    for method in ['commit', 'write', 'append_outcome', 'lock_scopes', 'lookup_outcome_delivery']:
        bad = compile(f'async fn forbidden<T: ComparisonReadTx>(mut tx:T) {{ tx.{method}().await; }}')
        assert bad.returncode != 0 and f'no method named `{method}`' in bad.stderr, bad.stderr

service = '\n'.join((root/'crates/ledgerlab/src/service'/name).read_text() for name in ['comparison.rs', 'comparison_run.rs'])
# Supplementary source checks; compile tests above exercise the real trait.
code = re.sub(r'//[^\n]*', '', service)
for forbidden in ['AcceptanceTx', 'OutcomeTx', 'AcceptCommand', 'OutcomeCommand',
                  'ValidatedOutcomePlan', 'SqliteStore', 'PostgresStore', 'outbox::',
                  'tokio::spawn', 'spawn_blocking']:
    assert forbidden not in code, forbidden
assert 'pub(crate) struct ComparisonWorkspace' in code
assert 'pub fn committed(&self) -> bool {\n        false' in code
print('comparison boundary PASS: actual port compiled; five forbidden methods rejected; source controls passed')

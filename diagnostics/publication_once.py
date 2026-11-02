#!/usr/bin/env python3
"""Private diagnostic only: fresh synthetic setup, one publication, no retry."""
import argparse, hashlib, json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts/e2e'))
from workflow_conditional import ConditionalJourney
from workflow_supported import Journey, canonical

class StopAfterPublication(Exception):
    pass

class Once(ConditionalJourney):
    def command(self, name, argv, expected=0, allow_unavailable=False):
        return Journey.command(self, name, argv, expected, allow_unavailable)

    def submit(self, name, kind, payload):
        request = self.root / (name + '.request.json')
        request.write_bytes(canonical({'schema':'ledger-operation/1','contract':'workflow/1',
            'kind':kind,'key':self.key(name),'payload':payload}))
        before_submit = self.snapshot() if name == 'publication' else None
        result = self.workflow(name, 'submit', request, allow_unavailable=True)
        if name == 'publication':
            after_submit = self.snapshot()
            before = after_submit
            key_path = self.write('publication-diagnostic-key.json', self.key(name))
            status = self.workflow('publication-diagnostic-status', 'status', key_path)
            after = self.snapshot()
            assert before == after
            assert status.get('schema') == 'ledger-operation-status/1'
            assert status.get('status') in ('not_observed', 'committed')
            if status['status'] == 'not_observed': assert status.get('key') == self.key(name)
            self.meta['diagnostic'] = {'publication_exit': self.meta['commands'][-2]['exit'],
                                       'status': status.get('status'), 'status_no_append': True}
            summary = {'schema':'publication-diagnostic/1', 'publication_exit':self.meta['commands'][-2]['exit'], 'status':status['status'], 'status_no_append':True, 'before_sha256':hashlib.sha256(canonical(before_submit)).hexdigest(), 'after_sha256':hashlib.sha256(canonical(after_submit)).hexdigest()}
            (self.root / 'diagnostic-summary.json').write_bytes(canonical(summary))
            self.meta['status'] = 'STOPPED_AFTER_ONE_PUBLICATION'
            self.save()
            raise StopAfterPublication()
        if self.meta['commands'][-1]['exit'] != 0:
            raise RuntimeError('setup refused; stopped without retry')
        (self.root / (name + '.committed.json')).write_bytes(canonical(result))
        return result

p=argparse.ArgumentParser()
for arg in ('ledger','publication-helper','terms-helper','output'): p.add_argument('--'+arg,type=Path,required=True)
a=p.parse_args(); a.context='matching'; a.resume=False; a.variant='main'; a.deadline_seconds=None; a.pending_finalize_check=False
j=Once(a)
try:
    j.setup()
except StopAfterPublication:
    pass
except Exception:
    j.meta['status']='STOPPED_BEFORE_PUBLICATION'; j.save(); raise

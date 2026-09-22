#!/usr/bin/env python3
"""Closed additive file inventory, excluding this inventory's own self-reference."""
import hashlib,json,subprocess,sys
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parents[2]
def files():
 names=subprocess.check_output(['git','ls-files','-co','--exclude-standard','--',str(HERE)],cwd=REPO,text=True).splitlines()
 return sorted({str((REPO/name).relative_to(HERE)) for name in names if Path(name).name!='ARTIFACTS.json'})
def build():
 return dict(format='r3-artifact-inventory/1',exclusion='ARTIFACTS.json self-reference only; ignored interpreter caches are not repository artifacts',files=[dict(path=p,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()) for p in files() for raw in [(HERE/p).read_bytes()]])
def main():
 path=HERE/'ARTIFACTS.json';raw=json.dumps(build(),indent=2)+'\n'
 if '--write'in sys.argv:path.write_text(raw)
 else:assert path.read_text()==raw,'CLOSED_INVENTORY_MISMATCH';print('closed inventory verified')
if __name__=='__main__':main()

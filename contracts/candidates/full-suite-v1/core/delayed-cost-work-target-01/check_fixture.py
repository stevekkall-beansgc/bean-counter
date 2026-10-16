"""Read-only author consistency/schema check. Not independent qualification."""
from pathlib import Path
import json,shutil,subprocess,sys,tempfile
p=Path(__file__).resolve().parent
with tempfile.TemporaryDirectory() as directory:
 q=Path(directory)
 for name in ['fixture-input.json','delayed-cost-work-target.schema.json','profile.json','profile.schema.json','registry.json','registry.schema.json','definition-provenance.json']:
  shutil.copy2(p/name,q/name)
 subprocess.run([sys.executable,str(p/'derive_author.py'),str(q)],check=True)
 assert json.loads((q/'fixture-author-expected.json').read_text())==json.loads((p/'fixture-author-expected.json').read_text())
 assert json.loads((q/'author-check.json').read_text())==json.loads((p/'author-check.json').read_text())
print('Author consistency PASS; no runtime or independent freeze claim')

"""Author consistency/schema check; not independent or runtime qualification."""
from pathlib import Path
import json,subprocess,sys,tempfile
p=Path(__file__).resolve().parent
with tempfile.TemporaryDirectory() as directory:
 q=Path(directory)
 subprocess.run([sys.executable,str(p/'derive_author.py'),str(q)],check=True)
 for name in ['fixture-author-expected.json','author-check.json']:
  assert json.loads((q/name).read_text())==json.loads((p/name).read_text())
print('Author consistency PASS; no product execution or independent freeze claim')

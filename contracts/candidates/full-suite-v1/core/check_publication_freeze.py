"""Read-only hash gate for named partial encoding freeze, never runtime proof."""
import hashlib,json
from pathlib import Path
p=Path(__file__).parent
freeze=json.loads((p/'publication-encoding-freeze.json').read_text())
assert freeze['status']=='reviewed-partial-encoding-freeze'
for file in freeze['files']:
 assert hashlib.sha256((p/file['path']).read_bytes()).hexdigest()==file['sha256'],file['path']
print('PUB-ENC-01 selected encoding snapshot hashes match; fullsuite/runtime gates remain pending')

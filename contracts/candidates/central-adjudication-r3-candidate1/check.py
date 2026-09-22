#!/usr/bin/env python3
"""Offline complete canonical check; evidence is written only outside candidate."""
import argparse,hashlib,json,os,shutil,subprocess,sys
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parents[2]

def main():
 parser=argparse.ArgumentParser();parser.add_argument('--output-dir',required=True,type=Path);parser.add_argument('--python-only',action='store_true',help='explicit partial author check');args=parser.parse_args();out=args.output_dir.resolve();assert out!=HERE and HERE not in out.parents,'evidence must be outside candidate';out.mkdir(parents=True,exist_ok=True);env=dict(os.environ,PYTHONDONTWRITEBYTECODE='1');runs=[];report=dict(status='RUNNING',runs=runs,actual_store_proofs='PENDING',independent_acceptance='SEPARATE')
 def run(name,argv,expected_error=None):
  result=subprocess.run(argv,cwd=HERE,env=env,capture_output=True);(out/(name+'.stdout')).write_bytes(result.stdout);(out/(name+'.stderr')).write_bytes(result.stderr);passed=result.returncode==0
  if expected_error:
   try:rejection=json.loads(result.stdout);passed=result.returncode==1 and rejection.get('accepted') is False and rejection.get('error')==expected_error
   except (ValueError,TypeError):passed=False
  runs.append(dict(name=name,exit=result.returncode,expected=expected_error or 'success',passed=passed));assert passed,(name,result.stderr[-2000:].decode(errors='replace'));return result.stdout
 try:
  inventory=HERE/'ARTIFACTS.json';assert inventory.exists(),'ARTIFACTS.json required';indexed=json.loads(inventory.read_text())['files'];listed=subprocess.check_output(['git','ls-files','-co','--exclude-standard','--',str(HERE)],cwd=REPO,text=True).splitlines();actual={str((REPO/name).relative_to(HERE)) for name in listed};assert actual=={x['path'] for x in indexed}|{'ARTIFACTS.json'},'CLOSED_FILE_SET'
  for entry in indexed:
   raw=(HERE/entry['path']).read_bytes();assert len(raw)==entry['bytes'] and hashlib.sha256(raw).hexdigest()==entry['sha256'],('inventory',entry['path'])
  for entry in json.loads((HERE/'SOURCE-PROVENANCE.json').read_text())['repository_dependencies']:
   raw=(REPO/entry['path']).read_bytes();assert len(raw)==entry['bytes'] and hashlib.sha256(raw).hexdigest()==entry['sha256'],('source dependency',entry['path'])
  for script in ['derive_schema.py','derive_resources.py','boundary_vectors.py','index_model.py','fixtures.py','tests.py','read_vectors.py','transition_matrix.py','regression_checks.py','authority_checks.py','inventory.py']:run(script.removesuffix('.py'),[sys.executable,str(HERE/script)])
  traces=[HERE/'minimal-trace.json',HERE/'customer-trace.json']+sorted((HERE/'vectors').glob('*.json'));node=os.environ.get('NODE') or shutil.which('node')
  if not args.python_only:assert node and (HERE/'validate.mjs').exists() and (HERE/'node-tests.mjs').exists(),'independent Node validator required';run('node-tests',[node,str(HERE/'node-tests.mjs')])
  for trace in traces:
   name=trace.stem;python=json.loads(run('python-'+name,[sys.executable,str(HERE/'validate.py'),str(trace)]))
   if not args.python_only:
    independent=json.loads(run('node-'+name,[node,str(HERE/'validate.mjs'),str(trace)]));assert python['accepted'] and independent['accepted'];assert python['summary']==independent['summary'],('summary',name);assert python['snapshot']==independent['snapshot'],('snapshot',name)
  negatives=json.loads((HERE/'negative-expectations.json').read_text());assert set(negatives)=={p.name for p in (HERE/'negative-vectors').glob('*.json')},'NEGATIVE_CLOSED_SET'
  for name,code in negatives.items():
   trace=HERE/'negative-vectors'/name;run('python-negative-'+trace.stem,[sys.executable,str(HERE/'validate.py'),str(trace)],code)
   if not args.python_only:run('node-negative-'+trace.stem,[node,str(HERE/'validate.mjs'),str(trace)],code)
  report.update(status='PYTHON_ONLY_INCOMPLETE' if args.python_only else 'CANONICAL_AUTHOR_CHECKS_COMPLETE',trace_count=len(traces),negative_count=len(negatives))
 except Exception as error:report.update(status='FAILED',error=str(error));raise
 finally:(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps(report,indent=2))
if __name__=='__main__':main()

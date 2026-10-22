#!/usr/bin/env python3
"""Build from an immutable local source capture; never a runtime provenance fallback."""
import argparse, hashlib, json, os, re, subprocess
from pathlib import Path

def canonical(value):
    if isinstance(value, dict):
        return '{'+','.join(canonical(k)+':'+canonical(value[k]) for k in sorted(value,key=lambda s:s.encode('utf-16-be')))+'}'
    if isinstance(value, list):
        return '['+','.join(map(canonical,value))+']'
    return json.dumps(value,ensure_ascii=False,separators=(',',':'))
def digest(raw): return hashlib.sha256(raw).hexdigest()
def inventory(files):
    entries=[{'path':p,'bytes':str(len(b)),'sha256':digest(b)} for p,b in files.items()]
    entries.sort(key=lambda e:e['path'].encode('utf-16-be'))
    if not 1<=len(entries)<=8192 or sum(len(b) for b in files.values())>2**63-1:
        raise ValueError('source inventory bounds')
    body={'schema':'ledger-build-source-inventory/1','recipe':'bean-counter-build-inputs/1','entries':entries}
    raw=canonical(body).encode()
    if len(raw)>1048576: raise ValueError('source inventory byte bound')
    return body,'sha256:'+digest(b'bean-counter/build-source-inventory/1\0'+raw)
def command(args,cwd,env=None): return subprocess.check_output(args,cwd=cwd,env=env).decode().strip()
def selected(root):
    paths={p for p in (root/'crates').rglob('*') if p.is_file() and not (len(p.relative_to(root).parts)>2 and p.relative_to(root).parts[2]=='target')}
    paths.update(root/p for p in ['Cargo.toml','Cargo.lock','scripts/build_workflow.py'])
    paths.update(root/p for p in ['rust-toolchain.toml','rust-toolchain','.cargo/config.toml','.cargo/config'] if (root/p).exists())
    # Literal include inputs are part of this capture, including embedded test
    # inputs. Generated OUT_DIR metadata is intentionally excluded from itself.
    for p in tuple(paths):
        if p.suffix=='.rs':
            for spelling in re.findall(r'include(?:_(?:bytes|str))?!\(\s*("(?:[^"\\]|\\.)*")\s*\)',p.read_text()):
                paths.add(p.parent/json.loads(spelling))
    files={}
    for p in paths:
        resolved=p.resolve(strict=True)
        rel=resolved.relative_to(root).as_posix()
        if len(rel)>1024 or any(x in ('','.','..') for x in rel.split('/')): raise ValueError('source path')
        # Check every path component before reading, not merely the final file.
        if any(q.is_symlink() for q in [p,*p.parents] if q!=root and q.is_relative_to(root)) or not resolved.is_file():
            raise ValueError('symlink or nonregular source input')
        if resolved.suffix.lower() in ('.db','.sqlite','.sqlite3','.pem','.key','.p12') or resolved.name.startswith('.env'):
            raise ValueError('private local artifact in build inputs')
        files[rel]=resolved.read_bytes()
    return files

def clean_source(root,files,base,env=None):
    scopes=['crates','Cargo.toml','Cargo.lock','rust-toolchain.toml','rust-toolchain','.cargo','scripts/build_workflow.py']
    # Include the committed selected universe: absent/deleted files cannot
    # disappear from the comparison simply because this capture walks present files.
    committed=command(['git','ls-tree','-r','--name-only',base,'--',*scopes],root,env).splitlines()
    committed={p for p in committed if not (len(Path(p).parts)>2 and Path(p).parts[2]=='target')}
    clean=committed.issubset(files)
    for path,raw in files.items():
        result=subprocess.run(['git','show',base+':'+path],cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
        if result.returncode or result.stdout!=raw: clean=False
    if subprocess.run(['git','diff','--cached','--quiet','HEAD','--',*scopes,*files],cwd=root,env=env).returncode: clean=False
    return clean

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--capture-only',action='store_true')
    parser.add_argument('--test',action='store_true')
    parser.add_argument('--local-execution',action='store_true',help='bind the adopted code-owned local CompilerDescriptor6')
    parser.add_argument('--operator-resources',action='store_true',help='bind stable operator-resource Compiler8 instead of legacy local7')
    parser.add_argument('--conditional-work',action='store_true',help='bind exact additive conditional-work Compiler8 support')
    args=parser.parse_args()
    if args.operator_resources and not args.local_execution: raise ValueError('--operator-resources requires --local-execution')
    if args.conditional_work and not (args.local_execution and args.operator_resources): raise ValueError('--conditional-work requires --local-execution and --operator-resources')
    root=Path(__file__).resolve().parents[1]
    # A declared minimal environment is used for both observed tool versions
    # and the build. Other ambient variables are not effective build inputs.
    env={k:os.environ[k] for k in ['PATH','HOME','CARGO_HOME','RUSTUP_HOME','RUSTUP_TOOLCHAIN','TMPDIR'] if k in os.environ}
    env['LC_ALL']='C'
    # This initial recipe excludes ambient configuration/flags/code generation.
    # Unsupported options refuse instead of silently altering recorded identity.
    overrides={'RUSTC','RUSTDOC','RUSTFLAGS','RUSTDOCFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_ENCODED_RUSTDOCFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','CC','CXX','AR','CFLAGS','CPPFLAGS','CXXFLAGS','LDFLAGS','LIBSQLITE3_SYS_USE_PKG_CONFIG','SQLITE3_LIB_DIR','SQLITE3_INCLUDE_DIR'}
    for key,value in os.environ.items():
        if value and (key in overrides or key.startswith(('CARGO_BUILD_','CARGO_PROFILE_')) or (key.startswith('CARGO_TARGET_') and key.endswith(('_RUSTFLAGS','_LINKER','_RUNNER')))):
            raise ValueError('effective compiler/build override unsupported')
    cargo_home=Path(os.environ.get('CARGO_HOME',Path.home()/'.cargo'))
    for base in [cargo_home,*root.parents]:
        for name in ['config','config.toml']:
            p=base/name if base==cargo_home else base/'.cargo'/name
            if p.exists(): raise ValueError('external cargo configuration unsupported')
    if (root/'.cargo').exists(): raise ValueError('project cargo configuration not yet supported by capture recipe')
    stages=command(['git','ls-files','--stage'],root,env).splitlines()
    if any(line.startswith('160000 ') for line in stages): raise ValueError('submodule build inputs unsupported')
    files=selected(root); inv, inv_digest=inventory(files)
    if any(p.endswith('/build.rs') and p!='crates/ledgerlab/build.rs' for p in files):
        raise ValueError('undeclared build script unsupported')
    base=command(['git','rev-parse','HEAD'],root,env)
    if not re.fullmatch('[0-9a-f]{40}',base): raise ValueError('git base unavailable')
    clean=clean_source(root,files,base,env)
    rustc=command(['rustc','--version'],root,env); cargo=command(['cargo','--version'],root,env)
    if not rustc.startswith('rustc 1.98.1 '): raise ValueError('unqualified rustc')
    target=next(line[6:] for line in command(['rustc','-vV'],root,env).splitlines() if line.startswith('host: '))
    catalogs=root/'contracts/candidates/full-suite-v1/core/runtime-support'
    limits=json.loads((catalogs/'limits.json').read_bytes());mode=json.loads((catalogs/'mode-matrix.json').read_bytes());contract=json.loads((catalogs/'contract-profile.json').read_bytes())
    descriptor={'schema':'ledger-workflow-compiler-descriptor/1','compiler_semantics':'ledger-policy-compiler/1','evaluator':'ledger-workflow-evaluator/1','grammar':'ledger-policy-grammar/1','facts_normalization':'ledger-facts-normalization/1','operation_normalization':'ledger-operation-normalization/2','limits_profile':'workflow-limits/1','primitive_versions':mode['primitives'],'fact_types':mode['fact_types'],'predicate_ops':mode['predicate_ops'],'formula_kinds':mode['formula_kinds'],'plan_schema':'ledger-workflow-compiled-plan/1','limits_digest':'sha256:'+digest(canonical(limits).encode()),'mode_matrix_digest':'sha256:'+digest(canonical(mode).encode()),'contract_manifest_digest':'sha256:'+digest(canonical(contract).encode())}
    if args.local_execution:
        version=8 if args.operator_resources else 7
        descriptor=json.loads((catalogs/('local-conditional-compiler8-data.json' if args.conditional_work else f'local-compiler{version}-data.json')).read_bytes())
        if descriptor['schema']!=f'ledger-workflow-compiler-descriptor/{version}':
            raise ValueError('local compiler descriptor schema')
        local_profile=json.loads((catalogs/'local-conditional-mode.json').read_bytes()) if args.conditional_work else json.loads((root/('contracts/candidates/full-suite-v1/core/local-operator-resources-01/profile.json' if args.operator_resources else 'contracts/candidates/full-suite-v1/core/local-runtime-cap-removal-01/profile.json')).read_bytes())
        local_manifest=json.loads((catalogs/('local-conditional-contract-profile.json' if args.conditional_work else 'local-contract-profile3.json' if args.operator_resources else 'local-contract-profile2.json')).read_bytes())
        local_limits=json.loads((root/('contracts/candidates/full-suite-v1/core/local-operator-resources-01/limits.json' if args.operator_resources else 'contracts/candidates/full-suite-v1/core/local-runtime-cap-removal-01/limits.json')).read_bytes())
        if descriptor['limits_digest']!='sha256:'+digest(canonical(local_limits).encode()):
            raise ValueError('local compiler limits identity')
        if descriptor['mode_matrix_digest']!='sha256:'+digest(canonical(local_profile).encode()) or descriptor['contract_manifest_digest']!='sha256:'+digest(canonical(local_manifest).encode()):
            raise ValueError('local compiler descriptor artifact identity')
    d='sha256:'+digest(b'bean-counter/workflow-document/1\0'+canonical([descriptor['schema'],descriptor]).encode());support={'kind':'workflow-compiler-descriptor','id':'wdoc_'+d[7:],'digest':d}
    package='ledgerlab' if args.test else 'ledgerlab-cli'
    packages=json.loads(command(['cargo','metadata','--no-deps','--format-version','1','--locked','--offline'],root,env))['packages']
    version=next(p['version'] for p in packages if p['name']==package)
    recipe={'package_name':package,'package_version':version,'build_profile':'test' if args.test else 'release','target':target,'features':[],'rustc_version':rustc,'cargo_version':cargo,'rustflags':[],'cargo_lock_digest':'sha256:'+digest(files['Cargo.lock']),'compiler_ref':support}
    inputs={'schema':'ledger-workflow-build-inputs/1','source':{'base_commit':base,'state':'clean_commit' if clean else 'worktree_snapshot','inventory_digest':inv_digest,'file_count':len(files),'total_bytes':str(sum(map(len,files.values())))},'recipe':recipe}
    encoded=canonical(inputs).encode()
    if len(encoded)>65536: raise ValueError('build inputs bound')
    output=args.output.resolve()
    if output.is_relative_to(root/'crates'): raise ValueError('capture output overlaps selected inputs')
    # Cargo discovers config from the actual captured working directory and
    # its ancestors, which may differ from the original checkout's ancestors.
    for base in [output,*output.parents]:
        if any((base/'.cargo'/name).exists() for name in ['config','config.toml']):
            raise ValueError('capture working-directory cargo configuration unsupported')
    output.mkdir(parents=True,exist_ok=False)
    (output/'inventory.json').write_bytes(canonical(inv).encode())
    (output/'build-inputs.json').write_bytes(encoded)
    if args.capture_only:
        print('Source capture written; no executable qualified.')
        return
    capture=output/'source'; capture.mkdir()
    for path,raw in files.items():
        p=capture/path; p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(raw); p.chmod(0o444)
    # Build.rs can embed only this build-time artifact. Runtime never reads it.
    env['LEDGER_WORKFLOW_BUILD_INPUTS_JSON']=str(output/'build-inputs.json')
    build=['cargo','test' if args.test else 'build','--locked','--offline','--package',recipe['package_name'],'--target',target,'--target-dir',str(output/'target')]
    build+=['--no-run'] if args.test else ['--release']
    subprocess.run(build,cwd=capture,env=env,check=True)
    if selected(capture)!=files: raise ValueError('captured build inputs changed during compilation')
    (output/'qualified.json').write_text(json.dumps({'source_inventory_digest':inv_digest,'build_inputs_sha256':digest(encoded),'status':'captured inputs unchanged; local build observation only'},indent=2)+'\n')
    print('Captured build completed; source inventory retained beside artifact.')
if __name__=='__main__': main()

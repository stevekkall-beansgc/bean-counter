#!/usr/bin/env python3
"""Resolved graph and narrow source checks; no production code generation."""
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PRODUCTION = {'ledgerlab-core', 'ledgerlab', 'ledgerlab-cli'}
DENIED_CORE = ('sqlx', 'tokio', 'axum', 'hyper', 'reqwest', 'ureq', 'reqwest',
               'getrandom', 'rand', 'mio', 'rustls', 'openssl', 'async-std', 'surf')


def check_graph(metadata):
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    members = {packages[p]['name'] for p in metadata['workspace_members']}
    assert members == PRODUCTION | {'ledgerlab-testkit'}, members
    for p in packages.values():
        if p['name'] in PRODUCTION:
            assert p['publish'] != [], 'production package unexpectedly unpublished'
            for dep in p['dependencies']:
                if dep['kind'] != 'dev':
                    assert dep['name'] != 'ledgerlab-testkit', 'testkit leaked into production'
        if p['name'] == 'ledgerlab-testkit':
            assert p['publish'] == [], 'testkit must be unpublished'
    core = next(p['id'] for p in packages.values() if p['name'] == 'ledgerlab-core')
    seen = set()

    def walk(key):
        if key in seen:
            return
        seen.add(key)
        name = packages[key]['name']
        assert not any(name == n or name.startswith(n + '-') for n in DENIED_CORE), name
        assert name not in {'ledgerlab', 'ledgerlab-cli', 'ledgerlab-testkit'}, name
        for child in nodes[key]['dependencies']:
            walk(child)
    walk(core)
    allowed = {'ledgerlab-core': set(), 'ledgerlab': {'ledgerlab-core'},
               'ledgerlab-cli': {'ledgerlab'}}
    for p in packages.values():
        if p['name'] in allowed:
            internal = {d['name'] for d in p['dependencies']
                        if d['name'] in members and d['kind'] != 'dev'}
            assert internal <= allowed[p['name']], (p['name'], internal)


def check_source(text, area):
    # Source lint is supplementary: graph checks and review remain necessary.
    code = re.sub(r'/\*.*?\*/|//[^\n]*', '', text, flags=re.S)
    assert not re.search(r'\bunsafe\s*(?:\{|fn\b|impl\b|trait\b)', code), 'unsafe code'
    if area == 'core':
        assert not re.search(r'\b(?:std|tokio)\s*::\s*(?:fs|net|env|process|thread|time)\b|\b(?:SystemTime|Instant|env!|option_env!|getrandom|rand::)', code), 'environment in core'
        assert not re.search(r'\b(?:f32|f64)\b', code), 'float economic core'
    if area == 'store':
        assert not re.search(r'\b(?:evaluate|compile_policy|resolve_authority)\s*\(', code), 'adapter decision logic'
        assert not re.search(r'\b(?:AnyPool|AnyConnection)\b|sqlx\s*::\s*Any\b', code), 'SQLx Any'
    if area in {'cli', 'ts'}:
        assert not re.search(r'\b(?:evaluate|calculate_price|round_atoms)\s*\(', code), 'transport evaluator'


def negatives():
    for text, area in [('use std::fs;', 'core'), ('std::env::var("HOME");', 'core'),
                       ('let x: f64 = 0.0;', 'core'), ('unsafe { x() }', 'core'),
                       ('evaluate(snapshot)', 'store'), ('let p: sqlx::AnyPool;', 'store'),
                       ('calculate_price(event)', 'cli')]:
        try:
            check_source(text, area)
        except AssertionError:
            continue
        raise AssertionError(f'boundary negative accepted: {text}')
    check_source('#![forbid(unsafe_code)]\npub const V: &str = "1";', 'core')


def main():
    negatives()
    for flags in [[], ['--all-features'], ['--no-default-features']]:
        result = subprocess.check_output(['cargo', 'metadata', '--format-version', '1',
                                          '--locked', '--offline', *flags], cwd=ROOT)
        metadata = json.loads(result)
        check_graph(metadata)
        # Transitive denial must fail even when hidden behind an innocuous dependency.
        import copy
        bad = copy.deepcopy(metadata)
        core = next(p for p in bad['packages'] if p['name'] == 'ledgerlab-core')
        bad['packages'].append({'id': 'forbidden', 'name': 'tokio', 'dependencies': []})
        bad['packages'].append({'id': 'bridge', 'name': 'innocuous-helper', 'dependencies': []})
        next(n for n in bad['resolve']['nodes'] if n['id'] == core['id'])['dependencies'].append('bridge')
        bad['resolve']['nodes'].append({'id': 'bridge', 'dependencies': ['forbidden']})
        bad['resolve']['nodes'].append({'id': 'forbidden', 'dependencies': []})
        try:
            check_graph(bad)
        except AssertionError:
            pass
        else:
            raise AssertionError('resolved forbidden dependency passed')
    for path in (ROOT / 'crates').rglob('*.rs'):
        rel = str(path.relative_to(ROOT))
        area = 'core' if '/ledgerlab-core/' in rel else 'store' if '/store/' in rel else 'cli' if '/ledgerlab-cli/' in rel else 'other'
        check_source(path.read_text(), area)
    for directory in ['sdk', 'inspector']:
        for path in (ROOT / directory).rglob('*.ts*'):
            check_source(path.read_text(), 'ts')
    manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
    assert len(manifest['workspace']['members']) == 4
    assert manifest['profile']['release']['overflow-checks'] is True
    print('PASS: three production crates; unpublished testkit; default/all/no-default resolved graphs; source boundaries and negative probes.')


if __name__ == '__main__':
    main()

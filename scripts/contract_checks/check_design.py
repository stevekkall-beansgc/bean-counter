#!/usr/bin/env python3
"""Read-only document checks, not a production canonicalizer or evaluator."""
import argparse
import hashlib
import importlib.metadata
import json
from fractions import Fraction as F
from pathlib import Path
import re
import subprocess

import jsonschema
import yaml


def canonical(value):
    if isinstance(value, dict):
        keys = sorted(value, key=lambda s: s.encode('utf-16-be'))
        return '{' + ','.join(canonical(k) + ':' + canonical(value[k]) for k in keys) + '}'
    if isinstance(value, list):
        return '[' + ','.join(map(canonical, value)) + ']'
    if isinstance(value, int) and not isinstance(value, bool):
        assert abs(value) <= 9007199254740991
    assert value is None or isinstance(value, (str, int, bool))
    return json.dumps(value, ensure_ascii=False, separators=(',', ':'), allow_nan=False)


def digest(kind, value):
    raw = ('ledgerlab/' + kind + '/1').encode() + b'\0' + canonical(value).encode()
    return hashlib.sha256(raw).hexdigest()


def round_away(value):
    q, r = divmod(abs(value.numerator), value.denominator)
    return (-1 if value < 0 else 1) * (q + (2 * r >= value.denominator))


def allocate(total, weights):
    quotas = [F(abs(total)) * w / sum(weights) for w in weights]
    parts = [q.numerator // q.denominator for q in quotas]
    order = sorted(range(len(parts)), key=lambda i: (-(quotas[i] - parts[i]), i))
    for i in order[:abs(total) - sum(parts)]:
        parts[i] += 1
    return [(-p if total < 0 else p) for p in parts]


class JsonYamlLoader(yaml.SafeLoader):
    pass


JsonYamlLoader.yaml_implicit_resolvers = {
    key: [(tag, rex) for tag, rex in rules if tag != 'tag:yaml.org,2002:bool']
    for key, rules in yaml.SafeLoader.yaml_implicit_resolvers.items()
}
JsonYamlLoader.add_implicit_resolver('tag:yaml.org,2002:bool', re.compile(r'^(?:true|false)$'), list('tf'))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--design', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    raw = args.design.read_bytes()
    text = raw.decode('utf-8')
    json_blocks = [json.loads(b) for b in re.findall(r'```json\n(.*?)\n```', text, re.S)]
    yaml_blocks = [yaml.load(b, Loader=JsonYamlLoader) for b in re.findall(r'```yaml\n(.*?)\n```', text, re.S)]
    schemas = {obj['$id']: obj for obj in json_blocks if '$schema' in obj}
    for schema in schemas.values():
        jsonschema.Draft202012Validator.check_schema(schema)
    counts = {'event_examples': 0, 'policy_examples': 0}
    for obj in json_blocks + yaml_blocks:
        for name in ['event', 'policy']:
            if obj.get('schema') == f'ledger-{name}/1':
                jsonschema.Draft202012Validator(schemas[f'urn:ledgerlab:{name}:1']).validate(obj)
                counts[name + '_examples'] += 1

    section = text.split('## 26. First code slice:')[1].split('## 27.')[0]
    event = json.loads(re.search(r'```json\n(.*?)\n```', section, re.S)[1])
    event.update(status='succeeded', unit='call', links=[], evidence=[], extensions={})
    specified = re.search(r'```text\n(.*?)\n```', section, re.S)[1]
    assert canonical(event) == specified
    scope = ['demo', 'sandbox']
    roles = dict(provider='demo-host', cost_originator='demo-host', recipient='demo-host',
                 bearer='demo-customer', payer='demo-customer', beneficiary='demo-customer')
    vectors = {}

    def identity(alias, kind, prefix, value):
        result = prefix + '_' + digest(kind, value)
        vectors[alias] = {'kind': kind, 'input': value, 'id': result}
        return result

    e = identity('E', 'event', 'ev', scope + ['urn:demo:app', 'generation-1'])
    c = identity('C', 'claim', 'cl', [scope, 'urn:demo:app', 'generation-1', 'completion', 'completion'])
    identity('D', 'decision', 'dc', [e])
    identity('R', 'receipt', 'rc', [e])
    f1 = identity('F1', 'effect', 'ef', [scope, 'demo-retail', 'generation.base', c, 'self', 'original'])
    f2 = identity('F2', 'effect', 'ef', [scope, 'demo-retail', 'generation.discount', c, 'self', 'original'])
    a1 = identity('A1', 'action', 'ac', [f1])
    a2 = identity('A2', 'action', 'ac', [f2])
    o = identity('O', 'obligation', 'ob', [scope, 'demo-retail', 'retail', 'USD', 2, roles])
    identity('I', 'intention', 'in', [scope, 'fake', o, sorted([a1, a2])])
    expected = dict(re.findall(r'\| (E|C|D|R|F1|F2|A1|A2|O|I) \| `([^`]+)` \|', section))
    assert {k: v['id'] for k, v in vectors.items()} == expected

    unicode_value = {'\ue000': 'private', '\U0001f600': 'supplementary', 'quantity': '1', 'é': 'é', 'é': 'é'}
    encoded = canonical(unicode_value)
    assert encoded.index('😀') < encoded.index('\ue000')
    assert digest('event', ['é']) != digest('event', ['é'])
    assert canonical({'x': '\u0061'}) == canonical({'x': 'a'})

    math = {}
    def check(name, actual, wanted):
        assert actual == wanted, (name, actual, wanted)
        math[name] = actual
    check('first_slice', [100, -round_away(F(100) * F(20, 100)), 80], [100, -20, 80])
    check('priority', 100 + 20 - round_away(F(100) * F(20, 100)), 100)
    check('unit_fraction', round_away(F('0.07') * F('1.5') * 100), 11)
    check('signed_ties', [round_away(F(s) * 100) for s in ['1.005', '-1.005']], [101, -101])
    check('signed_non_ties', [round_away(F(s) * 100) for s in ['1.0049', '-1.0049']], [100, -100])
    check('additive', 100 - 2 * round_away(F(100, 10)), 80)
    check('sequential', 90 - round_away(F(90, 10)), 81)
    check('equal_allocation', [allocate(100, [F(1)] * 3), allocate(-100, [F(1)] * 3)], [[34, 33, 33], [-34, -33, -33]])
    check('weighted_allocation', allocate(11, [F(1), F(2), F(3)]), [2, 4, 5])
    check('uncapped', [(80 - 8) + (30 - 3) + (40 - 4) + 200, 10 + 5, 15 + 50, 335 - (20 + 15 + 65)], [335, 15, 65, 235])
    credit = -max(0, 135 + 200 - 315)
    share = round_away(F(200 + credit) * F(25, 100))
    check('capped', [credit, 200 + credit, share, 15 + share, 315 - (20 + 15 + 60)], [-20, 180, 45, 60, 220])
    check('share_ceiling', min(round_away(F(1000) * F(25, 100)), 50), 50)
    check('reversal', [-(-20), -(500)], [20, -500])
    check('rounding_stage', [3 * round_away(F('0.004') * 100), round_away(F('0.012') * 100)], [0, 1])
    check('overflow_boundary', (10**30 - 1) + 1 > 10**30 - 1, True)

    node_input = {'event': event, 'vectors': vectors, 'unicode': unicode_value}
    completed = subprocess.run(['node', str(Path(__file__).with_name('check_ids.mjs'))],
                               input=json.dumps(node_input), text=True, capture_output=True, check=True)
    node = json.loads(completed.stdout)
    assert node['ids'] == expected
    assert node['canonical_event'] == specified
    assert node['unicode_canonical'] == encoded
    assert node['ingress_hash'] == digest('ingress', event)
    assert node['event_content_hash'] == digest('event-content', event)

    from check_records import audit
    canonical_records_result = audit()

    result = {
        'status': 'document-and-canonical-record-checks-pass; five encoding blockers closed',
        'design_sha256': hashlib.sha256(raw).hexdigest(),
        'tools': {'jsonschema': importlib.metadata.version('jsonschema'), 'PyYAML': importlib.metadata.version('PyYAML'),
                  'node': subprocess.check_output(['node', '--version'], text=True).strip()},
        'json_blocks': len(json_blocks), 'yaml_blocks': len(yaml_blocks), 'schemas': len(schemas), **counts,
        'canonical_event_utf8': specified, 'canonical_event_hex': specified.encode().hex(),
        'ingress_hash': digest('ingress', event), 'event_content_hash': digest('event-content', event),
        'identities': vectors, 'unicode_canonical': encoded, 'arithmetic_vectors': math,
        'independent_node_check': 'passed',
        'canonical_records': canonical_records_result,
        'limits': ['No Rust or store tests performed.', 'The first-slice journal is frozen; broader Phase 0 and production correctness gates remain.',
                   'This is a restricted document oracle, not production JCS conformance, parser, authority or overflow testing.'],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({k: result[k] for k in ['status', 'json_blocks', 'yaml_blocks', 'schemas', 'event_examples', 'policy_examples', 'independent_node_check']}))
    print(f'PASS: {len(vectors)} published IDs; {len(math)} arithmetic groups; canonical event bytes; Unicode ordering.')
    print('PASS: Python and Node independently reproduce 60 hash vectors, 25 accepted records, 29 manifest members, 16 complete fixture files; schema and negative checks.')


if __name__ == '__main__':
    main()

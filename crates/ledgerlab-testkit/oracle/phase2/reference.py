"""Proposed Phase 2 semantic projection, never a production evaluator or wire codec.

A closed set of story equations over exact Fractions and an append-only list.
No DSL interpreter, Rust imports, store calls, or reads of expected results.
See the testkit fixtures/phase2-proposed-v1/README.md for the deliberately bounded contract.
"""
from copy import deepcopy
from fractions import Fraction
import json
from pathlib import Path
import re
import sys

VERSION = 'ledgerlab-phase2-proposal/1'
POLICY = 'story-tariff/1'
CONFIG = 'story-context/1'
LIMIT = 10**30 - 1
WORK = {'content.generated', 'tool.optimized', 'content.published', 'tool.completed'}
ACQUIRED = 'outcome.acquired'
QUALITY = 'proposal.quality_failed'
REVERSE = 'economic.reversal'
STAGES = {'content.generated': 'generation', 'tool.optimized': 'optimization',
          'content.published': 'publication', 'tool.completed': 'call'}
RELATIONS = {
    'generated_from': ({'content.generated'}, {'content.generated'}),
    'optimized_from': ({'tool.optimized'}, {'content.generated'}),
    'published_as': ({'content.published'}, {'content.generated', 'tool.optimized'}),
    'attributed_to': ({ACQUIRED}, {'content.published'}),
    'consumes_service': ({'content.generated', 'tool.optimized', 'content.published'}, {'tool.completed'}),
    'quality_of': ({QUALITY}, WORK),
}


class Refusal(Exception):
    def __init__(self, code, status='rejected', missing=()):
        self.code, self.status, self.missing = code, status, sorted(missing)


def require(ok, code):
    if not ok:
        raise Refusal(code)


def decimal(token):
    require(isinstance(token, str) and len(token) <= 64 and
            re.fullmatch(r'[0-9]+(?:\.[0-9]{1,18})?', token), 'INPUT_PRECISION')
    normalized = token.lstrip('0') or '0'
    if '.' in normalized:
        normalized = normalized.rstrip('0').rstrip('.') or '0'
    require(len(normalized.replace('.', '').lstrip('0')) <= 30, 'INPUT_PRECISION')
    return Fraction(token)


def bounded(value):
    require(abs(value) <= LIMIT, 'ARITHMETIC_OVERFLOW')
    return value


def rational(value):
    value = Fraction(value)
    require(max(abs(value.numerator).bit_length(), value.denominator.bit_length()) <= 512,
            'ARITHMETIC_OVERFLOW')
    return value


def round_atoms(value):
    value = rational(value)
    magnitude = abs(value)
    # Floor(abs(x) + 1/2) is independent of the quotient/remainder core algorithm.
    rounded = (magnitude + Fraction(1, 2)).numerator // (magnitude + Fraction(1, 2)).denominator
    return bounded(-rounded if value < 0 else rounded)


def percent(token):
    value = decimal(token)
    require(value <= 100, 'POLICY_PERCENT_RANGE')
    return value / 100


def sum_atoms(rows):
    return bounded(sum(int(row['atoms']) for row in rows))


def obligations(rows):
    """Observations are costs, not payable instructions; net-zero owes nothing."""
    groups = {}
    for row in rows:
        if row['book'] not in ('retail', 'supplier'):
            continue
        key = (row['binding'], row['book'])
        groups.setdefault(key, []).append(row)
    return [dict(binding=binding, book=book, roles=group[0]['roles'],
                 atoms=str(sum_atoms(group)), posting_ids=sorted(r['id'] for r in group))
            for (binding, book), group in sorted(groups.items()) if sum_atoms(group)]


class Reference:
    def __init__(self, config):
        self.config = deepcopy(config)
        self.config['suppliers'].sort(key=lambda s: s['agreement'].encode('utf-8'))
        self.journal = []
        self.deliveries = {}
        self.claims = {}
        self.reversed = set()
        self.original_bindings = {}
        self.closed = False
        self.consumed = {s['invocation']['id']: 0 for s in config['suppliers']}

    def state(self):
        rows = [p for d in self.journal for p in d['postings']]
        return dict(revision=str(len(self.journal)), closed=self.closed,
                    reversed=sorted(self.reversed),
                    consumed={k: str(v) for k, v in sorted(self.consumed.items())},
                    totals={book: str(sum_atoms([p for p in rows if p['book'] == book]))
                            for book in ('retail', 'supplier', 'cost_observation')})

    def accepted(self):
        return {d['event']['id']: d for d in self.journal}

    def facts(self, event):
        facts = {k: v for k, v in event.items() if k not in ('id', 'operation')}
        facts['quantity'] = str(decimal(event['quantity']))
        facts['links'] = sorted(event['links'], key=lambda x: (x['relation'], x['source'], x['event']))
        facts['evidence'] = sorted(event['evidence'])
        return facts

    def claim_key(self, event):
        if event['kind'] == ACQUIRED:
            # Namespace/source/claim owns the outcome, never delivery operation label.
            return (event['source'], 'sale', event.get('claim', ''))
        if event['kind'] == QUALITY:
            return (event['source'], 'quality', event.get('claim', ''))
        return (event['source'], event['kind'] == REVERSE, event['operation'])

    def submit(self, event, received):
        # Speculative copy makes every rejection atomic even after an early posting.
        trial = deepcopy(self)
        try:
            result = trial._submit(deepcopy(event), int(received))
        except Refusal as refusal:
            result = dict(status=refusal.status, code=refusal.code)
            if refusal.missing:
                result['missing'] = refusal.missing
        else:
            self.__dict__.update(trial.__dict__)
        return dict(**result, state=self.state())

    def _submit(self, event, received):
        cfg = self.config
        grant = next((g for g in cfg['grants'] if g['source'] == event['source']), None)
        require(grant is not None and grant['active'] and grant['read'], 'SOURCE_UNAUTHORIZED')
        identity = (event['source'], event['id'])
        if identity in self.deliveries:
            old_event, decision = self.deliveries[identity]
            if old_event != event:
                raise Refusal('IDENTITY_CONFLICT', 'conflict')
            return dict(status='duplicate', code='IDENTITY_DUPLICATE', receipt=decision)
        require(event['kind'] in grant['kinds'], 'SOURCE_UNAUTHORIZED')
        key, facts = self.claim_key(event), self.facts(event)
        if key in self.claims:
            old_facts, decision = self.claims[key]
            if old_facts != facts:
                raise Refusal('SEMANTIC_CONFLICT', 'conflict')
            self.deliveries[identity] = (event, decision)
            return dict(status='duplicate', code='SEMANTIC_DUPLICATE', receipt=decision)
        require(cfg['version'] == CONFIG, 'UNKNOWN_CONFIG_VERSION')
        require(cfg['policy_version'] == POLICY, 'UNKNOWN_POLICY_VERSION')
        require(cfg['semantics_version'] == VERSION, 'UNKNOWN_SEMANTICS_VERSION')
        require(event['chain'] == cfg['chain'] and event['customer'] == cfg['customer'], 'CONTEXT_MISMATCH')
        require(event['kind'] in WORK | {ACQUIRED, QUALITY, REVERSE}, 'UNKNOWN_EVENT_KIND')
        require(not self.closed or event['kind'] == REVERSE, 'STAGE_CLOSED')
        require(cfg['retail']['assent_ref'] in cfg['evidence'], 'TERMS_NOT_ACCEPTED')
        require(cfg['retail']['roles']['bearer'] == cfg['retail']['roles']['payer'], 'PAYER_DELEGATION_REQUIRED')
        require(all(ref in cfg['evidence'] for ref in event['evidence']), 'EVIDENCE_MISSING')
        require(len({(l['relation'], l['source'], l['event']) for l in event['links']}) == len(event['links']),
                'DUPLICATE_LINK')
        for link in event['links']:
            require(link['event'] != event['id'], 'LINK_CYCLE')
            require(link['relation'] in RELATIONS and link['relation'] in grant['relations'], 'LINK_UNAUTHORIZED')
        required = {'tool.optimized': 'optimized_from', 'content.published': 'published_as',
                    ACQUIRED: 'attributed_to', QUALITY: 'quality_of'}.get(event['kind'])
        if required and (event['status'] == 'succeeded' or event['kind'] in (ACQUIRED, QUALITY)):
            count = sum(link['relation'] == required for link in event['links'])
            require(count != 0, 'MISSING_LINK')
            require(count == 1, 'AMBIGUOUS_LINK')
        accepted = self.accepted()
        missing = [l['event'] for l in event['links'] if l['event'] not in accepted]
        if event['kind'] == REVERSE:
            require(event.get('targets') and not event['links'], 'INVALID_REVERSAL')
            missing += [t for t in event['targets'] if t not in accepted]
        if event['kind'] == ACQUIRED and 'stage' in cfg:
            missing += [t for t in cfg['stage']['expected'] if t not in accepted]
        if missing:
            raise Refusal('DEPENDENCIES_MISSING', 'waiting', set(missing))
        for link in event['links']:
            parent = accepted[link['event']]['event']
            child_types, parent_types = RELATIONS[link['relation']]
            require(event['kind'] in child_types and parent['kind'] in parent_types, 'LINK_TYPE')
            require(parent['source'] == link['source'], 'LINK_SOURCE')
            require(parent['chain'] == event['chain'] and parent['customer'] == event['customer'], 'CONTEXT_MISMATCH')
            require(parent['id'] not in self.reversed, 'REVERSED_DEPENDENCY')
        require(decimal(event['quantity']) > 0 or event['status'] == 'failed', 'INVALID_QUANTITY')
        decision = dict(id=event['id'], event=event, received=str(received), revision=str(len(self.journal) + 1),
                        authority_refs=sorted([grant['id'], cfg['id'], cfg['retail']['id'],
                                               cfg['retail']['assent_ref'], cfg['policy_version']]),
                        depends_on=[], postings=[], explanations=[], obligations=[])
        if event['kind'] == REVERSE:
            self.reverse(event, decision)
        elif event['kind'] in WORK:
            self.work(event, decision)
        elif event['kind'] == ACQUIRED:
            self.acquisition(event, received, decision)
        else:
            self.quality(event, received, decision)
        decision['obligations'] = obligations(decision['postings'])
        decision['depends_on'] = sorted(set(decision['depends_on']))
        decision['authority_refs'] = sorted(set(decision['authority_refs']))
        # Check whole-book totals before acceptance, including independent obligations.
        for book in ('retail', 'supplier', 'cost_observation'):
            sum_atoms([p for d in self.journal + [decision] for p in d['postings'] if p['book'] == book])
        self.journal.append(decision)
        self.claims[key] = (facts, event['id'])
        self.deliveries[identity] = (event, event['id'])
        return dict(status='accepted', code='ACCEPTED', receipt=event['id'])

    def emit(self, decision, component, book, kind, exact, binding, inputs=(), basis=None, code=None, reverses=None):
        self.original_bindings[(decision['id'], binding['id'])] = deepcopy(binding)
        exact = rational(exact)
        atoms = round_atoms(exact)
        explanation = dict(component=component, code=code or {
            'charge': 'BASE_APPLIED', 'cost': 'BASE_APPLIED', 'premium': 'PREMIUM_APPLIED',
            'discount': 'DISCOUNT_APPLIED', 'credit': 'CAP_APPLIED', 'share': 'SHARE_APPLIED',
            'reversal': 'EXACT_REVERSAL'}[kind],
            unrounded=dict(numerator=str(exact.numerator), denominator=str(exact.denominator)),
            rounded_atoms=str(atoms))
        if basis is not None:
            explanation['basis_atoms'] = str(basis)
        if atoms == 0:
            explanation['code'] = 'ZERO_ROUNDED'
        decision['explanations'].append(explanation)
        if atoms:
            row = dict(id=decision['id'] + '/' + component, component=component, book=book,
                       kind=kind, atoms=str(atoms), binding=binding['id'], roles=deepcopy(binding['roles']),
                       inputs=sorted(inputs))
            if reverses:
                row['reverses'] = reverses
                row['id'] = decision['id'] + '/reverse/' + reverses
            decision['postings'].append(row)
        return atoms

    def supplier_authority(self, supplier, decision):
        cfg, inv = self.config, supplier['invocation']
        require(supplier['roles']['bearer'] == supplier['roles']['payer'], 'PAYER_DELEGATION_REQUIRED')
        require(supplier['policy_version'] == POLICY, 'UNKNOWN_POLICY_VERSION')
        require(supplier['assent_ref'] in cfg['evidence'] and supplier['offer_ref'] in cfg['evidence'],
                'SUPPLIER_TERMS_NOT_ACCEPTED')
        original = next((d['event'] for d in self.journal if d['event']['id'] == inv['event']), decision['event'])
        require(original.get('invocation') == inv['id'] and original['operation'] == inv['operation'] and
                original['source'] == supplier['source'] and original['kind'] == supplier['kind'] and
                inv['binding'] == supplier['id'], 'INVOCATION_MISMATCH')
        require(int(inv['authorized_at']) <= int(original['occurred']) < int(inv['start_before']), 'INVOCATION_ORDER')
        require(inv['chain'] == cfg['chain'] and inv['customer'] == cfg['customer'], 'INVOCATION_MISMATCH')
        require(decimal(original['quantity']) <= decimal(inv['maximum_quantity']), 'INVOCATION_EXPOSURE')
        decision['authority_refs'] += [supplier['id'], supplier['assent_ref'], supplier['offer_ref'], inv['id']]

    def consume(self, supplier, atoms):
        inv = supplier['invocation']
        total = bounded(self.consumed[inv['id']] + atoms)
        require(total <= int(inv['maximum_atoms']), 'INVOCATION_EXPOSURE')
        self.consumed[inv['id']] = total

    def work(self, event, decision):
        cfg = self.config
        if event['status'] == 'failed':
            decision['explanations'].append(dict(component='completion', code='FAILED_WORK'))
            return
        retail = cfg['retail']
        rates = [r for r in cfg['rates'] if r['kind'] == event['kind'] and r['tier'] == cfg['tier']]
        require(len(rates) == 1, 'POLICY_AMBIGUOUS_MATCH' if rates else 'UNKNOWN_TIER')
        rate = rates[0]
        component = STAGES[event['kind']]
        base = self.emit(decision, component + '.base', 'retail', 'charge',
                         decimal(rate['unit_price']) * decimal(event['quantity']) * 10**cfg['scale'], retail)
        for supplier in cfg['suppliers']:
            if supplier['kind'] != event['kind']:
                continue
            self.supplier_authority(supplier, decision)
            amount = self.emit(decision, supplier['component'] + '.base', 'supplier', 'cost',
                               int(supplier['fee_atoms']), supplier)
            self.consume(supplier, amount)
        # Funding applies only to the model observation, not unrelated paid tools.
        if event['kind'] == 'content.generated':
            if cfg['funding'] == 'byok':
                decision['explanations'].append(dict(component='model.observation', code='BYOK_NO_HOST_COST'))
            elif 'model_cost' not in cfg:
                decision['explanations'].append(dict(component='model.observation', code='COST_UNKNOWN'))
            else:
                cost = cfg['model_cost']
                require(cost['evidence_ref'] in cfg['evidence'], 'COST_EVIDENCE_MISSING')
                decision['authority_refs'].append(cost['evidence_ref'])
                self.emit(decision, 'model.observation', 'cost_observation', 'cost', int(cost['atoms']), cost)
        discount = -round_atoms(base * percent(rate['discount_percent']))
        require(base + discount >= 0, 'DISCOUNT_EXCEEDS_BASIS')
        if decimal(rate['discount_percent']):
            self.emit(decision, component + '.discount', 'retail', 'discount',
                      -base * percent(rate['discount_percent']), retail,
                      [p['id'] for p in decision['postings'] if p['component'] == component + '.base'], base)

    def outcome_authority(self, event, received, source, parent, window):
        require(event['source'] == source, 'OUTCOME_AUTHORITY_CONFLICT')
        require(event.get('claim') and event['evidence'], 'OUTCOME_EVIDENCE_REQUIRED')
        start = int(parent['occurred'])
        require(start <= int(event['occurred']) < start + int(window), 'OUTCOME_WINDOW')
        require(received >= int(event['occurred']), 'INVALID_RECEIVED_ORDER')
        require(received <= start + int(window) + int(self.config['report_grace']), 'AUTHORITY_REVIEW_REQUIRED')

    def acquisition(self, event, received, decision):
        cfg = self.config
        parent = self.accepted()[next(l['event'] for l in event['links'] if l['relation'] == 'attributed_to')]['event']
        require(parent['status'] == 'succeeded', 'OUTCOME_AUTHORITY')
        self.outcome_authority(event, received, cfg['outcome_source'], parent, cfg['outcome_window'])
        retail = cfg['retail']
        closure = self.emit(decision, 'acquisition.premium', 'retail', 'premium', int(cfg['premium_atoms']), retail)
        pending_consumption = {}
        for supplier in cfg['suppliers']:
            self.supplier_authority(supplier, decision)
            require(supplier['invocation']['event'] in self.accepted(), 'SUPPLIER_COMPLETION_REQUIRED')
            if int(supplier['premium_atoms']) or decimal(supplier['share_percent']):
                completion = self.accepted()[supplier['invocation']['event']]['event']
                require(completion['status'] == 'succeeded', 'OUTCOME_AUTHORITY')
                require(completion['id'] not in self.reversed, 'REVERSED_DEPENDENCY')
                allowed = completion['id'] == parent['id'] or (
                    completion['kind'] == 'tool.optimized' and any(
                        l['relation'] == 'published_as' and l['event'] == completion['id']
                        and l['source'] == completion['source'] for l in parent['links']))
                require(allowed, 'SUPPLIER_TARGET_PATH')
            amount = 0
            if int(supplier['premium_atoms']):
                amount = self.emit(decision, supplier['component'] + '.premium', 'supplier', 'premium',
                                   int(supplier['premium_atoms']), supplier)
            pending_consumption[supplier['id']] = amount
        net = closure
        if 'stage' in cfg:
            stage = cfg['stage']
            prior = [d for d in self.journal if d['id'] in stage['expected']]
            require(all(d['event']['kind'] in WORK and d['id'] not in self.reversed for d in prior), 'STAGE_INPUT_INVALID')
            rows = [p for d in prior for p in d['postings'] if p['book'] == 'retail']
            p = sum_atoms(rows)
            require(int(stage['cap_atoms']) >= p, 'CAP_BELOW_BOOKED')
            credit = -max(0, bounded(p + closure) - int(stage['cap_atoms']))
            decision['depends_on'] += stage['expected']
            if credit:
                self.emit(decision, 'acquisition.cap-credit', 'retail', 'credit', credit, retail,
                          [r['id'] for r in rows] + [r['id'] for r in decision['postings'] if r['book'] == 'retail'], closure)
            else:
                decision['explanations'].append(dict(component='acquisition.cap-credit', code='CAP_NOT_BINDING'))
            net += credit
            self.closed = True
        shares = [s for s in cfg['suppliers'] if decimal(s['share_percent'])]
        require(len(shares) <= 1, 'MULTIPLE_SHARES')
        for supplier in cfg['suppliers']:
            consumed = pending_consumption[supplier['id']]
            if supplier in shares:
                exact = net * percent(supplier['share_percent'])
                amount = min(round_atoms(exact), int(supplier['share_ceiling_atoms']))
                inputs = [p['id'] for p in decision['postings'] if p['book'] == 'retail']
                # Preserve pre-ceiling rational in the explanation, then cap rounded atoms.
                self.emit(decision, supplier['component'] + '.share', 'supplier', 'share', amount, supplier,
                          inputs, net, 'SHARE_CEILING' if amount < round_atoms(exact) else 'SHARE_APPLIED')
                decision['explanations'][-1]['unrounded'] = dict(numerator=str(exact.numerator), denominator=str(exact.denominator))
                if amount < round_atoms(exact):
                    decision['explanations'][-1]['code'] = 'SHARE_CEILING'
                consumed += amount
            self.consume(supplier, consumed)

    def quality(self, event, received, decision):
        cfg = self.config
        require(cfg['quality_enabled'], 'PROPOSED_EXTENSION_DISABLED')
        require('stage' not in cfg, 'QUALITY_STAGE_UNRESOLVED')
        target = self.accepted()[next(l['event'] for l in event['links'] if l['relation'] == 'quality_of')]
        self.outcome_authority(event, received, cfg['quality_source'], target['event'], cfg['outcome_window'])
        require(not any(d['event']['kind'] == QUALITY and target['id'] in d['depends_on'] for d in self.journal),
                'QUALITY_ALREADY_ADJUSTED')
        rows = [p for p in target['postings'] if p['book'] == 'retail']
        basis = sum_atoms(rows)
        require(basis >= 0, 'NEGATIVE_BASIS')
        decision['depends_on'].append(target['id'])
        self.emit(decision, 'quality.discount', 'retail', 'discount', -basis * percent(cfg['quality_percent']),
                  cfg['retail'], [p['id'] for p in rows], basis)

    def reverse(self, event, decision):
        targets = set(event['targets'])
        require(len(targets) == len(event['targets']), 'INVALID_REVERSAL')
        require(not (targets & self.reversed), 'ALREADY_REVERSED')
        require(event['evidence'], 'REVERSAL_EVIDENCE_REQUIRED')
        originals = [d for d in self.journal if d['id'] in targets]
        require(all(d['event']['kind'] != REVERSE and d['postings'] for d in originals), 'NOTHING_TO_REVERSE')
        missing = {d['id'] for d in self.journal if d['id'] not in self.reversed | targets
                   and set(d['depends_on']) & targets}
        if missing:
            raise Refusal('REVERSAL_DEPENDENTS_REQUIRED', missing=missing)
        for original in originals:
            for row in original['postings']:
                binding = self.original_bindings[(original['id'], row['binding'])]
                require(event['source'] in binding['correction_sources'], 'CORRECTION_UNAUTHORIZED')
                self.emit(decision, row['component'], row['book'], 'reversal', -int(row['atoms']), binding,
                          [row['id']], reverses=row['id'])
                decision['authority_refs'].append(binding['id'])
        decision['depends_on'] += sorted(targets)
        self.reversed |= targets
        # Never rerun prices, release/replenish consumed exposure, or reopen stages.


def calculate(fixture):
    require(fixture['schema'] == VERSION and fixture['status'] == 'proposed', 'UNKNOWN_FIXTURE_VERSION')
    reference = Reference(fixture['config'])
    results = [reference.submit(fixture['events'][step['event']], step['received']) for step in fixture['attempts']]
    return dict(results=results, journal=reference.journal, state=reference.state())


if __name__ == '__main__':
    # Read only: emitted projections are not a fixture-authoring/update command.
    from jsonschema import Draft202012Validator
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from oracle import strict
    data = strict(Path(sys.argv[1]).read_bytes())
    schema_path = Path(__file__).resolve().parents[2] / 'fixtures/phase2-proposed-v1/history.schema.json'
    Draft202012Validator(strict(schema_path.read_bytes())).validate(data)
    print(json.dumps(calculate(data), indent=2, ensure_ascii=False))

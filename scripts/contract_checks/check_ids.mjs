// Independent restricted JSON-byte/hash check. No product code imported.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
if (process.argv[2] === '--journal') {
  const { verify } = await import('./check_records.mjs');
  process.stdout.write(JSON.stringify(verify(process.argv[3] || '.')) + '\n');
  process.exit(0);
}
const input = JSON.parse(readFileSync(0, 'utf8'));
function canonical(v) {
  if (Array.isArray(v)) return '[' + v.map(canonical).join(',') + ']';
  if (v !== null && typeof v === 'object') {
    return '{' + Object.keys(v).sort().map(k => JSON.stringify(k) + ':' + canonical(v[k])).join(',') + '}';
  }
  if (typeof v === 'number' && !Number.isSafeInteger(v)) throw new Error('not a safe integer');
  return JSON.stringify(v);
}
function hash(kind, value) {
  return createHash('sha256').update(`ledgerlab/${kind}/1\0`).update(canonical(value)).digest('hex');
}
const prefixes = { event: 'ev', claim: 'cl', decision: 'dc', receipt: 'rc', effect: 'ef', action: 'ac', obligation: 'ob', intention: 'in' };
const ids = Object.fromEntries(Object.entries(input.vectors).map(([alias, v]) => [alias, prefixes[v.kind] + '_' + hash(v.kind, v.input)]));
process.stdout.write(JSON.stringify({ ids, canonical_event: canonical(input.event), unicode_canonical: canonical(input.unicode), ingress_hash: hash('ingress', input.event), event_content_hash: hash('event-content', input.event) }));

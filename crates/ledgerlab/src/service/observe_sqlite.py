"""Test-only readback. Reads all physical columns; no production evaluator/oracle import."""
import sqlite3, json, sys, datetime

def canon(v):
    if isinstance(v, dict):
        return b'{' + b','.join(canon(k)+b':'+canon(v[k]) for k in sorted(v,key=lambda k:k.encode('utf-16-be'))) + b'}'
    if isinstance(v, list): return b'['+b','.join(map(canon,v))+b']'
    return json.dumps(v,ensure_ascii=False,separators=(',',':')).encode()
def blob(v): return {'bytes':v.hex()} if isinstance(v,bytes) else v
def stamp(us): return (datetime.datetime(1970,1,1,tzinfo=datetime.timezone.utc)+datetime.timedelta(microseconds=us)).strftime('%Y-%m-%dT%H:%M:%S.%fZ')
data={}; inventory={}; pks={}
if sys.argv[1]=='--postgres':
    raw=json.load(sys.stdin)
    for t, item in raw.items():
        data[t]=[{k:bytes.fromhex(v[2:]) if k in item['byte_columns'] and v is not None else v for k,v in r.items()} for r in item['rows']]
        pks[t]=item['pk']
else:
    con=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); con.row_factory=sqlite3.Row
    con.execute('BEGIN')
    tables=[r[0] for r in con.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")]
    for t in tables:
        assert t.replace('_','').isalnum()
        info=list(con.execute('PRAGMA table_info("'+t+'")'))
        pks[t]=[r[1] for r in sorted(info,key=lambda r:r[5]) if r[5]]
        data[t]=[dict(r) for r in con.execute('SELECT * FROM "'+t+'"')]
for t,rows in data.items():
    pk=pks[t]
    inventory[t]=[]; keys=set()
    for row in rows:
        key=canon([blob(row[k]) for k in pk]) if pk else canon({k:blob(v) for k,v in row.items()})
        assert key not in keys, 'duplicate primary key'; keys.add(key)
        inventory[t].append([key.hex(),canon({k:blob(v) for k,v in row.items()}).hex()])
K={'documents':'document','parties':'party','source_grants':'source-grant-record','bindings':'binding-record','snapshots':'snapshot-ref','events':'event','delivery_keys':'delivery-key','claims':'claim','effects':'effect','actions':'action','action_sources':'action-source','action_dependencies':'action-dependency','explanations':'explanation','intentions':'intention','control_transitions':'control-transition','chain_revisions':'chain-revision','decision_manifests':'decision-manifest','accepted_receipts':'receipt'}
exclude={'tenant','environment','id','canonical_bytes','content_hash','schema_version','received_us','observed_us','from_event_count','to_event_count'}
rows=[]; aliases=[]
for table,kind in K.items():
    for r in data[table]:
        scope=[r['tenant'],r['environment']]
        if kind=='delivery-key' and r['kind']=='alias':
            receipt=next(x for x in data['accepted_receipts'] if x['event_id']==r['canonical_event_id'])
            body=json.loads(r['canonical_bytes']);aliases.append(dict(scope=scope,source=r['source'],external_id=r['external_id'],canonical_receipt=receipt['canonical_bytes'].hex(),ingress=canon(body['ingress']).hex(),ingress_hash=r['ingress_hash'],observed_at=stamp(r['observed_us'])))
            continue
        if kind=='delivery-key': identity=[scope,r['source'],r['external_id']]
        elif kind=='action-source': identity=[scope,r['action_id'],r['event_id']]
        elif kind=='action-dependency': identity=[scope,r['action_id'],r['input_action_id']]
        elif kind=='chain-revision': identity=[scope,r['chain_id'],str(r['revision'])]
        else: identity=r['id']
        columns={k:v for k,v in r.items() if k not in exclude and v is not None}
        for name in ['revision','from_revision','to_revision']:
            if name in columns: columns[name]=str(columns[name])
        for name in ['ingress_bytes','match_key_bytes']:
            if name in columns: columns[name+'_hex']=columns.pop(name).hex()
        env=dict(kind=kind,scope=scope,id=identity,content_hash=r['content_hash'],body=json.loads(r['canonical_bytes']))
        if kind=='document': env['document_type']=r['kind']
        index=dict(kind=kind,scope=scope,id=identity,schema_version=r['schema_version'],content_hash=r['content_hash'],canonical_bytes_hex=r['canonical_bytes'].hex(),columns=columns)
        rows.append((kind.encode(),canon(identity),canon(env).hex(),canon(index).hex()))
rows.sort()
i=data['installation'][0];c=data['chains'][0];a=data['authority_heads'][0];b=data['binding_heads'][0]
chain={k:c[k] for k in ['id','customer','currency','scale','binding_set_doc','context_doc']};chain.update(revision=str(c['revision']),event_count=str(c['event_count']))
principals=set(r['principal_id'] for r in data['source_grants'])
principals.update(json.loads(r['canonical_bytes'])['acceptor'] for r in data['documents'] if r['kind']=='assent')
state=dict(schema='ledger-first-slice-state/1',scope=[i['tenant'],i['environment']],logical_store_id=i['logical_store_id'],mode=i['mode'],admission=i['admission'],dispatch_enabled=bool(i['dispatch_enabled']),dispatch_hold=bool(i['dispatch_hold']),credentials='excluded',principals=sorted(principals),chain=chain,authority_head=dict(id=a['id'],grant_id=a['grant_id'],revision=str(a['revision']),active=bool(a['active'])),binding_head=dict(id=b['id'],binding_id=b['binding_id'],selector_doc=b['selector_doc'],revision=str(b['revision']),active=bool(b['active'])))
operational=None
if data['events']:
    e=data['events'][0];key=next(r for r in data['delivery_keys'] if r['kind']=='original' and r['canonical_event_id']==e['id'])
    operational=dict(schema='ledger-first-slice-operational/1',scope=[i['tenant'],i['environment']],chain=chain,received_at=stamp(e['received_us']),delivery_key_observed_at=stamp(key['observed_us']))
    if data['delivery_state']:
        d=data['delivery_state'][0];operational['delivery_state']=dict(intention_id=d['intention_id'],state=d['state'],attempts=str(d['attempts']),generation=str(d['generation']),next_attempt_at=stamp(d['next_attempt_us']))
        for k in ['lease_owner','lease_until_us','last_observation']:
            if d[k] is not None: operational['delivery_state'][k]=d[k]
print(json.dumps(dict(journal=[r[2] for r in rows],indexes=[r[3] for r in rows],state=canon(state).hex(),operational=canon(operational).hex() if operational else None,aliases=aliases,rows=inventory)))

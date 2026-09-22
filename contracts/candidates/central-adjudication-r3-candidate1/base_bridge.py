"""Independent byte/math reconstruction of retained original-profile base input."""
import hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
sys.dont_write_bytecode=True

def verify(objects,p,canonical,require):
 rows=[json.loads(__import__('base64').b64decode(o['body'])) for o in objects]
 envelopes=[r for r in rows if type(r)is dict and {'kind','scope','id','body','content_hash'}<=set(r)]
 require(len(envelopes)>0,'BASE_ENVELOPES')
 require(all(r['scope']==p['scope'] for r in envelopes),'BASE_SCOPE')
 def h(d,x,version):return hashlib.sha256(('ledgerlab/'+d+'/'+version).encode()+b'\0'+canonical(x)).hexdigest()
 v2=any(r['kind']=='base-acceptance' for r in envelopes)
 if not v2:
  sys.path.insert(0,str(ROOT/'scripts/contract_checks'))
  from check_records import verify_journal
  import jsonschema
  validator=jsonschema.Draft202012Validator(json.loads((ROOT/'contracts/schemas/v1/canonical-records.schema.json').read_text()))
  for row in rows:validator.validate(row)
  seeds=[r for r in rows if r['kind']=='document' and r['document_type']!='snapshot'];accepted=[r for r in rows if r not in seeds]
  verify_journal(sorted(accepted,key=lambda r:(r['kind'].encode(),canonical(r['id']))),seeds)
  require(all(r['body'].get('schema','').endswith('/1') or r['kind'] in {'action-source','action-dependency','chain-revision','delivery-key'} for r in envelopes),'BASE_PROFILE')
  actions=[r for r in envelopes if r['kind']=='action'];retail=sum(int(r['body']['amount']['atoms']) for r in actions if r['body']['book']=='retail');supplier=sum(int(r['body']['amount']['atoms']) for r in actions if r['body']['book']=='supplier')
  require(retail==int(p['base_atoms']) and supplier==int(p['supplier_booked']),'BASE_AMOUNT')
  manifests=[r for r in envelopes if r['kind']=='decision-manifest'];receipts=[r for r in envelopes if r['kind']=='receipt'];require(len(manifests)==len(receipts)==1,'BASE_CLOSURE')
  m=manifests[0];rc=receipts[0];require(rc['body']['decision_hash']==m['content_hash'] and rc['body']['decision_id']==m['id'],'BASE_RECEIPT')
  # Membership may include reused immutable documents provided in the same inventory.
  members={(r['kind'],canonical(r['id'])):r for r in envelopes}
  for entry in m['body']['members']:
   require((entry['kind'],canonical(entry['id'])) in members,'BASE_MEMBER_MISSING');require(members[entry['kind'],canonical(entry['id'])]['content_hash']==entry['content_hash'],'BASE_MEMBER_HASH')
  return
 sys.path.insert(0,str(ROOT/'scripts/contract_checks/v2_candidate'))
 from audit import verify as original_verify
 from profile import row_order,reference
 acceptance=next(r for r in rows if r['kind']=='base-acceptance')
 original_verify(dict(status='candidate-not-frozen',name='r3-retained-base',target_admission='accepted',seed=sorted(rows,key=row_order),decisions=[],probes=[]),reference(acceptance))
 version='2-candidate.4';prefix={'evidence':'ed','policy-snapshot':'po','event':'ev','base-posting':'bp','target-basis':'tb','obligation':'ob','base-identity':'bi','binding-snapshot':'bs','base-evaluation':'be','target-snapshot':'ts','base-acceptance':'ba'}
 for r in envelopes:
  k=r['kind'];b=r['body'];s=r['scope'];require(k in prefix,'BASE_KIND');require(b['schema']=='ledger-'+k+'/'+version,'BASE_PROFILE')
  require(r['content_hash']=='sha256:'+h('record-content',[k,2,b],version),'BASE_RECORD_HASH')
  if k in {'evidence','policy-snapshot','target-basis','binding-snapshot','base-evaluation','target-snapshot'}:v=[s,b]
  elif k=='base-identity':v=[s,b['target'],b['original_kind'],b['original_id']]
  elif k=='event':v=[s,b['data']['source'],b['data']['external_id']]
  elif k=='base-posting':v=[s,b['event_id'],b['agreement_id'],b['book'],b['ordinal']]
  elif k=='obligation':v=[s,b['agreement_id'],b['book'],b['currency'],b['scale'],b['roles']]
  else:v=[b['target']]
  require(r['id']==prefix[k]+'2_'+h(k,v,version),'BASE_ID')
 byid={(r['kind'],canonical(r['id'])):r for r in envelopes};require(len(byid)==len(envelopes),'BASE_DUPLICATE')
 bases=[r for r in envelopes if r['kind']=='base-acceptance'];require(len(bases)==1,'BASE_CLOSURE');b=bases[0]['body'];require(b['target']==p['target'],'BASE_TARGET')
 members=b['members'];require(len(members)==len(envelopes)-1,'BASE_MEMBER_COUNT')
 for q in members:require((q['kind'],canonical(q['id'])) in byid and byid[q['kind'],canonical(q['id'])]['content_hash']==q['content_hash'],'BASE_MEMBER_HASH')
 receipt=json.loads(b['original_receipt_utf8']);require(canonical(receipt).decode()==b['original_receipt_utf8'],'BASE_RECEIPT_CANONICAL');require(receipt['membership_hash']=='sha256:'+h('base-membership',members,version),'BASE_MEMBERSHIP_HASH');require(receipt['target']==b['target'],'BASE_RECEIPT_TARGET')
 baseeval=next(r['body'] for r in envelopes if r['kind']=='base-evaluation');material=json.loads(baseeval['evaluation_utf8']);require(canonical(material).decode()==baseeval['evaluation_utf8'],'BASE_EVAL_CANONICAL')
 postings=[r['body'] for r in envelopes if r['kind']=='base-posting'];actual={book:sum(int(r['amount']['atoms']) for r in postings if r['book']==book) for book in ['retail','supplier']};require(actual=={'retail':int(p['base_atoms']),'supplier':int(p['supplier_booked'])},'BASE_AMOUNT')
 for book in actual:require(sum(int(a['amount']['atoms']) for a in material['actions'] if a['book']==book)==actual[book],'BASE_EVAL_AMOUNT')
 # Independent fixed-price first-work arithmetic; no candidate evaluator call.
 prices={pol['binding']['id']:pol for pol in material['bundle']['policies']}
 for a in material['actions']:
  pol=prices[a['binding']['id']];rule=next(r for r in pol['rules'] if r['component']==a['component']);op=rule['operation'];require(op['kind']=='base' and op['price']['kind']=='fixed','BASE_PRICE_PROFILE');from decimal import Decimal
  atoms=Decimal(op['price']['value'])*(10**material['bundle']['scale']);require(atoms==atoms.to_integral() and int(atoms)==int(a['amount']['atoms']),'BASE_FIXED_PRICE');require(a['binding']['roles']==pol['binding']['roles'],'BASE_ROLES')
 require(len(material['actions'])==len(postings),'BASE_ACTION_CLOSURE')
 if actual['supplier']:
  require(len(material['invocations'])>0,'BASE_INVOCATION')
  require(sum(int(c['consume']['atoms']) for c in material['consumptions'])==actual['supplier'],'BASE_CONSUMPTION')

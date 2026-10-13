"""Deterministic authoring helper for UNFROZEN candidate shapes; not an oracle."""
import json
from pathlib import Path
D={}
def r(n): return {'$ref':'#/$defs/'+n}
def s(pattern=None,maxLength=128):
 d={'type':'string','minLength':1,'maxLength':maxLength}
 if pattern:d['pattern']=pattern
 return d
def c(v):return {'const':v}
def e(*v):return {'enum':list(v)}
def a(item,n=64,minimum=0):return {'type':'array','items':item,'minItems':minimum,'maxItems':n,'uniqueItems':True}
def o(fields,optional=()):return {'type':'object','properties':fields,'required':[k for k in fields if k not in optional],'additionalProperties':False}
def seq(item,n=64,minimum=0):return {'type':'array','items':item,'minItems':minimum,'maxItems':n}
def union(*v):return {'oneOf':list(v)}
def define(n,v):D[n]=v;return r(n)
def doc(n,tag,fields,optional=()):return define(n,o({'schema':c(tag),**fields},optional))
N=define('Name',s(r'^[A-Za-z][A-Za-z0-9_.-]{0,62}:[A-Za-z][A-Za-z0-9_.-]{0,63}$'))
I=define('Id',s(r'^[^\u0000-\u001f\u007f]+$'))
C=define('Counter',s(r'^(0|[1-9][0-9]{0,18})$',19))
V=define('Version',s(r'^[1-9][0-9]{0,18}$',19))
SI=define('SignedInt',s(r'^(0|-?[1-9][0-9]{0,29})$',31))
U=define('UnsignedInt',s(r'^(0|[1-9][0-9]{0,154})$',155))
P=define('PositiveInt',s(r'^[1-9][0-9]{0,154}$',155))
DEC=define('Decimal',s(r'^(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$',64))
SD=define('SignedDecimal',s(r'^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$',64))
T=define('Time',s(r'^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z$',27))
H=define('Digest',s(r'^sha256:[0-9a-f]{64}$',71))
RAW=define('RawSha256',s(r'^[0-9a-f]{64}$',64))
SRC=define('SourceId',s(maxLength=256))
SCOPE=define('Scope',{'type':'array','prefixItems':[I,I],'items':False,'minItems':2,'maxItems':2})
RK=define('RecordKind',e('workflow-product','workflow-fact-schema','workflow-policy','workflow-slot-template','workflow-slot-catalog','workflow-slot-instantiation','workflow-compiler-descriptor','workflow-compiled-plan','workflow-binding','workflow-publication','workflow-work','workflow-target','workflow-outcome','workflow-action','workflow-stage-instance','workflow-invocation','workflow-cost','workflow-link','workflow-activation','workflow-authority','workflow-assent','workflow-roles','workflow-offer','workflow-delegation','workflow-evidence','workflow-fact-attestation','workflow-verification','workflow-correction','workflow-decision','workflow-capacity','workflow-source-grant','workflow-operation','workflow-head-transition','workflow-manifest','workflow-receipt','legacy-event','legacy-document'))
REF=define('Ref',o({'kind':r('RecordKind'),'id':I,'digest':H}))
D['Ref']['description']='Kind and ID prefix must pass the compiled record-family registry; see normative-amendment.md. Structural acceptance alone is never authorization.'
DR=define('DocRef',REF.copy())
AM=define('Amount',o({'currency':c('USD'),'scale':c(18),'atoms':SI}))
NA=define('NonnegativeAmount',o({'currency':c('USD'),'scale':c(18),'atoms':s(r'^(0|[1-9][0-9]{0,29})$',30)}))
RATIO=define('Ratio',o({'numerator':U,'denominator':P}))
LAB=define('Labels',o({'title':s(maxLength=128),'description':s(maxLength=1024)},('description',)))
BOOK=e('retail','supplier','cost_observation')
EK=define('EvidenceKind',e('occurrence','outcome','supplier_final_cost','meter_quantity','allocation_input'))
PROV=define('Provenance',union(o({'kind':c('caller')}),o({'kind':c('attested'),'source_slot':N,'evidence_kind':EK})))
TYPE=define('TypeDef',union(o({'kind':c('boolean')}),o({'kind':c('string'),'min_bytes':C,'max_bytes':C}),o({'kind':c('enum'),'values':a(N,32,1)}),o({'kind':c('integer'),'min':SI,'max':SI,'unit':N},('unit',)),o({'kind':c('decimal'),'min':SD,'max':SD,'max_scale':C,'unit':N},('unit',)),o({'kind':c('timestamp'),'min':T,'max':T}),o({'kind':c('reference'),'record_kind':r('RecordKind'),'relation':e('same_scope_customer','same_binding','same_invocation')})))
FIELD=define('FieldDef',o({'name':N,'required':{'type':'boolean'},'type':TYPE,'provenance':PROV}))
FS=doc('FactSchema','ledger-workflow-fact-schema/1',{'schema_id':N,'version':V,'normalization':c('ledger-facts-normalization/1'),'fields':a(FIELD)})
SCALAR=define('FactScalar',union({'type':'boolean'},{'type':'string','maxLength':1024},REF))
FACTS=define('Facts',{'type':'object','propertyNames':N,'additionalProperties':SCALAR,'maxProperties':64})
D['Facts']['description']='Dynamic keys are declared by an exact retained FactSchema; reject undeclared fields and validate original types/provenance at runtime. This is the only dynamic business-field object.'
UNIT=define('UnitDef',o({'name':N,'labels':LAB,'dimension':N,'normalization':o({'to':N,'factor':RATIO})},('normalization',)))
ACT=define('ActivityDef',o({'name':N,'labels':LAB,'primitive':e('work','outcome'),'fact_schema':DR,'source_slot':N,'funding_leg':N},('funding_leg',)))
LINK=define('LinkDef',o({'name':N,'child_activity':N,'predecessor_activities':a(N,16,1),'child_primitive':e('work','outcome'),'predecessor_primitive':e('work','outcome'),'minimum':e('0','1'),'maximum':e(*map(str,range(1,9))),'binding_relation':e('same_binding','same_agreement','declared_supplier'),'supplier_slot':N,'on_missing':e('refuse','wait'),'max_depth':C},('supplier_slot',)))
PROD=doc('Product','ledger-workflow-product/1',{'product_id':N,'version':V,'labels':LAB,'profile':c('workflow/1'),'evaluator':c('ledger-workflow-evaluator/1'),'grammar':c('ledger-policy-grammar/1'),'limits_profile':c('workflow-limits/1'),'units':a(UNIT,16,1),'activities':a(ACT,16,1),'fact_schemas':a(DR,16,1),'links':a(LINK,32)})
TC=define('TemplateComponent',o({'key':N,'book':BOOK,'root_key':N,'role_slot':N,'aliases':a(N,64)}))
TF=define('TemplateFamily',o({'key':N,'target_component_key':N,'role_slot':N,'aliases':a(N,64)}))
ST=doc('SlotTemplate','ledger-workflow-slot-template/1',{'template_id':N,'version':V,'previous':DR,'components':a(TC,128,1),'families':a(TF,64)},('previous',))
CAT=doc('SlotCatalog','ledger-workflow-slot-catalog/1',{'agreement_id':I,'customer':I,'revision':C,'previous':DR,'components':a(o({'slot_id':I,'book':BOOK,'root_slot':I,'role_slot':N}),128,1),'families':a(o({'slot_id':I,'target_component_slot':I,'role_slot':N}),64)},('previous',))
MAP=o({'template_key':N,'slot_id':I})
INST=doc('SlotInstantiation','ledger-workflow-slot-instantiation/1',{'agreement_id':I,'customer':I,'revision':C,'previous':DR,'template_ref':DR,'catalog_ref':DR,'component_map':a(MAP,128,1),'family_map':a(MAP,64)},('previous',))
OPND=define('Operand',union(o({'fact':N}),o({'binding_fact':N}),o({'context':e('status','funding_mode','source')})))
PRED=define('Predicate',union(o({'op':e('true','false')}),o({'op':e('eq','lt','le','gt','ge'),'field':OPND,'value':SCALAR}),o({'op':c('in'),'field':OPND,'values':a(SCALAR,32,1)}),o({'op':e('all','any'),'args':seq(r('Predicate'),8,1)}),o({'op':c('not'),'arg':r('Predicate')})))
BASIS=define('SelfBasis',o({'component_key':N,'view':e('booked_base','booked_premium','prior_net')}))
FP=define('FixedOrPercent',union(o({'kind':c('fixed'),'amount':DEC}),o({'kind':c('percent'),'percent':DEC})))
FORM=define('Formula',union(o({'kind':c('fixed'),'amount':DEC}),o({'kind':c('unit'),'quantity_field':N,'unit':N,'rate':DEC,'normalize_to':N},('normalize_to',)),o({'kind':c('percent'),'basis':BASIS,'percent':DEC}),o({'kind':c('discount'),'basis':BASIS,'mode':e('additive','sequential'),'amount':FP}),o({'kind':c('final_cost'),'evidence_slot':N,'supplier_slot':N,'multiplier':RATIO,'markup':DEC,'ceiling':NA}),o({'kind':c('supplier_final_cost'),'evidence_slot':N,'ceiling':NA}),o({'kind':c('observe_cost'),'evidence_slot':N}),o({'kind':c('closure_cap'),'stage':N,'basis_component_key':N,'ceiling':NA}),o({'kind':c('closure_share'),'basis_component_key':N,'percent':DEC,'ceiling':NA,'allocation_view':{'type':'boolean'}})))
SELECT=define('Selector',o({'path':seq(N,4),'target_activity':N,'binding_slot':N,'component_key':N,'basis':c('booked_original_net')}))
WIN=define('Window',o({'anchor':e('work_accepted_at','verified_predecessor_occurred_at'),'anchor_path':seq(N,4),'start_offset_us':C,'occurrence_us':C,'receipt_grace_us':C,'acceptance_grace_us':C,'activation_deadline_us':C,'correction_us':C}))
RULE=define('Rule',o({'rule_id':N,'activity':N,'component_name':N,'component_key':N,'binding_slot':N,'phase':e('base','premium','discount','cost','cap','share'),'when':PRED,'formula':FORM,'dependency':e('on_missing_refuse','on_missing_wait')}))
FAM=define('OutcomeFamily',o({'family_name':N,'family_key':N,'trigger_activity':N,'target_link_slot':N,'selector':SELECT,'source_slot':N,'code_field':N,'codes':a(o({'code':N,'effect':union(o({'kind':c('none')}),o({'kind':e('premium','discount'),'amount':FP}))}),32,1),'window':WIN,'maximum_premium':NA,'maximum_discount':NA,'correction_source_slot':N}))
STAGE=define('StageDef',o({'name':N,'closure_activity':N,'closure_component_key':N,'closure_source_slot':N,'closure_evidence_kind':c('outcome'),'window':WIN,'expected_slots':a(o({'key':N,'source_slot':N,'activity':N,'component_keys':a(N,128,1)}),32,1)}))
BS=define('BindingSlot',o({'name':N,'book':BOOK,'funding_leg':N,'allowed_activities':a(N,16,1),'correction_source_slot':N}))
POL=doc('Policy','ledger-workflow-policy/1',{'policy_id':N,'version':V,'previous':DR,'product_ref':DR,'context_schema_ref':DR,'slot_template_ref':DR,'compiler_ref':DR,'currency':c('USD'),'scale':c(18),'rounding':c('nearest_ties_away'),'mode':e('component_workflow','closure_share'),'binding_slots':a(BS,16,1),'rules':seq(RULE),'families':a(FAM),'stages':a(STAGE,32),'delayed_costs':a(o({'component_key':N,'evidence_source_slot':N,'finalize_within_us':C,'on_missing':c('wait')}),64),'allocation':o({'primitive':c('allocation/scale18-toward-zero-bean-residual/1'),'evidence_slot':N,'bean_role_slot':N})},('previous','allocation'))
CD=doc('CompilerDescriptor','ledger-workflow-compiler-descriptor/1',{'compiler_semantics':c('ledger-policy-compiler/1'),'evaluator':c('ledger-workflow-evaluator/1'),'grammar':c('ledger-policy-grammar/1'),'facts_normalization':c('ledger-facts-normalization/1'),'operation_normalization':c('ledger-operation-normalization/2'),'limits_profile':c('workflow-limits/1'),'limits_digest':H,'primitive_versions':a(I),'fact_types':a(I),'predicate_ops':a(I),'formula_kinds':a(I),'mode_matrix_digest':H,'plan_schema':I,'contract_manifest_digest':H})
CTX=define('AcceptedContext',o({'fact_schema_ref':DR,'facts':FACTS}))
FL=define('FundingLeg',o({'name':N,'mode':e('customer_api','customer_subscription','customer_local','platform'),'resource_owner':I,'payer':I,'provider':I,'evidence_refs':a(REF,16),'supplier_binding_slot':N},('provider','supplier_binding_slot')))
bfields={'agreement_id':I,'version':V,'customer':I,'source_namespace':SRC,'policy_ref':DR,'product_ref':DR,'fact_schema_refs':a(DR,16,1),'slot_catalog_ref':DR,'slot_instantiation_ref':DR,'compiled_plan_ref':REF,'evaluator':c('ledger-workflow-evaluator/1'),'roles_refs':a(REF,16,1),'assent_refs':a(REF,16,1),'authority_refs':a(REF,32,1),'source_slots':a(o({'name':N,'sources':a(SRC,16,1),'evidence_kinds':a(EK,5)}),32,1),'accepted_context':CTX,'bindings':a(o({'slot':N,'binding_ref':REF}),16,1),'funding_legs':a(FL,16,1),'starts_at':T,'ends_at':T,'quantity_ceilings':a(o({'activity':N,'field':N,'unit':N,'maximum':DEC}),64),'exposure_ceilings':a(o({'binding_slot':N,'maximum':NA}),16),'accepted_by':I,'accepted_at':T}
BIND=doc('Binding','ledger-workflow-binding/1',bfields,('ends_at',))
ALL_OPS=['installation.initialize','installation.upgrade','evidence.retain','policy.publish','agreement.accept','agreement.activate','agreement.revoke','authority.change','invocation.authorize','invocation.close','work.accept','link.assert','outcome.submit','outcome.correct','outcome.close','work.reverse','term.set','cumulative_agreement.set','activity.submit','quantity.correct','recurrence.set','recurrence.cancel','occurrence.accept','fiscal.set','period.resolve','period.close','statement.adjustments.issue','settlement.close','cost.finalize','cost.correct','connector.configure','connector.pause','connector.reconcile','connector.resume','dispatch.hold','dispatch.resume','settlement.dispatch','delivery.enqueue','backup.create','backup.retrieve','backup.verify','backup.prune','restore.stage','restore.verify','restore.activate','restore.resume','meter.ingest','meter.finalize','meter.correct','service.rate.accept','deployment.apply','deployment.pause','deployment.resume','deployment.export','deployment.offboard','credential.issue','credential.rotate','credential.revoke','producer.enroll','producer.revoke','provider.observation.record','lifecycle.target.close','recovery.bundle.import','settlement.credit.allocate']
COP=define('ClosedOperationKind',e(*ALL_OPS))
SEL=define('NameSelector',union(o({'names':a(N,64,1)}),o({'namespace':s(r'^[A-Za-z][A-Za-z0-9_.-]{0,62}:$',64)})))
GRANT=doc('SourceGrant','ledger-workflow-source-grant/1',{'grant_id':I,'revision':C,'principal_id':I,'source':SRC,'canonical_economic_namespace':SRC,'customer_scope':a(I,64,1),'operations':a(COP,64,1),'product_selectors':a(SEL,64),'activity_selectors':a(SEL,64),'link_selectors':a(SEL,32),'source_slots':a(N,32),'permissions':a(e('read','submit','correct','link','admin','invocation','close','operate'),8,1),'starts_at':T,'ends_at':T,'original_ceiling_ref':REF})
OFFER=doc('Offer','ledger-workflow-offer/1',{'agreement_id':I,'version':V,'provider':I,'policy_ref':DR,'valuation_mode':e('fixed_unit','final_evidence'),'role_refs':a(REF,16,1),'activity_names':a(N,16,1),'unit_ceilings':a(o({'activity':N,'field':N,'unit':N,'maximum':DEC}),64),'exposure_ceiling':NA,'allowed_component_slots':a(I,128,1),'source_slots':a(N,32,1),'starts_at':T,'ends_at':T,'evidence_refs':a(REF,16,1)})
# Derived publication copies and trusted acceptance metadata do not belong to caller inputs.
inputs={k:v for k,v in bfields.items() if k not in {'accepted_by','accepted_at','policy_ref','product_ref','fact_schema_refs','compiled_plan_ref','evaluator'}}
BI=define('BindingInputs',o(inputs,('ends_at',)))
ACCEPT=doc('AgreementAccept','ledger-workflow-agreement-accept/1',{'agreement_id':I,'expected_revision':C,'publication_ref':REF,'slot_catalog_ref':DR,'slot_instantiation_ref':DR,'binding_inputs':BI,'assent_refs':a(REF,16,1),'authority_refs':a(REF,32,1)})
af={'agreement_id':I,'expected_revision':C,'accepted_binding_ref':REF,'policy_ref':DR,'product_ref':DR,'fact_schema_refs':a(DR,16,1),'compiled_plan_ref':REF,'effective':union(o({'kind':c('at_commit')}),o({'kind':c('not_before'),'at':T})),'evidence_refs':a(REF,16)}
ACTIVATE=doc('AgreementActivate','ledger-workflow-activation/1',af)
ACTREPL=doc('AgreementActivationReplace','ledger-workflow-activation-replace/1',{**af,'replaces_transition_ref':REF})
PROOF=define('FactProof',o({'field':N,'attestation_ref':REF}))
LIN=define('LinkInput',o({'slot':N,'predecessor':REF}))
WORK=doc('Work','ledger-workflow-work/1',{'external_event_id':I,'business_operation_id':I,'customer':I,'chain':I,'activity':N,'product_ref':DR,'policy_ref':DR,'fact_schema_ref':DR,'binding_ref':REF,'status':e('succeeded','failed'),'facts':FACTS,'fact_proofs':a(PROOF),'links':a(LIN,32),'evidence_refs':a(REF,16),'invocation_refs':a(REF,16),'stage_instance_ref':REF,'occurred_at':T},('stage_instance_ref','occurred_at'))
COR=define('CorrectionClosure',o({'decision_ref':REF,'expected_revision':C,'authority_evidence_refs':a(REF,16,1)}))
QC=doc('QuantityCorrection','ledger-workflow-quantity-correction/1',{'work_ref':REF,'field':N,'replacement_quantity':DEC,'expected_revision':C,'evidence_refs':a(REF,16),'fact_proofs':a(PROOF),'dependent_reversals':a(COR,31)})
KEY=define('OperationKey',o({'logical_store_id':I,'scope':SCOPE,'source':SRC,'operation_id':I}))
# Publication transport is an explicit initial syntactic transcription choice, not compiler output ABI.
DOCUMENT=define('Document',union(FS,PROD,ST,CAT,INST,POL,CD,BIND,GRANT,OFFER))
PUBLISH=define('PolicyPublish',union(o({'schema':c('ledger-workflow-policy-publication/1'),'mode':c('inline'),'documents':a(union(FS,PROD,ST,POL,CD),128,1),'root_policy_ref':DR,'expected_plan_digest':H}),o({'schema':c('ledger-workflow-policy-publication/1'),'mode':c('retained'),'document_refs':a(DR,128,1),'root_policy_ref':DR,'expected_plan_digest':H})))
OUTCOME=doc('OutcomeSubmit','ledger-workflow-outcome/1',{'target_ref':REF,'family_slot':I,'business_outcome_token':I,'activity':N,'product_ref':DR,'policy_ref':DR,'fact_schema_ref':DR,'binding_ref':REF,'facts':FACTS,'fact_proofs':a(PROOF),'evidence_refs':a(REF,16),'occurred_at':T})
OUTCOR=doc('OutcomeCorrect','ledger-workflow-outcome-correction/1',{'original_claim_ref':REF,'expected_revision':C,'replacement':union(OUTCOME,o({'kind':c('none'),'evidence_refs':a(REF,16),'reason':s(maxLength=1024)}))})
REVOKE=doc('AgreementRevoke','ledger-workflow-agreement-revoke/1',{'agreement_id':I,'accepted_binding_ref':REF,'expected_revision':C,'transition_ref':REF,'evidence_refs':a(REF,16,1),'reason':s(maxLength=1024)})
HEAD=doc('HeadTransition','ledger-workflow-head-transition/1',{'head_kind':e('agreement_acceptance','agreement_activation'),'scope':SCOPE,'customer':I,'agreement_id':I,'operation_key':KEY,'actor':I,'prior_head_ref':REF,'old_revision':C,'new_revision':C,'accepted_binding_ref':REF,'accepted_at':T,'commit_sequence':C,'resolved_effective_time':T},('prior_head_ref','resolved_effective_time'))
ops={'policy.publish':PUBLISH,'agreement.accept':ACCEPT,'agreement.activate':union(ACTIVATE,ACTREPL),'work.accept':WORK,'quantity.correct':QC,'outcome.submit':OUTCOME,'outcome.correct':OUTCOR,'agreement.revoke':REVOKE}
ENV=define('OperationEnvelope',union(*[o({'schema':c('ledger-operation/1'),'key':KEY,'kind':c(k),'contract':c('workflow/1'),'payload':v}) for k,v in ops.items()]))
ROLE=define('DescriptorRole',e('product','policy','fact_schema','binding','stage_instance','work','quantity_field','dependent_decision','publication','slot_catalog','slot_instantiation','authority','assent','accepted_binding','transition','target','claim','family','inline_document'))
LOC=define('DescriptorLocator',union(o({'role':ROLE,'ref':REF,'field':N},('field',)),o({'role':ROLE,'inline_document':DOCUMENT})))
ANCH=define('DescriptorAnchor',o({'role':ROLE,'record_ref':REF,'relation':e('direct','original_work','original_binding','publication_source','dependent_decision','accepted_binding','original_transition','original_target','original_claim'),'descriptor_refs':a(DR,128)}))
RES=doc('DescriptorResolution','ledger-operation-descriptors/1',{'registry_digest':H,'operation_kind':e(*ops),'payload_schema':I,'locator_digest':H,'anchors':a(ANCH,128),'descriptors':a(DR,128),'normalization_profile':c('ledger-operation-normalization/2'),'closure_digest':H})
REC=doc('OperationReceipt','ledger-operation-receipt/2',{'key':KEY,'request_hash':H,'request_hash_profile':c('bean-counter/operation-request/2'),'normalization_profile':c('ledger-operation-normalization/2'),'schema_bundle_digest':H,'descriptor_closure_digest':H,'original_request_bytes_sha256':RAW,'result_kind':e('economic','administrative','job_accepted','semantic_alias'),'result_refs':a(REF,4096),'accepted_at':T,'commit_sequence':C,'manifest_hash':H,'receipt_hash':H})

AT=define('PlanActivity',o({'activity':N,'primitive':e('work','outcome'),'fact_schema_ref':DR,'source_slot':N,'funding_leg':N},('funding_leg',)))
FSL=define('PlanFactSlot',o({'index':C,'schema_ref':DR,'field':FIELD}))
INS=define('TypedInstruction',union(o({'opcode':c('evaluate_rule'),'ordinal':C,'policy_ref':DR,'rule':RULE,'fact_slots':a(C,64),'binding_fact_slots':a(C,64),'basis_instructions':a(C,64)}),o({'opcode':c('evaluate_family'),'ordinal':C,'policy_ref':DR,'family':FAM,'fact_slots':a(C,64),'target_component_key':N}),o({'opcode':c('close_stage'),'ordinal':C,'policy_ref':DR,'stage':STAGE})))
WC=define('WorstCase',o({k:c(v) for k,v in {'fields':'1088','nodes':'256','hops':'16','loaded_refs':'4096','actions':'128','explanations':'256','bytes':'4194304','compile_work':'1048576','runtime_work':'65536'}.items()}))
PLAN=doc('CompiledPlan','ledger-workflow-compiled-plan/1',{'compiler_ref':DR,'compiler_semantics':c('ledger-policy-compiler/1'),'evaluator':c('ledger-workflow-evaluator/1'),'grammar':c('ledger-policy-grammar/1'),'limits_profile':c('workflow-limits/1'),'source_refs':a(DR,128,1),'activity_table':a(AT,16,1),'fact_slots':seq(FSL,1088),'link_paths':a(SELECT,256),'component_slots':a(TC,128,1),'rule_order':seq(o({'policy_ref':DR,'rule_id':N}),64),'typed_instructions':seq(INS,256),'dependency_edges':a(o({'producer':C,'consumer':C}),4096),'worst_case':WC,'explanation_map':seq(o({'ordinal':C,'policy_ref':DR,'rule_or_family':N,'fields':a(N,64),'primitive':e('fixed','unit','percent','discount','final_cost','supplier_final_cost','observe_cost','closure_cap','closure_share','family','stage')}),256)})
PUB=doc('Publication','ledger-workflow-publication/1',{'root_policy_ref':DR,'source_refs':a(DR,128,1),'compiled_plan_ref':REF,'compiler_ref':DR,'provenance':o({'source_sha':s(r'^[0-9a-f]{40}$',40),'binary_sha256':RAW,'build_manifest_digest':H})})
EVID=doc('RequestEvidence','ledger-operation-request-evidence/2',{'normalization_profile':c('ledger-operation-normalization/2'),'original_request_bytes_base64':s(maxLength=349528),'original_request_bytes_sha256':RAW,'normalized_request':ENV,'request_hash':H,'schema_bundle_digest':H,'descriptor_resolution':RES})
RESULT=define('OperationResult',union(o({'schema':c('ledger-operation-result/1'),'status':c('committed'),'receipt':REC,'request_evidence':EVID,'duplicate_kind':e('identity','semantic')},('duplicate_kind',)),o({'schema':c('ledger-operation-result/1'),'status':c('waiting_dependencies'),'missing_refs':a(REF,4096,1)}),o({'schema':c('ledger-operation-result/1'),'status':e('conflict','refused'),'code':r('ErrorCode'),'mutation':c('none')}),o({'schema':c('ledger-operation-result/1'),'status':e('retryable','outcome_unknown'),'key':KEY,'status_route':c('operation.status')})))
STATUS=define('OperationStatus',union(o({'schema':c('ledger-operation-status/1'),'status':c('committed'),'receipt':REC,'request_evidence':EVID}),o({'schema':c('ledger-operation-status/1'),'status':c('not_observed'),'key':KEY})))
ERR=define('ErrorCode',e('UNSUPPORTED_SCHEMA_PROFILE','INVALID_SHAPE','INVALID_SCALAR','INVALID_REFERENCE','DOCUMENT_DIGEST_MISMATCH','PROGRAM_BUDGET_EXCEEDED','PREDICATE_ANALYSIS_LIMIT','BASIS_NOT_TOTAL','UNSUPPORTED_COMPOSITION','SLOT_SEMANTICS_CHANGED','DESCRIPTOR_DEPENDENCY_MISSING','DESCRIPTOR_SCOPE_MISMATCH','REQUEST_EVIDENCE_MISMATCH','CORRECTION_CAPACITY_EXCEEDED','REVERSAL_DEPENDENTS_REQUIRED','STALE_REVISION','AUTHORITY_REQUIRED','IDENTITY_CONFLICT','SEMANTIC_CONFLICT','PLAN_DIGEST_MISMATCH','INTEGRITY_FAILURE'))

LABAS=doc('LocalAuthorityBasis','ledger-workflow-local-authority-basis/1',{'logical_store_id':I,'scope':SCOPE,'principal_id':I,'source':SRC,'canonical_economic_namespace':SRC,'rights':{'const':['admin','read']},'source_grant_revision':C})
LAS=doc('LocalAuthoritySnapshot','ledger-workflow-local-authority-snapshot/1',{'basis':LABAS,'basis_digest':H,'verification_profile':c('local-private-directory-owner/1'),'verified_at':T})
PA=doc('PublicationAuthority','ledger-workflow-publication-authority/1',{'principal_id':I,'source':SRC,'authority_refs':a(REF,16,1),'rights':{'const':['admin']},'validated_at':T})
OR=doc('OperationRecord','ledger-workflow-operation-record/1',{'key':KEY,'kind':c('policy.publish'),'contract':c('workflow/1'),'request_evidence':EVID,'authority_snapshot':PA,'accepted_at':T,'commit_sequence':C,'result_refs':a(REF,2,2)})
MAN=doc('PublicationManifest','ledger-workflow-publication-manifest/1',{'key':KEY,'members':a(REF,147,5)})
def record(kind,body):return o({'kind':c(kind),'logical_store_id':I,'scope':SCOPE,'id':I,'digest':H,'body':body})
BOUND=define('PublicationBoundaryEnvelope',union(record('workflow-authority',LAS),record('workflow-product',PROD),record('workflow-fact-schema',FS),record('workflow-policy',POL),record('workflow-slot-template',ST),record('workflow-compiler-descriptor',CD),record('workflow-compiled-plan',PLAN),record('workflow-publication',PUB),record('workflow-operation',OR),record('workflow-manifest',MAN),record('workflow-receipt',REC)))
READ=define('PublicationRead',union(o({'schema':c('ledger-read/1'),'kind':c('policy.document'),'payload':o({'ref':REF})}),o({'schema':c('ledger-read/1'),'kind':c('operation.descriptors'),'payload':o({'contract':c('workflow/1'),'operation_kind':c('policy.publish'),'payload_schema':c('ledger-workflow-policy-publication/1'),'locators':a(LOC,128,1)})})))
READRES=doc('PublicationReadResult','ledger-document-read/1',{'record':BOUND,'canonical_bytes_base64':s(maxLength=5592408),'canonical_digest':H})
READERR=doc('ReadError','ledger-read-error/1',{'code':e('AUTHORITY_REQUIRED','INVALID_REFERENCE','INTEGRITY_FAILURE','UNAVAILABLE','UNSUPPORTED_SCHEMA_PROFILE')})
# No response or retained compiler-plan placeholders: absent families are explicitly blockers.
root={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'urn:bean-counter:candidate:full-suite-v1:core:workflow:1','title':'UNFROZEN workflow candidate normalized shapes','description':'Partial Wave 1 transcription. Read normative-amendment.md and coverage.json. Not a runtime capability declaration.','$defs':D,'oneOf':[DOCUMENT,ENV,RES,REC,PLAN,PUB,EVID,RESULT,STATUS,HEAD,OR,MAN,BOUND,READ,READRES,READERR]}
Path(__file__).with_name('workflow.schema.json').write_text(json.dumps(root,indent=2,ensure_ascii=False)+'\n')

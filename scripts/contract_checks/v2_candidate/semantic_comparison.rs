
// Appended only to a disposable archive of the approved testkit by the Python
// comparison runner. Never linked into the candidate's production workspace.
fn candidate_goldens() -> Vec<Value> {
    let root=std::env::var("LEDGERLAB_CANDIDATE_ROOT").unwrap();
    let mut histories:Vec<Value>=fs::read_dir(Path::new(&root).join("contracts/candidates/v2/goldens")).unwrap()
        .map(|p| p.unwrap().path()).filter(|p| !["inventory.json","vectors.json"].contains(&p.file_name().unwrap().to_str().unwrap()))
        .map(|p| serde_json::from_slice(&fs::read(p).unwrap()).unwrap()).collect();
    if std::env::var("LEDGERLAB_CAPTURE_ORIGINALS").is_err(){
        let extra:Vec<Value>=serde_json::from_slice(&fs::read(Path::new(&root).join("work/validation/v2-rehashed-accepted-unicode.json")).unwrap()).unwrap();
        assert_eq!(extra.len(),1);histories.extend(extra);
    }
    histories
}
fn candidate_binding(v:&Value)->c::Binding {
    c::Binding {
        id:v["id"].as_str().unwrap().into(), agreement:v["agreement"].as_str().unwrap().into(),
        book:match v["book"].as_str().unwrap(){"retail"=>c::Book::Retail,"supplier"=>c::Book::Supplier,_=>panic!()},
        roles:serde_json::from_value(v["roles"].clone()).unwrap(),assent:v["assent"].as_str().unwrap().into(),
        offer:v["offer"].as_str().map(Into::into),sources:v["sources"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().into()).collect(),
        event_types:v["event_types"].as_array().unwrap().iter().map(|v|serde_json::from_value(v.clone()).unwrap()).collect(),
        unit:v["unit"].as_str().unwrap().into(), maximum_quantity:Decimal::parse(v["maximum_quantity"].as_str().unwrap()).unwrap(),
        maximum_exposure:v.get("maximum_exposure").map(|v|serde_json::from_value(v.clone()).unwrap()),
        outcome:v.get("outcome").map(|v|c::OutcomeTerms {source:v["source"].as_str().unwrap().into(),window_us:v["window_us"].as_u64().unwrap(),report_grace_us:v["report_grace_us"].as_u64().unwrap(),claim_namespace:v["claim_namespace"].as_str().unwrap().into()}),
        correction_sources:v["correction_sources"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().into()).collect(),
        allowed_modifiers:v["allowed_modifiers"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().into()).collect(),allocation_view:v["allocation_view"].as_bool().unwrap(),
    }
}
#[test]
fn candidate_lossless_event_binding_policy_and_economics() {
    let mut count=0;let mut decisions=0;
    for h in candidate_goldens().into_iter().filter(|h|h["target_admission"]=="accepted") {
        let seed=h["seed"].as_array().unwrap();
        let row=|kind:&str| seed.iter().find(|r|r["kind"]==kind).unwrap();
        let b=&row("base-evaluation")["body"];
        let material:Value=serde_json::from_str(b["evaluation_utf8"].as_str().unwrap()).unwrap();
        let wire=b["original_ingress_utf8"].as_str().unwrap();
        let event=normalize(wire.as_bytes(),Scope::new("synthetic","sandbox").unwrap(),"urn:synthetic:work").unwrap().resolve(None).unwrap();
        assert_eq!(event.bytes().as_slice(),b["original_event_utf8"].as_str().unwrap().as_bytes());
        assert_eq!(event.id(),b["original_event_id"].as_str().unwrap());
        assert_eq!(event.content_hash(),b["original_event_hash"].as_str().unwrap());
        assert_eq!(event.candidate().ingress_hash(),b["original_ingress_hash"].as_str().unwrap());
        let policies=material["bundle"]["policies"].as_array().unwrap().iter().map(|p|c::Policy {
            binding:candidate_binding(&p["binding"]),
            rules:p["rules"].as_array().unwrap().iter().map(|r|c::Rule {
                id:r["id"].as_str().unwrap().into(),on:serde_json::from_value(r["on"].clone()).unwrap(),component:r["component"].as_str().unwrap().into(),when:vec![],matcher:None,
                operation:match r["operation"]["kind"].as_str().unwrap() {
                    "base"=>c::Operation::Base(c::Price::Fixed(Decimal::parse(r["operation"]["price"]["value"].as_str().unwrap()).unwrap())),
                    "discount"=>c::Operation::Discount {amount:c::DiscountAmount::Fixed(Decimal::parse(r["operation"]["amount"]["value"].as_str().unwrap()).unwrap()),component:r["operation"]["component"].as_str().unwrap().into(),mode:c::DiscountMode::Additive},
                    _=>panic!(),
                },
            }).collect(),
        }).collect();
        let bundle=c::Bundle::compile("USD",2,policies).unwrap();
        for p in bundle.policies() {
            let retained=seed.iter().find(|r|r["kind"]=="binding-snapshot"&&r["body"]["binding_id"]==p.binding.id).unwrap();
            let binding:Value=serde_json::from_str(retained["body"]["binding_utf8"].as_str().unwrap()).unwrap();
            assert_eq!(candidate_binding(&binding),p.binding);
        }
        let context=c::Context {document:material["context"]["document"].as_str().unwrap().into(),customer:"customer".into(),funding:c::Funding::Byok,tier:None,priority:None,stage:None};
        let source=&material["source_authority"];
        let auth=c::SourceAuthority {source:source["source"].as_str().unwrap().into(),grant:source["grant"].as_str().unwrap().into(),revision:Revision::parse(source["revision"].as_str().unwrap()).unwrap(),active:true,event_types:vec![event.dto().kind],relations:vec![]};
        let invocations:Vec<_>=material["invocations"].as_array().unwrap().iter().map(|v|c::Invocation {
            id:v["id"].as_str().unwrap().into(),binding_id:v["binding_id"].as_str().unwrap().into(),operation_id:v["operation_id"].as_str().unwrap().into(),chain:v["chain"].as_str().unwrap().into(),customer:v["customer"].as_str().unwrap().into(),source:v["source"].as_str().unwrap().into(),unit:v["unit"].as_str().unwrap().into(),maximum_quantity:Decimal::parse(v["maximum_quantity"].as_str().unwrap()).unwrap(),maximum_exposure:serde_json::from_value(v["maximum_exposure"].clone()).unwrap(),held:serde_json::from_value(v["held"].clone()).unwrap(),authorized_at:Timestamp::parse(v["authorized_at"].as_str().unwrap()).unwrap(),start_before:Timestamp::parse(v["start_before"].as_str().unwrap()).unwrap(),attested_start:Timestamp::parse(v["attested_start"].as_str().unwrap()).unwrap(),outcome_deadline:v["outcome_deadline"].as_str().map(|v|Timestamp::parse(v).unwrap()),completion_event:None,
        }).collect();
        let received=Timestamp::parse(material["received_at"].as_str().unwrap()).unwrap();
        let base=bundle.evaluate(c::Input {event:&event,context:&context,history:&[],source_authority:&auth,invocations:&invocations,costs:&[],received_at:&received}).unwrap();
        let original_bytes=canonical::CanonicalBytes::from_value(&base).unwrap();
        let restored:c::Evaluation=serde_json::from_slice(original_bytes.as_slice()).unwrap();
        assert_eq!(canonical::CanonicalBytes::from_value(&restored).unwrap().as_slice(),original_bytes.as_slice());
        assert_eq!(restored.actions(),base.actions());
        assert_eq!(restored.explanations(),base.explanations());
        assert_eq!(restored.deltas(),base.deltas());
        assert_eq!(restored.consumptions(),base.consumptions());
        assert_eq!(restored.invocations(),base.invocations());
        assert_eq!(restored.bundle(),base.bundle());
        assert_eq!(restored.context(),base.context());
        assert_eq!(restored.claim_id(),base.claim_id());
        assert_eq!(restored.closed_stage(),base.closed_stage());
        if let Ok(dir)=std::env::var("LEDGERLAB_CAPTURE_ORIGINALS") {
            fs::write(Path::new(&dir).join(format!("{}.json",h["name"].as_str().unwrap())),original_bytes.as_slice()).unwrap();
        } else {
            assert_eq!(original_bytes.as_slice(),b["original_evaluation_utf8"].as_str().unwrap().as_bytes(),"complete original Evaluation: {}",h["name"]);
            let retained:c::Evaluation=serde_json::from_str(b["original_evaluation_utf8"].as_str().unwrap()).unwrap();
            assert_eq!(retained.actions(),base.actions());
            assert_eq!(canonical::CanonicalBytes::from_value(&retained).unwrap().as_slice(),original_bytes.as_slice());
        }
        let target_row=&row("target-snapshot")["body"];
        let source_policy:Value=serde_json::from_str(target_row["policy_utf8"].as_str().unwrap()).unwrap();
        let p=o::Policy {
            version:source_policy["version"].as_str().unwrap().into(),document:source_policy["document"].as_str().unwrap().into(),
            families:source_policy["families"].as_array().unwrap().iter().map(|f|o::Family {
                family:f["family"].as_str().unwrap().into(),binding_id:f["binding_id"].as_str().unwrap().into(),source:f["source"].as_str().unwrap().into(),correction_source:f["correction_source"].as_str().unwrap().into(),evidence_required:f["evidence_required"].as_bool().unwrap(),
                ordinary:candidate_window(&f["ordinary"]),corrections:candidate_window(&f["corrections"]),
                codes:f["codes"].as_array().unwrap().iter().map(|c|o::Code {code:c["code"].as_str().unwrap().into(),amount:if c["amount"]["kind"]=="fixed" {o::Amount::Fixed(serde_json::from_value(c["amount"]["money"].clone()).unwrap())} else {o::Amount::Percent(ExactRatio::from_canonical(c["amount"]["rate"]["numerator"].as_str().unwrap(),c["amount"]["rate"]["denominator"].as_str().unwrap()).unwrap())}}).collect(),
                replacement_codes:f["replacement_codes"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().into()).collect(),allow_reversal:f["allow_reversal"].as_bool().unwrap(),
            }).collect(),
            limits:source_policy["limits"].as_array().unwrap().iter().map(|l|o::Limit {binding_id:l["binding_id"].as_str().unwrap().into(),premium:serde_json::from_value(l["premium"].clone()).unwrap()}).collect(),
        };
        let document=|id:&Value|seed.iter().find(|r|r["id"]==*id).unwrap()["body"]["document_id"].as_str().unwrap().to_string();
        let tv=o::TargetVerification {rated_final:true,accepted_at:Timestamp::parse(target_row["accepted_at"].as_str().unwrap()).unwrap(),policy_document:target_row["verified_policy_document"].as_str().unwrap().into(),verified_assents:target_row["verified_assents"].as_array().unwrap().iter().map(document).collect(),verified_offers:target_row["verified_offers"].as_array().unwrap().iter().map(document).collect(),verified_delegations:target_row["verified_delegations"].as_array().unwrap().iter().map(document).collect()};
        let target=o::Target::freeze(&base,p.clone(),tv.clone()).unwrap();
        let mut bad=tv;bad.policy_document=doc("wrong-policy");assert_eq!(o::Target::freeze(&base,p,bad).unwrap_err().code,"TERMS_NOT_VERIFIED");
        let retail_basis=seed.iter().find(|r|r["kind"]=="target-basis"&&r["body"]["book"]=="retail").unwrap();
        assert_eq!(target.retail_basis().atoms().to_string(),retail_basis["body"]["amount"]["atoms"].as_str().unwrap());
        let mut history=vec![];let mut available=seed.clone();let mut first_request=None;
        for d in h["decisions"].as_array().unwrap() {
            let records=d["records"].as_array().unwrap();let one=|k:&str|&records.iter().find(|r|r["kind"]==k).unwrap()["body"];
            available.extend(records.iter().cloned());
            let data=&one("event")["data"];let v=one("authority-decision");
            let evidence=|id:&Value|available.iter().find(|r|r["id"]==*id).unwrap()["body"]["document_id"].as_str().unwrap().to_string();
            let request=o::Request {scope:event.scope().clone(),id:data["external_id"].as_str().unwrap().into(),target:event.id().into(),agreement:data["agreement_id"].as_str().unwrap().into(),family:data["family_id"].as_str().unwrap().into(),source:data["source"].as_str().unwrap().into(),occurred_at:Timestamp::parse(data["occurred_at"].as_str().unwrap()).unwrap(),evidence:data["evidence"].as_array().unwrap().iter().map(evidence).collect(),change:if data["type"]=="outcome"{o::Change::Claim{code:data["code"].as_str().unwrap().into()}}else{o::Change::Correct{expected_revision:data["expected_revision_number"].as_str().unwrap().parse().unwrap(),replacement:data["replacement"]["code"].as_str().map(Into::into)}}};
            let verified=o::Verified {scope:request.scope.clone(),target:request.target.clone(),agreement:request.agreement.clone(),family:request.family.clone(),source:request.source.clone(),principal:v["principal"].as_str().unwrap().into(),grant:evidence(&v["grant"]),grant_revision:Revision::parse(v["grant_revision"].as_str().unwrap()).unwrap(),active:v["active"].as_bool().unwrap(),may_read:v["may_read"].as_bool().unwrap(),may_submit:v["may_submit"].as_bool().unwrap(),may_correct:v["may_correct"].as_bool().unwrap(),verified_evidence:v["verified_evidence"].as_array().unwrap().iter().map(evidence).collect(),received_at:Timestamp::parse(v["received_at"].as_str().unwrap()).unwrap(),accepted_at:Timestamp::parse(v["accepted_at"].as_str().unwrap()).unwrap()};
            let mut duplicate=request.clone();duplicate.evidence.push(request.evidence[0].clone());
            assert_eq!(o::evaluate(&duplicate,&verified,std::slice::from_ref(&target),std::slice::from_ref(&base),&history).unwrap_err().code,"OUTCOME_EVIDENCE_REQUIRED");
            if first_request.is_none(){first_request=Some((request.clone(),verified.clone()));}
            let o::Submission::Accepted(result)=o::evaluate(&request,&verified,std::slice::from_ref(&target),std::slice::from_ref(&base),&history).unwrap() else {panic!()};
            assert_eq!(result.current().atoms().to_string(),one("claim-revision")["amount"]["atoms"].as_str().unwrap());
            assert_eq!(result.binding().id,one("claim-revision")["binding_id"].as_str().unwrap());
            assert_eq!(result.postings().len(),records.iter().filter(|r|r["kind"]=="action").count());
            assert_eq!(result.explanations().len(),records.iter().filter(|r|r["kind"]=="explanation").count());
            history.push(*result);decisions+=1;
        }
        if h["name"]=="decision-time-evidence" {
            let (mut retry,verified)=first_request.unwrap();retry.id="different-evidence-wrapper-retry".into();
            let proof=h["decisions"][2]["records"].as_array().unwrap().iter().find(|r|r["kind"]=="evidence").unwrap();
            retry.evidence=vec![proof["body"]["document_id"].as_str().unwrap().into()];
            assert!(matches!(o::evaluate(&retry,&verified,std::slice::from_ref(&target),std::slice::from_ref(&base),&history).unwrap(),o::Submission::Duplicate(0)));
        }
        count+=1;
    }
    println!("Candidate comparisons against approved Rust: {count} exact typed Evaluation roundtrips with unchanged original IDs, fields and vectors; {decisions} decisions and duplicate-document rejections; reused-document correction and original-receipt retry; complete bindings, policy verification and economics.");
}

#[test]
fn candidate_fully_rehashed_scalar_histories_reject_in_rust() {
    if std::env::var("LEDGERLAB_CAPTURE_ORIGINALS").is_ok(){return;}
    let root=std::env::var("LEDGERLAB_CANDIDATE_ROOT").unwrap();
    let path=Path::new(&root).join("work/validation/v2-rehashed-scalar-attacks.json");
    let histories:Value=serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    for h in histories.as_array().unwrap() {
        let mut rejected=false;
        for row in h["seed"].as_array().unwrap().iter().chain(h["decisions"].as_array().unwrap().iter().flat_map(|d|d["records"].as_array().unwrap().iter())) {
            let b=&row["body"];
            if row["kind"]=="policy-snapshot" {
                for r in b["rules"].as_array().unwrap() {
                    rejected|=if let Some(a)=r["fixed_atoms"].as_str(){ledgerlab_core::money::parse_atoms(a).is_err()}
                    else {ExactRatio::from_canonical(r["rate"]["numerator"].as_str().unwrap(),r["rate"]["denominator"].as_str().unwrap()).is_err()};
                }
            }
            if row["kind"]=="explanation" {rejected|=ExactRatio::from_canonical(b["unrounded_atoms"]["numerator"].as_str().unwrap(),b["unrounded_atoms"]["denominator"].as_str().unwrap()).is_err();}
            if row["kind"]=="base-evaluation" {rejected|=serde_json::from_str::<c::Evaluation>(b["original_evaluation_utf8"].as_str().unwrap()).is_err();}
        }
        assert!(rejected,"fully rehashed scalar history must reject");
    }
    println!("Approved Rust rejects all {} fully rehashed noncanonical scalar histories.",histories.as_array().unwrap().len());
}
fn candidate_window(v:&Value)->o::Window {
    o::Window {starts_at:Timestamp::parse(v["starts_at"].as_str().unwrap()).unwrap(),occurs_before:Timestamp::parse(v["occurs_before"].as_str().unwrap()).unwrap(),received_by:Timestamp::parse(v["received_by"].as_str().unwrap()).unwrap(),accepted_by:Timestamp::parse(v["accepted_by"].as_str().unwrap()).unwrap()}
}

#[test]
fn candidate_exact_deadlines_fresh_evidence_and_cross_cancellation() {
    let stories:Value=serde_json::from_slice(&fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/phase2-outcomes-v0/histories.json")).unwrap()).unwrap();
    let base=base(&stories["histories"][0]);let mut p=policy(&stories["histories"][0]["policy"]);
    let ts=|s:&str|Timestamp::parse(s).unwrap();
    p.families[0].ordinary=o::Window {starts_at:ts("2026-09-21T12:00:00.000000Z"),occurs_before:ts("2026-09-22T12:00:00.000000Z"),received_by:ts("2026-09-23T12:00:00.000000Z"),accepted_by:ts("2026-09-23T13:00:00.000000Z")};
    p.families[0].corrections=o::Window {starts_at:ts("2026-09-21T12:00:00.000000Z"),occurs_before:ts("2026-09-24T12:00:00.000000Z"),received_by:ts("2026-09-25T12:00:00.000000Z"),accepted_by:ts("2026-09-25T13:00:00.000000Z")};
    let target=o::Target::freeze(&base,p.clone(),o::TargetVerification {rated_final:true,accepted_at:time(12),policy_document:p.document,verified_assents:vec![doc("retail"),doc("supplier")],verified_offers:vec![doc("offer")],verified_delegations:vec![]}).unwrap();
    let cases=[
        (false,"2026-09-21T12:00:00.000000Z","2026-09-23T12:00:00.000000Z","2026-09-23T13:00:00.000000Z","accepted"),
        (false,"2026-09-21T12:30:00.000000Z","2026-09-23T12:00:00.000001Z","2026-09-23T13:00:00.000000Z","OUTCOME_DEADLINE"),
        (false,"2026-09-21T12:30:00.000000Z","2026-09-23T12:00:00.000000Z","2026-09-23T13:00:00.000001Z","OUTCOME_DEADLINE"),
        (false,"2026-09-22T12:00:00.000000Z","2026-09-23T12:00:00.000000Z","2026-09-23T13:00:00.000000Z","OUTCOME_WINDOW"),
        (false,"2026-09-21T11:59:59.999999Z","2026-09-23T12:00:00.000000Z","2026-09-23T13:00:00.000000Z","OUTCOME_WINDOW"),
        (false,"2026-09-21T12:30:00.000000Z","2026-09-21T12:30:00.000000Z","2026-09-21T12:30:00.000000Z","accepted"),
        (true,"2026-09-24T11:59:59.999999Z","2026-09-25T12:00:00.000000Z","2026-09-25T13:00:00.000000Z","accepted"),
        (true,"2026-09-24T11:59:59.999999Z","2026-09-25T12:00:00.000001Z","2026-09-25T13:00:00.000000Z","OUTCOME_DEADLINE"),
        (true,"2026-09-24T11:59:59.999999Z","2026-09-25T12:00:00.000000Z","2026-09-25T13:00:00.000001Z","OUTCOME_DEADLINE"),
        (true,"2026-09-24T12:00:00.000000Z","2026-09-25T12:00:00.000000Z","2026-09-25T13:00:00.000000Z","OUTCOME_WINDOW"),
        (true,"2026-09-21T12:15:00.000000Z","2026-09-21T13:00:00.000000Z","2026-09-21T13:00:00.000000Z","accepted"),
    ];
    for (correction,occurred,received,accepted,want) in cases {
        let mut history=vec![];
        if correction {
            let step=json!({"id":"first","evidence":["proof"]});let mut r=request(&step,&base);r.occurred_at=ts("2026-09-21T12:30:00.000000Z");let mut v=verified(&step,&r);v.received_at=ts("2026-09-21T13:00:00.000000Z");v.accepted_at=v.received_at.clone();
            let o::Submission::Accepted(d)=o::evaluate(&r,&v,std::slice::from_ref(&target),std::slice::from_ref(&base),&history).unwrap() else {panic!()};history.push(*d);
        }
        let step=if correction {json!({"id":"later","revision":1,"replacement":"no","evidence":["new-proof"]})} else {json!({"id":"later","evidence":["new-proof"]})};
        let mut r=request(&step,&base);r.occurred_at=ts(occurred);let mut v=verified(&step,&r);v.received_at=ts(received);v.accepted_at=ts(accepted);
        let actual=match o::evaluate(&r,&v,std::slice::from_ref(&target),std::slice::from_ref(&base),&history){Ok(o::Submission::Accepted(_))=>"accepted",Ok(_)=>"duplicate",Err(e)=>e.code};
        assert_eq!(actual,want,"{correction}/{occurred}/{received}/{accepted}");
    }
    let denominator="3351951982485649274893506249551461531869841455148098344430890360930441007518386744200468574541725856922507964546621512713438470702986642486608412251521025";
    let rate=ExactRatio::from_canonical("1",denominator).unwrap();
    assert_eq!(ExactRatio::integer(100).mul(&rate).unwrap().div(&ExactRatio::integer(100)).unwrap(),rate);
    println!("Approved exact comparisons: 11 deadline/ordering cases, fresh claim/correction evidence, reduced percentage cross-cancellation.");
}

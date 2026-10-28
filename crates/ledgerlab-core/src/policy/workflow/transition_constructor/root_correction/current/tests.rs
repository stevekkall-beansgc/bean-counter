//! Consumer DATA regression. This exercises retained explanations, not source
//! authority, custody, a store append, or an independently qualified journey.
use super::*;
use crate::{
    canonical::{self, audit_arena::AuditArena},
    policy::workflow::{
        document_digest,
        selected_program::{
            compile_selected_publication, SelectedCompilerSupport, SelectedSourceInput,
        },
        DefinitionRef,
    },
    validation_work::{OriginalAuditSession, ResourceLimits},
};
use serde_json::json;

fn reference(kind: &str, prefix: &str, digit: char) -> ExactRef {
    ExactRef {
        kind: kind.into(),
        id: format!("{prefix}{}", digit.to_string().repeat(64)),
        digest: format!("sha256:{}", digit.to_string().repeat(64)),
    }
}
#[test]
fn retained_quantity_predicate_flips_verify_immediate_inverse_and_replacement_steps() {
    for (profile, failed) in [(0, false), (1, false), (2, false), (2, true)] {
        retained_quantity_predicate_flips(profile, failed);
    }
}
fn retained_quantity_predicate_flips(profile: u8, failed: bool) {
    let operational = profile != 0;
    let compiled_arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let mut session = OriginalAuditSession::for_resource_profile_v2(&compiled_arena).unwrap();
    let support = match profile {
        0 => SelectedCompilerSupport::local_policy_semantics_execution_with_session(&mut session),
        1 => SelectedCompilerSupport::local_policy_context_execution_with_session(&mut session),
        _ => SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut session),
    }
    .unwrap();
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../../selected_program/fixtures/local-first-policy-input.json"
    ))
    .unwrap();
    let mut docs = fixture["documents"].as_array().unwrap().clone();
    docs[5]["compiler_ref"] = json!(support.reference());
    docs[5]["rules"][0]["formula"] = json!({"kind":"unit","quantity_field":"support:quantity","unit":"support:request","rate":"0.37"});
    docs[5]["rules"][0]["when"] =
        json!({"op":"ge","field":{"fact":"support:quantity"},"value":"2"});
    if operational {
        docs[5]["rules"][0]["when"] = json!({"op":"all","args":[docs[5]["rules"][0]["when"],{"op":"eq","field":{"context":"source"},"value":"original-worker"},{"op":"eq","field":{"context":"funding_mode"},"value":"customer_local"}]});
    }
    let digest = document_digest("ledger-workflow-policy/4", &docs[5]).unwrap();
    let root = DefinitionRef::new(
        "workflow-policy",
        &format!("wdoc_{}", &digest[7..]),
        &digest,
    )
    .unwrap();
    docs.push(support.canonical_value().clone());
    let p = compile_selected_publication(
        SelectedSourceInput {
            documents: docs
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: root,
        },
        &support,
        &mut session,
    )
    .unwrap();
    let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let mut consumer = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let m = consumer.meter_mut();
    let _fixture_owner = arena.reserve_external_payload(32_768, m).unwrap();
    let c = DraftContextData {
        key: OperationKey {
            logical_store_id: "store".into(),
            scope: ["tenant".into(), "project".into()],
            source: if operational {
                "current-corrector"
            } else {
                "worker"
            }
            .into(),
            operation_id: "later-reverse".into(),
        },
        actor: "worker".into(),
        canonical_economic_namespace: "worker".into(),
        installation_mode: "isolated_synthetic".into(),
        received_at: "2026-10-26T00:00:04.000000Z".into(),
        accepted_at: "2026-10-26T00:00:04.000000Z".into(),
        authorities: vec![],
        source_observation_inputs: vec![],
    };
    let work_ref = reference("workflow-work", "wwork_", 'a');
    let claim_ref = reference("workflow-work-claim", "wclaim_", 'b');
    let binding_ref = reference("workflow-binding", "wdoc_", 'c');
    let terms_ref = reference("workflow-slot-terms", "wdoc_", 'd');
    let role_ref = reference("workflow-roles", "wdoc_", 'e');
    let context = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{}});
    let schema_ref = docs[0].clone();
    let digest = document_digest("ledger-workflow-fact-schema/1", &schema_ref).unwrap();
    let schema_ref =
        json!({"kind":"workflow-fact-schema","id":format!("wdoc_{}",&digest[7..]),"digest":digest});
    let payload = json!({"status":if failed {"failed"} else {"succeeded"},"activity":"support:complete","policy_ref":p.root_policy_ref(),"binding_ref":binding_ref,"fact_schema_ref":schema_ref,"facts":{"support:quantity":"3"},"fact_proofs":[]});
    let work = RecordBody::owned(
        &arena,
        arena
            .clone_value(
                &json!({"payload":payload,"key":{"source":"original-worker"}}),
                m,
            )
            .unwrap(),
        m,
    )
    .unwrap();
    let terms = |m: &mut CoreMeter| {
        let value = |m: &mut CoreMeter, v: Value| {
            RecordBody::owned(&arena, arena.clone_value(&v, m).unwrap(), m).unwrap()
        };
        let terms = authority::Terms {
            binding: value(
                m,
                json!({"agreement_id":"agreement","accepted_context":context,"bindings":[{"slot":"support:retail"}],"funding_legs":[{"name":"support:aaa-platform","mode":"platform"},{"name":"support:customer","mode":"customer_local"}]}),
            ),
            accepted: value(m, json!({})),
            _activation: value(m, json!({})),
            catalog: value(
                m,
                json!({"components":[{"slot_id":"service","root_slot":"service","role_slot":"support:retail-role","book":"retail"}]}),
            ),
            instantiation: value(
                m,
                json!({"component_map":[{"template_key":"support:service","slot_id":"service"}]}),
            ),
            slot_terms: value(m, json!({"funding_leg":"support:customer"})),
            slot_terms_ref: terms_ref.clone(),
            role_ref: role_ref.clone(),
        };
        let empty = arena.clone_value(&json!({}), m).unwrap();
        let metadata = arena.reserve_external_payload(4096, m).unwrap();
        authority::ResolvedOperands::owning(terms, vec![], empty, metadata)
    };
    let operands = terms(m);
    let authority = body::document(
        m,
        "workflow-economic-authority",
        json!({"schema":"ledger-workflow-economic-authority/8","verified_fact_refs":[]}),
    )
    .unwrap();
    let decision_id = super::super::super::work_keys::decision(m, &claim_ref.id).unwrap();
    let mut rows = vec![authority];
    let mut decisions = Vec::new();
    let mut previous_action: Option<(ConstructedEnvelope, Value)> = None;
    for (revision, quantity, atoms) in [
        (1usize, "3", 1_110_000_000_000_000_000i128),
        (2, "1", 0),
        (3, "2", 740_000_000_000_000_000),
    ] {
        let facts = json!({"support:quantity":quantity});
        let prices = pricing::work_subject(
            &p,
            ("support:complete", &payload),
            (&facts, &context),
            Some((&operands.binding, "original-worker")),
            m,
        )
        .unwrap();
        assert_eq!(prices[0].atoms, if failed { 0 } else { atoms });
        assert_eq!(prices[0].failed, failed);
        let key = OperationKey {
            operation_id: format!("revision-{revision}"),
            source: if operational && revision == 1 {
                "original-worker".into()
            } else {
                c.key.source.clone()
            },
            ..c.key.clone()
        };
        let mut steps = Vec::new();
        let mut appended = Vec::new();
        if let Some((old, old_body)) = previous_action.take() {
            let inverse = cost_postings::exact_root_inverse_key(
                m,
                &key,
                &old.reference,
                &old_body,
                &prices[0],
                &revision.into(),
            )
            .unwrap();
            let action = inverse.action.unwrap();
            appended.push(action.reference.clone());
            steps.push(inverse.step.value().clone());
            rows.push(action);
            rows.push(old);
        }
        let posting = cost_postings::original_cost_posting_key(
            m,
            &key,
            cost_postings::OriginalCostPosting {
                binding: &operands.binding,
                original_work_claim: &claim_ref,
                component_slot: &json!("service"),
                root_slot: &json!("service"),
                book: &json!("retail"),
                terms_ref: &terms_ref,
                terms: &operands.slot_terms,
                role_ref: &role_ref,
                decision_id: &decision_id,
                revision: &revision.into(),
                replacement: revision > 1,
                basis_refs: Some(&json!([])),
            },
            &prices[0],
        )
        .unwrap();
        let mut active = Vec::new();
        if let Some(action) = posting.action {
            let value = action.decoded(m, 4194304).unwrap();
            active.push(action.reference.clone());
            appended.push(action.reference.clone());
            previous_action = Some((action, (*value).clone()));
        }
        steps.push(posting.step.value().clone());
        let evaluation = body::document(m,"workflow-evaluation",json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":work_ref,"policy_ref":p.root_policy_ref(),"compiled_plan_ref":p.compiled_plan().reference(),"compiler_ref":p.compiler_descriptor_ref(),"binding_ref":binding_ref,"fact_schema_ref":schema_ref,"facts":facts,"context":context,"steps":steps})).unwrap();
        let prior = decisions
            .last()
            .map(|row: &ConstructedEnvelope| row.reference.clone());
        let mut decision = json!({"schema":"ledger-workflow-decision-revision/1","decision_id":decision_id,"revision":revision.to_string(),"subject_ref":work_ref,"kind":"work","transition":if revision==1{"initial"}else{"quantity_correct"},"status":"active","original_terms_ref":binding_ref,"authority_ref":rows[0].reference,"evaluation_ref":evaluation.reference,"actions_appended":body::ref_set(m,appended).unwrap(),"active_action_refs":active,"dependency_refs":[],"operation_key":key,"accepted_at":format!("2026-10-26T00:00:0{revision}.000000Z")});
        if let Some(prior) = prior {
            decision["prior_ref"] = json!(prior);
        }
        let id = body::semantic_id(
            m,
            "wdrev_",
            "workflow-decision-revision",
            &json!([decision_id, revision.to_string()]),
        )
        .unwrap();
        decisions.push(body::record(m, &c.key, "workflow-decision", id, decision).unwrap());
        rows.push(evaluation);
    }
    if let Some((last, _)) = previous_action {
        rows.push(last);
    }
    let views: Vec<_> = rows
        .iter()
        .chain(decisions.iter())
        .map(|row| EconomicRecordView {
            reference: &row.reference,
            canonical_body: &row.canonical_body,
        })
        .collect();
    for (i, quantity) in [(1usize, "1"), (2, "2")] {
        let mut r = Records::borrowed_parent_app(m, &arena, &c.key, &views).unwrap();
        let current = decisions[i].decoded(m, 4194304).unwrap();
        let original = decisions[0].decoded(m, 4194304).unwrap();
        let facts = json!({"support:quantity":quantity});
        let prices = pricing::work_subject(
            &p,
            ("support:complete", &payload),
            (&facts, &context),
            Some((&operands.binding, "original-worker")),
            m,
        )
        .unwrap();
        let mut plan = RootPlan {
            work_ref: work_ref.clone(),
            work: work.clone(),
            claim_ref: claim_ref.clone(),
            original_ref: decisions[0].reference.clone(),
            original,
            current_ref: decisions[i].reference.clone(),
            current,
            next_revision: (i + 2).to_string(),
            payload: super::super::super::working_value::WorkingValue::copy(m, &payload).unwrap(),
            replacement_facts: super::super::super::working_value::WorkingValue::copy(m, &facts)
                .unwrap(),
            affected_roots: vec!["support:service".into()],
            old_prices: prices,
            new_prices: pricing::PricedInstructions::empty(),
            operands: terms(m),
            reverse: true,
            proofs: None,
            observations: None,
            _ownership: None,
        };
        let (_, evaluation) = load(
            m,
            &mut r,
            &c,
            &plan.current["evaluation_ref"],
            "workflow-evaluation",
            "ledger-workflow-evaluation-proof/1",
        )
        .unwrap();
        verify(&p, m, &mut r, &c, &plan, &evaluation).unwrap();
        let mut wrong = (*evaluation).clone();
        wrong["steps"].as_array_mut().unwrap().remove(0);
        assert_eq!(
            verify(&p, m, &mut r, &c, &plan, &wrong).unwrap_err().code,
            "INTEGRITY_FAILURE"
        );
        if i == 2 {
            // Rehash both linked DATA records so this reaches the actual prior
            // transition/shape consumer, rather than merely a digest refusal.
            let good_current = plan.current.clone();
            let good_reference = plan.current_ref.clone();
            for defect in ["transition", "shape"] {
                let prior = decisions[i - 1].decoded(m, 4_194_304).unwrap();
                let mut bad = (*prior).clone();
                if defect == "transition" {
                    bad["transition"] = json!("work_reverse");
                } else {
                    bad["unexpected"] = json!(true);
                }
                let bad_prior = body::record(
                    m,
                    &c.key,
                    "workflow-decision",
                    decisions[i - 1].reference.id.clone(),
                    bad,
                )
                .unwrap();
                let mut bad = (*good_current).clone();
                bad["prior_ref"] = json!(bad_prior.reference);
                let bad_current = body::record(
                    m,
                    &c.key,
                    "workflow-decision",
                    good_reference.id.clone(),
                    bad,
                )
                .unwrap();
                let mut bad_views: Vec<_> = views
                    .iter()
                    .copied()
                    .filter(|v| {
                        v.reference != &decisions[i - 1].reference && v.reference != &good_reference
                    })
                    .collect();
                for row in [&bad_prior, &bad_current] {
                    bad_views.push(EconomicRecordView {
                        reference: &row.reference,
                        canonical_body: &row.canonical_body,
                    });
                }
                let mut bad_records =
                    Records::borrowed_parent_app(m, &arena, &c.key, &bad_views).unwrap();
                plan.current = bad_current.decoded(m, 4_194_304).unwrap();
                plan.current_ref = bad_current.reference.clone();
                let error = verify(&p, m, &mut bad_records, &c, &plan, &evaluation).unwrap_err();
                assert_eq!(error.code, "INTEGRITY_FAILURE");
                if defect == "transition" {
                    assert_eq!(error.detail, "current root predecessor transition");
                }
                plan.current = good_current.clone();
                plan.current_ref = good_reference.clone();
            }
        }
    }
}

/// Actual selected Component profile and the shared posting constructors. This
/// is arithmetic/explanation DATA coverage, not source/host custody admission.
#[test]
fn component_posting_factory_replaces_only_generation_then_reverses_remaining_roots() {
    let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let mut author = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let support =
        SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut author)
            .unwrap();
    let input = pricing::component_tests::component_input(&support, &mut author);
    let mut compile = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let program = compile_selected_publication(input, &support, &mut compile).unwrap();
    let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let m = session.meter_mut();
    let _fixture = arena.reserve_external_payload(65_536, m).unwrap();
    let value = |m: &mut CoreMeter, value: Value| {
        RecordBody::owned(&arena, arena.clone_value(&value, m).unwrap(), m).unwrap()
    };
    let key = OperationKey {
        logical_store_id: "store".into(),
        scope: ["tenant".into(), "project".into()],
        source: "original-worker".into(),
        operation_id: "original-work".into(),
    };
    let work = reference("workflow-work", "wevent_", '1');
    let claim = reference("workflow-work-claim", "wclaim_", '2');
    let terms_ref = reference("workflow-slot-terms", "wdoc_", '4');
    let role = reference("workflow-roles", "wdoc_", '5');
    let context = json!({});
    let payload = json!({"activity":"support:complete","status":"succeeded"});
    let original = pricing::work_subject(
        &program,
        ("support:complete", &payload),
        (&json!({"support:quantity":"2"}), &context),
        None,
        m,
    )
    .unwrap();
    let replacement = pricing::work_subject(
        &program,
        ("support:complete", &payload),
        (&json!({"support:quantity":"1"}), &context),
        None,
        m,
    )
    .unwrap();
    let map: Vec<_> = program.compiled_plan().canonical_value()["component_slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| json!({"template_key":v["key"],"slot_id":v["key"]}))
        .collect();
    let terms = authority::Terms {
        binding: value(
            m,
            json!({"agreement_id":"agreement","accepted_context":context}),
        ),
        accepted: value(m, json!({})),
        _activation: value(m, json!({})),
        catalog: value(m, json!({})),
        instantiation: value(m, json!({"component_map":map})),
        slot_terms: value(m, json!({"funding_leg":"support:customer"})),
        slot_terms_ref: terms_ref.clone(),
        role_ref: role.clone(),
    };
    let snapshot = arena.clone_value(&json!({}), m).unwrap();
    let owner = arena.reserve_external_payload(4096, m).unwrap();
    let operands = authority::ResolvedOperands::owning(terms, vec![], snapshot, owner);
    let decision = json!({"decision_id":"generation-decision"});
    let mut originals: Vec<ConstructedEnvelope> = Vec::new();
    for price in &original {
        let mut basis = Vec::new();
        for name in &price.basis_components {
            for prior in &originals {
                let body = prior.decoded(m, 4_194_304).unwrap();
                if body["component_slot"] == *name {
                    basis.push(&prior.reference);
                }
            }
        }
        let basis = super::super::super::app_cost::reference_set(m, &arena, &basis).unwrap();
        let posting = cost_postings::original_cost_posting_key(
            m,
            &key,
            cost_postings::OriginalCostPosting {
                binding: &operands.binding,
                original_work_claim: &claim,
                component_slot: &json!(price.component_key),
                root_slot: &json!(price.root_key),
                book: &json!("retail"),
                terms_ref: &terms_ref,
                terms: &operands.slot_terms,
                role_ref: &role,
                decision_id: "generation-decision",
                revision: &1.into(),
                replacement: false,
                basis_refs: Some(basis.value()),
            },
            price,
        )
        .unwrap();
        if let Some(action) = posting.action {
            originals.push(action);
        }
    }
    let priority = originals
        .iter()
        .find(|a| a.decoded(m, 4_194_304).unwrap()["component_slot"] == "support:priority")
        .unwrap()
        .reference
        .clone();
    let mut old = ActiveRootActions {
        rows: Vec::new(),
        observation_indices: Vec::new(),
        _ownership: None,
    };
    for (index, price) in original.iter().enumerate() {
        let action = originals
            .iter()
            .find(|a| a.decoded(m, 4_194_304).unwrap()["component_slot"] == price.component_key)
            .unwrap();
        old.rows.push(ActiveRootAction {
            price_index: index,
            reference: action.reference.clone(),
            body: action.decoded(m, 4_194_304).unwrap(),
            affected: price.root_key == "support:service",
        });
    }
    let mut plan = RootPlan {
        work_ref: work,
        work: value(m, json!({})),
        claim_ref: claim,
        original_ref: reference("workflow-decision", "wdrev_", '6'),
        original: value(m, json!({})),
        current_ref: reference("workflow-decision", "wdrev_", '7'),
        current: value(m, decision),
        next_revision: "2".into(),
        payload: super::super::super::working_value::WorkingValue::copy(m, &payload).unwrap(),
        replacement_facts: super::super::super::working_value::WorkingValue::copy(
            m,
            &json!({"support:quantity":"1"}),
        )
        .unwrap(),
        affected_roots: vec!["support:service".into()],
        old_prices: original,
        new_prices: replacement,
        operands,
        reverse: false,
        proofs: None,
        observations: None,
        _ownership: None,
    };
    let correction_key = OperationKey {
        source: "corrector".into(),
        operation_id: "quantity-correct".into(),
        ..key.clone()
    };
    let effects = monetary_effects_owned_for(
        m,
        &plan,
        &old,
        PostingRevision {
            key: &correction_key,
            old_prices: &plan.old_prices,
            new_prices: &plan.new_prices,
            affected_roots: &plan.affected_roots,
            revision: "2",
            reverse: false,
        },
    )
    .unwrap();
    assert_eq!(effects.actions.len(), 4);
    assert!(effects.active.contains(&priority));
    assert!(!effects
        .actions
        .iter()
        .any(|a| a.decoded(m, 4_194_304).unwrap()["component_slot"] == "support:priority"));
    let scale = 10i128.pow(18);
    let delta: i128 = effects
        .actions
        .iter()
        .map(|a| {
            a.decoded(m, 4_194_304).unwrap()["amount"]["atoms"]
                .as_str()
                .unwrap()
                .parse::<i128>()
                .unwrap()
        })
        .sum();
    assert_eq!(delta, -4 * scale / 10);
    // F=.08 and G=-.08 both close under their own exact inverse, net zero.
    assert_eq!(
        130 * scale / 100 + delta - 8 * scale / 100 + 8 * scale / 100,
        90 * scale / 100
    );
    let mut current = ActiveRootActions {
        rows: Vec::new(),
        observation_indices: Vec::new(),
        _ownership: None,
    };
    for (index, price) in plan.new_prices.iter().enumerate() {
        let action = effects
            .actions
            .iter()
            .chain(originals.iter())
            .find(|a| {
                effects.active.contains(&a.reference)
                    && a.decoded(m, 4_194_304).unwrap()["component_slot"] == price.component_key
            })
            .unwrap();
        current.rows.push(ActiveRootAction {
            price_index: index,
            reference: action.reference.clone(),
            body: action.decoded(m, 4_194_304).unwrap(),
            affected: true,
        });
    }
    plan.affected_roots.push("support:priority".into());
    let empty = pricing::PricedInstructions::empty();
    let reverse_key = OperationKey {
        source: "corrector".into(),
        operation_id: "work-reverse".into(),
        ..key
    };
    let reversed = monetary_effects_owned_for(
        m,
        &plan,
        &current,
        PostingRevision {
            key: &reverse_key,
            old_prices: &plan.new_prices,
            new_prices: &empty,
            affected_roots: &plan.affected_roots,
            revision: "3",
            reverse: true,
        },
    )
    .unwrap();
    assert!(reversed.active.is_empty());
    let delta: i128 = reversed
        .actions
        .iter()
        .map(|a| {
            a.decoded(m, 4_194_304).unwrap()["amount"]["atoms"]
                .as_str()
                .unwrap()
                .parse::<i128>()
                .unwrap()
        })
        .sum();
    assert_eq!(delta, -6 * scale / 10);
    assert_eq!(90 * scale / 100 + delta, 30 * scale / 100);
    // The unrelated tool Work's .30 is never present in this root's action set.
    assert!(reversed
        .actions
        .iter()
        .all(|a| a.decoded(m, 4_194_304).unwrap()["book"] == "retail"));
}

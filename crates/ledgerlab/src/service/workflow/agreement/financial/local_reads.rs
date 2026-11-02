//! Current local-owner reads return committed immutable core results. Subject
//! source is never used as the identity of the authenticated reader.
use super::*;
use ledgerlab_core::validation_work::{OriginalAuditSession, RepresentationKind};

async fn certified_origin(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<prefix::LocatedWrappers> {
    let sequence = tx
        .workflow_selected_association_sequence(
            reference,
            &key.logical_store_id,
            &key.scope,
            before,
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let prior = sequence
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let origin = prefix::next(tx, key, prior, sequence, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    let encoded = wire::reference(reference, s)?;
    let encoded = s
        .parse_representation(encoded.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    origin.require_member(encoded.value(), s)?;
    let results = request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut present = false;
    for value in results {
        let actual = s
            .canonical_representation(value, RepresentationKind::Record)
            .map_err(core_error)?;
        let expected = wire::reference(reference, s)?;
        s.semantic_work()
            .scan_bytes(actual.bytes().len() + expected.bytes().len())
            .map_err(core_error)?;
        present |= actual.bytes() == expected.bytes();
    }
    require(present, "INTEGRITY_FAILURE")?;
    require(
        local_financial_original::verify(tx, &origin, None, s).await?,
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    Ok(origin)
}

pub(super) async fn current(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    kind: &str,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<Value> {
    request_wrapper::closed(payload, &["subject_ref"], s)?;
    ledger.admin.read(&ledger.store, tx, key).await?;
    let (subject, _subject_owner) =
        current::source::from_value_local(request_wrapper::field(payload, "subject_ref", s)?, s)?;
    require(
        matches!(
            (kind, subject.kind.as_str()),
            ("work.current", "workflow-work") | ("outcome.current", "workflow-outcome-claim")
        ),
        "INVALID_REFERENCE",
    )?;
    let before = tx
        .workflow_management_sequence(s)
        .await
        .map_err(prefix::read_error)?;
    let origin = certified_origin(tx, key, &subject, before, s).await?;
    let mut pool = local_inputs::LocalInputs::new(s)?;
    let evidence = request_wrapper::field(origin.wrappers().operation(), "request_evidence", s)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    pool.select_saved_registry(request_wrapper::text(resolution, "registry_digest", s)?, s)?;
    pool.add(&subject, s)?;
    let mut initial = None;
    for reference in request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if request_wrapper::text(reference, "kind", s)? == "workflow-decision" {
            let (r, _owner) = current::source::from_value_local(reference, s)?;
            pool.add(&r, s)?;
        }
    }
    pool.complete(tx, key, before, s).await?;
    let expected = wire::reference(&subject, s)?;
    for row in pool.iter_views() {
        if row.reference.kind != "workflow-decision" {
            continue;
        }
        let body = s
            .parse_representation(row.canonical_body, RepresentationKind::Record)
            .map_err(core_error)?;
        let selected_subject = request_wrapper::field(body.value(), "subject_ref", s)?;
        let selected = s
            .canonical_representation(selected_subject, RepresentationKind::Record)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(selected.bytes().len() + expected.bytes().len())
            .map_err(core_error)?;
        if selected.bytes() == expected.bytes() {
            require(initial.is_none(), "INTEGRITY_FAILURE")?;
            let value = wire::reference(row.reference, s)?;
            initial = Some((value, body));
        }
    }
    let (_initial_ref, initial) = initial.ok_or(ServiceError::IntegrityFailure)?;
    request_wrapper::same_text(
        request_wrapper::text(initial.value(), "revision", s)?,
        "1",
        s,
    )?;
    let decision_id = request_wrapper::text(initial.value(), "decision_id", s)?;
    let head = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            crate::store::sqlite::workflow::financial::HeadKey::Decision(decision_id),
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let decode = |bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| ServiceError::IntegrityFailure)
    };
    let strings_owner = s
        .reserve_external_payload(
            std::mem::size_of::<ledgerlab_core::policy::workflow::transition_constructor::ExactRef>(
            ) + head.kind.bytes().len()
                + head.id.bytes().len()
                + head.digest.bytes().len(),
        )
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(head.kind.bytes().len() + head.id.bytes().len() + head.digest.bytes().len())
        .map_err(core_error)?;
    let reference = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
        kind: decode(head.kind.bytes())?,
        id: decode(head.id.bytes())?,
        digest: decode(head.digest.bytes())?,
    };
    certified_origin(tx, key, &reference, before, s).await?;
    pool.add(&reference, s)?;
    pool.complete(tx, key, before, s).await?;
    let body = s
        .parse_representation(pool.body(&reference)?, RepresentationKind::Record)
        .map_err(core_error)?;
    request_wrapper::same_text(
        request_wrapper::text(body.value(), "decision_id", s)?,
        decision_id,
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(body.value(), "revision", s)?,
        std::str::from_utf8(head.revision.bytes()).map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )?;
    let selected_subject = request_wrapper::field(body.value(), "subject_ref", s)?;
    let actual = s
        .canonical_representation(selected_subject, RepresentationKind::Record)
        .map_err(core_error)?;
    require(actual.bytes() == expected.bytes(), "INTEGRITY_FAILURE")?;
    // Pay the bounded legacy Value response before cloning the parsed tree.
    // Forty-eight bytes per input byte conservatively cover node/map/string
    // control storage, including the smallest possible JSON scalar members.
    let output_bytes = pool
        .body(&reference)?
        .len()
        .checked_mul(48)
        .and_then(|v| v.checked_add(4096))
        .ok_or(ServiceError::Unavailable)?;
    let _response_owner = s
        .reserve_external_payload(output_bytes)
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(output_bytes)
        .map_err(core_error)?;
    let reply = serde_json::json!({"kind":reference.kind,"logical_store_id":key.logical_store_id,"scope":key.scope,"id":reference.id,"digest":reference.digest,"body":body.value()});
    let _output = s
        .canonical_representation(&reply, RepresentationKind::Record)
        .map_err(core_error)?;
    let _owner = strings_owner;
    Ok(reply)
}

// WIP: held read and verified cutoff for the bounded explanation collector.
// Refuse until the selected graph is complete; this is not a finished view.
pub(super) async fn explain(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<Value> {
    request_wrapper::closed(payload, &["subject_ref"], s)?;
    ledger.admin.read(&ledger.store, tx, key).await?;
    let (subject, _owner) =
        current::source::from_value_local(request_wrapper::field(payload, "subject_ref", s)?, s)?;
    require(subject.kind == "workflow-work", "INVALID_REFERENCE")?;
    let before = tx
        .workflow_management_sequence(s)
        .await
        .map_err(prefix::read_error)?;
    let through = before
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let subject_origin = certified_origin(tx, key, &subject, before, s).await?;
    let subject_sequence = subject_origin.wrappers().sequence();
    let mut after = 0i64;
    let mut last = None;
    while let Some(input) = prefix::next(tx, key, after, through, s).await? {
        let origin = input.checked_wrappers(tx, s).await?;
        after = origin.wrappers().sequence();
        last = Some(origin);
    }
    let selected = last.ok_or(ServiceError::IntegrityFailure)?;
    require(after == through, "INTEGRITY_FAILURE")?;
    type Path = ledgerlab_core::policy::workflow::transition_constructor::SelectedReportPath;
    let mut paths: Vec<(i64, Path)> = Vec::new();
    let mut paths_owner = None;
    let mut verified = {
        let mut capture =
            |sequence: i64, path: Path, session: &mut OriginalAuditSession| -> Result<()> {
                if paths.len() == paths.capacity() {
                    let next = paths
                        .capacity()
                        .max(1)
                        .checked_mul(2)
                        .ok_or(ServiceError::Unavailable)?;
                    let bytes = next
                        .checked_mul(std::mem::size_of::<(i64, Path)>())
                        .ok_or(ServiceError::Unavailable)?;
                    let lease = session
                        .reserve_external_payload(bytes)
                        .map_err(core_error)?;
                    session
                        .semantic_work()
                        .write_bytes(bytes)
                        .map_err(core_error)?;
                    let mut grown = Vec::new();
                    grown
                        .try_reserve_exact(next)
                        .map_err(|_| ServiceError::Unavailable)?;
                    require(grown.capacity() <= next, "INTEGRITY_FAILURE")?;
                    grown.append(&mut paths);
                    paths = grown;
                    paths_owner = Some(lease);
                }
                paths.push((sequence, path));
                Ok(())
            };
        prefix::fold_original_prefix_capture(ledger, tx, key, &selected, s, Some(&mut capture))
            .await?
    };
    // Second pass retains only authenticated introductions of graph node kinds.
    // Association or a later alias is not an introduction certificate.
    let mut after = 0i64;
    type Node = local_explain_family::Node;
    let mut nodes: Vec<Node> = Vec::new();
    let mut storage_owner = None;
    while let Some(input) = prefix::next(tx, key, after, through, s).await? {
        let origin = input.checked_wrappers(tx, s).await?;
        after = origin.wrappers().sequence();
        let results = request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        let members = request_wrapper::field(origin.wrappers().manifest(), "members", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        for value in results.iter().chain(members.iter()) {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            let kind = request_wrapper::text(value, "kind", s)?;
            if !matches!(
                kind,
                "workflow-work"
                    | "workflow-target"
                    | "workflow-family-state"
                    | "workflow-outcome-claim"
                    | "workflow-work-claim"
                    | "workflow-link"
                    | "workflow-chain-state"
                    | "workflow-dependency"
                    | "workflow-decision"
                    | "workflow-action"
                    | "workflow-evaluation"
                    | "workflow-evidence"
                    | "workflow-fact-attestation"
                    | "workflow-evidence-verification"
                    | "workflow-verified-fact"
                    | "workflow-fact-schema"
                    | "workflow-slot-terms"
                    | "workflow-binding"
                    | "workflow-compiler-descriptor"
                    | "workflow-compiled-plan"
                    | "workflow-product"
                    | "workflow-policy"
                    | "workflow-publication"
                    | "workflow-economic-authority"
                    | "workflow-roles"
                    | "workflow-slot-template"
                    | "workflow-slot-catalog"
                    | "workflow-slot-instantiation"
                    | "workflow-source-enrollment"
                    | "workflow-source-grant"
                    | "workflow-source-state"
                    | "workflow-authority"
                    | "workflow-activation"
                    | "workflow-assent"
                    | "workflow-accepted-agreement"
                    | "workflow-head-transition"
                    | "workflow-build-manifest"
            ) {
                continue;
            }
            let (reference, owner) = current::source::from_value_local(value, s)?;
            let stored = tx
                .workflow_financial_document(
                    &reference,
                    &key.logical_store_id,
                    &key.scope,
                    before,
                    s,
                )
                .await
                .map_err(prefix::read_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let introduced = verified.introduced_at(value, stored.body.bytes(), s)?;
            if introduced == after {
                let mut duplicate = false;
                for existing in &nodes {
                    s.semantic_work()
                        .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                        .map_err(core_error)?;
                    if local_original_work::same_ref(&reference, &existing.1, s)? {
                        duplicate = true;
                        break;
                    }
                }
                if duplicate {
                    continue;
                }
                let parsed = s
                    .parse_representation(stored.body.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                let canonical = s
                    .canonical_representation(parsed.value(), RepresentationKind::Record)
                    .map_err(core_error)?;
                s.semantic_work()
                    .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                    .map_err(core_error)?;
                let scanned = canonical
                    .bytes()
                    .len()
                    .checked_add(stored.body.bytes().len())
                    .ok_or(ServiceError::Unavailable)?;
                s.semantic_work().scan_bytes(scanned).map_err(core_error)?;
                require(
                    canonical.bytes() == stored.body.bytes(),
                    "INTEGRITY_FAILURE",
                )?;
                if nodes.len() == nodes.capacity() {
                    let next = nodes
                        .capacity()
                        .max(1)
                        .checked_mul(2)
                        .ok_or(ServiceError::Unavailable)?;
                    let bytes = next
                        .checked_mul(std::mem::size_of::<Node>())
                        .ok_or(ServiceError::Unavailable)?;
                    let lease = s.reserve_external_payload(bytes).map_err(core_error)?;
                    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
                    let mut grown = Vec::new();
                    grown
                        .try_reserve_exact(next)
                        .map_err(|_| ServiceError::Unavailable)?;
                    require(grown.capacity() <= next, "INTEGRITY_FAILURE")?;
                    grown.append(&mut nodes);
                    nodes = grown;
                    storage_owner = Some(lease);
                }
                nodes.push((after, reference, stored.body, owner));
            }
        }
    }
    require(after == through, "INTEGRITY_FAILURE")?;
    let mut found_subject = false;
    for (sequence, reference, _, _) in &nodes {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if local_original_work::same_ref(reference, &subject, s)? {
            require(
                !found_subject && *sequence == subject_sequence,
                "INTEGRITY_FAILURE",
            )?;
            found_subject = true;
        }
    }
    require(found_subject, "AUTHORITY_REQUIRED")?;
    // Target discovery follows each produced target's exact work_ref.  A
    // target without that edge cannot be attached by matching family names.
    let mut matching_targets = 0usize;
    for (_, reference, bytes, _) in &nodes {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if reference.kind != "workflow-target" {
            continue;
        }
        let body = s
            .parse_representation(bytes.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(body.value(), "schema", s)?,
            "ledger-workflow-component-target/1",
            s,
        )?;
        let (work, _work_owner) = current::source::from_value_local(
            request_wrapper::field(body.value(), "work_ref", s)?,
            s,
        )?;
        if local_original_work::same_ref(&work, &subject, s)? {
            s.semantic_work()
                .arithmetic(
                    ledgerlab_core::validation_work::ArithmeticOperation::Add,
                    &[&matching_targets.into(), &1usize.into()],
                )
                .map_err(core_error)?;
            matching_targets = matching_targets
                .checked_add(1)
                .ok_or(ServiceError::Unavailable)?;
        }
    }
    require(matching_targets > 0, "UNSUPPORTED_SCHEMA_PROFILE")?;
    // Follow every original root decision revision by its stable subject and
    // exact predecessor, then compare the last revision to the held head.
    let mut prior: Option<&ledgerlab_core::policy::workflow::transition_constructor::ExactRef> =
        None;
    let mut first = None;
    let mut decision_id: Option<String> = None;
    let mut decision_id_owner = None;
    let mut revision = 0u64;
    for (sequence, reference, bytes, _) in &nodes {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if reference.kind != "workflow-decision" {
            continue;
        }
        let body = s
            .parse_representation(bytes.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let (body_subject, _body_subject_owner) = current::source::from_value_local(
            request_wrapper::field(body.value(), "subject_ref", s)?,
            s,
        )?;
        if !local_original_work::same_ref(&body_subject, &subject, s)? {
            continue;
        }
        request_wrapper::same_text(
            request_wrapper::text(body.value(), "schema", s)?,
            "ledger-workflow-decision-revision/1",
            s,
        )?;
        let id = request_wrapper::text(body.value(), "decision_id", s)?;
        if let Some(expected) = &decision_id {
            request_wrapper::same_text(id, expected, s)?;
        } else {
            let lease = s.reserve_external_payload(id.len()).map_err(core_error)?;
            s.semantic_work()
                .write_bytes(id.len())
                .map_err(core_error)?;
            decision_id = Some(id.to_owned());
            decision_id_owner = Some(lease);
        }
        s.semantic_work()
            .arithmetic(
                ledgerlab_core::validation_work::ArithmeticOperation::Add,
                &[&revision.into(), &1u64.into()],
            )
            .map_err(core_error)?;
        revision = revision.checked_add(1).ok_or(ServiceError::Unavailable)?;
        let number = wire::counter_string(revision, s)?;
        let parsed_number = s
            .parse_representation(number.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(body.value(), "revision", s)?,
            parsed_number
                .value()
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?,
            s,
        )?;
        if let Some(expected) = prior {
            let (predecessor, _predecessor_owner) = current::source::from_value_local(
                request_wrapper::field(body.value(), "prior_ref", s)?,
                s,
            )?;
            require(
                local_original_work::same_ref(&predecessor, expected, s)?,
                "INTEGRITY_FAILURE",
            )?;
        } else {
            require(body.value().get("prior_ref").is_none(), "INTEGRITY_FAILURE")?;
            first = Some(reference);
        }
        // The decision's literal monetary and evidence operands must all
        // resolve to exact, authenticated introductions in this snapshot.
        for field in ["original_terms_ref", "authority_ref", "evaluation_ref"] {
            if let Some(value) = body.value().get(field) {
                let index = explain_owned_node(&nodes, value, s)?;
                if field == "evaluation_ref" {
                    explain_evaluation(&nodes, index, s)?;
                }
            }
        }
        for field in ["actions_appended", "active_action_refs", "dependency_refs"] {
            let refs = request_wrapper::field(body.value(), field, s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            for value in refs {
                let index = explain_owned_node(&nodes, value, s)?;
                if field == "actions_appended" {
                    require(nodes[index].0 == *sequence, "INTEGRITY_FAILURE")?;
                    explain_action(
                        tx,
                        key,
                        &nodes,
                        index,
                        request_wrapper::text(body.value(), "decision_id", s)?,
                        s,
                    )
                    .await?;
                }
            }
        }
        prior = Some(reference);
    }
    let id = decision_id
        .as_deref()
        .ok_or(ServiceError::IntegrityFailure)?;
    let expected = prior.ok_or(ServiceError::IntegrityFailure)?;
    let head = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            crate::store::sqlite::workflow::financial::HeadKey::Decision(id),
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let expected_revision = wire::counter_string(revision, s)?;
    let parsed_revision = s
        .parse_representation(expected_revision.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    request_wrapper::same_text(
        std::str::from_utf8(head.revision.bytes()).map_err(|_| ServiceError::IntegrityFailure)?,
        parsed_revision
            .value()
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?,
        s,
    )?;
    for (_, reference, bytes, _) in &nodes {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if reference.kind != "workflow-target" {
            continue;
        }
        let body = s
            .parse_representation(bytes.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let (work, _work_owner) = current::source::from_value_local(
            request_wrapper::field(body.value(), "work_ref", s)?,
            s,
        )?;
        if !local_original_work::same_ref(&work, &subject, s)? {
            continue;
        }
        let (original, _original_owner) = current::source::from_value_local(
            request_wrapper::field(body.value(), "original_decision_ref", s)?,
            s,
        )?;
        require(
            local_original_work::same_ref(
                &original,
                first.ok_or(ServiceError::IntegrityFailure)?,
                s,
            )?,
            "INTEGRITY_FAILURE",
        )?;
    }
    for (actual, wanted) in [
        (head.kind.bytes(), expected.kind.as_bytes()),
        (head.id.bytes(), expected.id.as_bytes()),
        (head.digest.bytes(), expected.digest.as_bytes()),
    ] {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
            .map_err(core_error)?;
        let bytes = actual
            .len()
            .checked_add(wanted.len())
            .ok_or(ServiceError::Unavailable)?;
        s.semantic_work().scan_bytes(bytes).map_err(core_error)?;
        require(actual == wanted, "INTEGRITY_FAILURE")?;
    }
    let families = local_explain_family::select(&nodes, &subject, s)?;
    for &(target_index, family_index) in &families.family_heads {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let target = &nodes[target_index].1;
        let family = &nodes[family_index];
        let body = s
            .parse_representation(family.2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let slot = request_wrapper::text(body.value(), "family_slot", s)?;
        let held = tx
            .workflow_financial_head(
                &key.logical_store_id,
                &key.scope,
                crate::store::sqlite::workflow::financial::HeadKey::Family {
                    target_kind: &target.kind,
                    target_id: &target.id,
                    target_digest: &target.digest,
                    slot,
                },
                s,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        request_wrapper::same_text(
            std::str::from_utf8(held.revision.bytes())
                .map_err(|_| ServiceError::IntegrityFailure)?,
            request_wrapper::text(body.value(), "revision", s)?,
            s,
        )?;
        for (actual, wanted) in [
            (held.kind.bytes(), family.1.kind.as_bytes()),
            (held.id.bytes(), family.1.id.as_bytes()),
            (held.digest.bytes(), family.1.digest.as_bytes()),
        ] {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(
                    actual
                        .len()
                        .checked_add(wanted.len())
                        .ok_or(ServiceError::Unavailable)?,
                )
                .map_err(core_error)?;
            require(actual == wanted, "INTEGRITY_FAILURE")?;
        }
    }
    let dependent = local_explain_decisions::select(&nodes, &families, s)?;
    for &(_, decision_index) in &dependent.heads {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let node = &nodes[decision_index];
        let body = s
            .parse_representation(node.2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let id = request_wrapper::text(body.value(), "decision_id", s)?;
        let held = tx
            .workflow_financial_head(
                &key.logical_store_id,
                &key.scope,
                crate::store::sqlite::workflow::financial::HeadKey::Decision(id),
                s,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        request_wrapper::same_text(
            std::str::from_utf8(held.revision.bytes())
                .map_err(|_| ServiceError::IntegrityFailure)?,
            request_wrapper::text(body.value(), "revision", s)?,
            s,
        )?;
        for (actual, wanted) in [
            (held.kind.bytes(), node.1.kind.as_bytes()),
            (held.id.bytes(), node.1.id.as_bytes()),
            (held.digest.bytes(), node.1.digest.as_bytes()),
        ] {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(
                    actual
                        .len()
                        .checked_add(wanted.len())
                        .ok_or(ServiceError::Unavailable)?,
                )
                .map_err(core_error)?;
            require(actual == wanted, "INTEGRITY_FAILURE")?;
        }
    }
    for &index in &dependent.history {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let body = s
            .parse_representation(nodes[index].2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        for field in ["original_terms_ref", "authority_ref", "evaluation_ref"] {
            if let Some(value) = body.value().get(field) {
                let index = explain_owned_node(&nodes, value, s)?;
                if field == "evaluation_ref" {
                    explain_evaluation(&nodes, index, s)?;
                }
            }
        }
        for field in ["actions_appended", "active_action_refs", "dependency_refs"] {
            let refs = request_wrapper::field(body.value(), field, s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            for value in refs {
                let action_index = explain_owned_node(&nodes, value, s)?;
                if field == "actions_appended" {
                    require(nodes[action_index].0 == nodes[index].0, "INTEGRITY_FAILURE")?;
                    explain_action(
                        tx,
                        key,
                        &nodes,
                        action_index,
                        request_wrapper::text(body.value(), "decision_id", s)?,
                        s,
                    )
                    .await?;
                }
            }
        }
    }
    for &index in &families.family_history {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let body = s
            .parse_representation(nodes[index].2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        for field in [
            "target_ref",
            "anchor_work_ref",
            "prior_ref",
            "claim_ref",
            "decision_ref",
        ] {
            if let Some(value) = body.value().get(field) {
                explain_owned_node(&nodes, value, s)?;
            }
        }
        if let Some(refs) = body.value().get("closure_evidence_refs") {
            for value in refs.as_array().ok_or(ServiceError::IntegrityFailure)? {
                explain_owned_node(&nodes, value, s)?;
            }
        }
        if let Some(causal) = body.value().get("causal_closure") {
            for field in [
                "root_decision_ref",
                "authority_ref",
                "original_slot_terms_ref",
            ] {
                if let Some(value) = causal.get(field) {
                    explain_owned_node(&nodes, value, s)?;
                }
            }
        }
    }
    for &index in &dependent.claims {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let body = s
            .parse_representation(nodes[index].2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        explain_owned_node(
            &nodes,
            request_wrapper::field(body.value(), "target_ref", s)?,
            s,
        )?;
    }
    // Only the exact path captured from each selected claim's original
    // constructor is explanatory evidence. Association-by-child is not proof.
    let n = nodes.len();
    let _link_controls = s.reserve_external_payload(n).map_err(core_error)?;
    s.semantic_work().write_bytes(n).map_err(core_error)?;
    let mut links = Vec::new();
    links
        .try_reserve_exact(n)
        .map_err(|_| ServiceError::Unavailable)?;
    require(links.capacity() <= n, "INTEGRITY_FAILURE")?;
    links.resize(n, 0u8);
    for &claim_index in &dependent.claims {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let claim = &nodes[claim_index];
        let mut matched = false;
        for (sequence, path) in &paths {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            if !local_original_work::same_ref(&path.claim, &claim.1, s)? {
                continue;
            }
            require(!matched && *sequence == claim.0, "INTEGRITY_FAILURE")?;
            matched = true;
            for r in &path.links {
                s.semantic_work()
                    .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                    .map_err(core_error)?;
                let mut link_index = None;
                for (index, node) in nodes.iter().enumerate() {
                    s.semantic_work()
                        .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                        .map_err(core_error)?;
                    if local_original_work::same_ref(r, &node.1, s)? {
                        require(link_index.is_none(), "INTEGRITY_FAILURE")?;
                        link_index = Some(index);
                    }
                }
                let index = link_index.ok_or(ServiceError::IntegrityFailure)?;
                require(nodes[index].1.kind == "workflow-link", "INTEGRITY_FAILURE")?;
                let body = s
                    .parse_representation(nodes[index].2.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                request_wrapper::same_text(
                    request_wrapper::text(body.value(), "schema", s)?,
                    "ledger-workflow-link/1",
                    s,
                )?;
                for field in [
                    "child",
                    "predecessor",
                    "authority_ref",
                    "binding_ref",
                    "product_ref",
                ] {
                    explain_owned_node(&nodes, request_wrapper::field(body.value(), field, s)?, s)?;
                }
                links[index] = 1;
            }
        }
        require(matched, "INTEGRITY_FAILURE")?;
    }
    // This view is deliberately limited to the selected Work, its Component
    // family histories, their decisions/evaluations/actions, and the exact
    // originally selected links. Other proof inputs remain referenced DATA.
    let count = nodes.len();
    let marks_owner = s.reserve_external_payload(count).map_err(core_error)?;
    s.semantic_work().write_bytes(count).map_err(core_error)?;
    let mut marks = Vec::new();
    marks
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    require(marks.capacity() <= count, "INTEGRITY_FAILURE")?;
    marks.resize(count, 0u8);
    for (i, node) in nodes.iter().enumerate() {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if local_original_work::same_ref(&node.1, &subject, s)? {
            marks[i] = 1;
        }
        if links[i] != 0 {
            marks[i] = 1;
        }
    }
    for &i in families
        .targets
        .iter()
        .chain(families.family_history.iter())
        .chain(dependent.claims.iter())
        .chain(dependent.history.iter())
    {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        marks[i] = 1;
    }
    for (i, node) in nodes.iter().enumerate() {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if node.1.kind != "workflow-decision" {
            continue;
        }
        let body = s
            .parse_representation(node.2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let (body_subject, _owner) = current::source::from_value_local(
            request_wrapper::field(body.value(), "subject_ref", s)?,
            s,
        )?;
        if local_original_work::same_ref(&body_subject, &subject, s)? {
            marks[i] = 1;
        }
    }
    for i in 0..count {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if marks[i] == 0 {
            continue;
        }
        let node = &nodes[i];
        let body = s
            .parse_representation(node.2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        if node.1.kind == "workflow-decision" {
            if let Some(value) = body.value().get("evaluation_ref") {
                marks[explain_owned_node(&nodes, value, s)?] = 1;
            }
            for field in ["actions_appended", "active_action_refs"] {
                for value in request_wrapper::field(body.value(), field, s)?
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    s.semantic_work()
                        .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                        .map_err(core_error)?;
                    marks[explain_owned_node(&nodes, value, s)?] = 1;
                }
            }
        } else if node.1.kind == "workflow-link" {
            for field in ["child", "predecessor"] {
                s.semantic_work()
                    .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                    .map_err(core_error)?;
                marks[explain_owned_node(
                    &nodes,
                    request_wrapper::field(body.value(), field, s)?,
                    s,
                )?] = 1;
            }
        }
    }
    // Follow exact inverse and Action basis links; these cannot invent money.
    let mut converged = false;
    for _ in 0..=count {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let mut changed = false;
        for i in 0..count {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            if marks[i] == 0 || nodes[i].1.kind != "workflow-action" {
                continue;
            }
            let body = s
                .parse_representation(nodes[i].2.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            if let Some(value) = body.value().get("inverse_of") {
                let index = explain_owned_node(&nodes, value, s)?;
                if marks[index] == 0 {
                    marks[index] = 1;
                    changed = true;
                }
            }
            if let Some(refs) = body.value().get("basis_refs") {
                for value in refs.as_array().ok_or(ServiceError::IntegrityFailure)? {
                    s.semantic_work()
                        .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                        .map_err(core_error)?;
                    let index = explain_owned_node(&nodes, value, s)?;
                    if nodes[index].1.kind == "workflow-action" && marks[index] == 0 {
                        marks[index] = 1;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            converged = true;
            break;
        }
    }
    require(converged, "INTEGRITY_FAILURE")?;
    let response = explain_render(
        tx,
        key,
        ExplainOutput {
            subject: &subject,
            root_head: expected,
            through,
            nodes: &nodes,
            marks: &marks,
            paths: &paths,
            claim_indices: &dependent.claims,
        },
        s,
    )
    .await?;
    let _owners = (
        storage_owner,
        decision_id_owner,
        families,
        dependent,
        links,
        _link_controls,
        paths,
        paths_owner,
        marks_owner,
    );
    Ok(response)
}

// Resolve only an exact full Ref to an authenticated introduction collected
// from the held prefix. This remains DATA until complete graph proof.
fn explain_owned_node(
    nodes: &[local_explain_family::Node],
    value: &Value,
    s: &mut OriginalAuditSession,
) -> Result<usize> {
    let (wanted, _owner) = current::source::from_value_local(value, s)?;
    let mut found = None;
    for (i, node) in nodes.iter().enumerate() {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if local_original_work::same_ref(&wanted, &node.1, s)? {
            require(found.is_none(), "INTEGRITY_FAILURE")?;
            found = Some(i);
        }
    }
    found.ok_or(ServiceError::IntegrityFailure)
}

fn explain_evaluation(
    nodes: &[local_explain_family::Node],
    index: usize,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let node = nodes.get(index).ok_or(ServiceError::IntegrityFailure)?;
    require(node.1.kind == "workflow-evaluation", "INTEGRITY_FAILURE")?;
    let body = s
        .parse_representation(node.2.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let schema = request_wrapper::text(body.value(), "schema", s)?;
    require(
        matches!(
            schema,
            "ledger-workflow-component-evaluation-proof/1" | "ledger-workflow-evaluation-proof/1"
        ),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    for field in [
        "binding_ref",
        "compiled_plan_ref",
        "compiler_ref",
        "fact_schema_ref",
        "policy_ref",
        "subject_ref",
    ] {
        if let Some(reference) = body.value().get(field) {
            explain_owned_node(nodes, reference, s)?;
        }
    }
    if let Some(context) = body.value().get("context") {
        if let Some(reference) = context.get("fact_schema_ref") {
            explain_owned_node(nodes, reference, s)?;
        }
    }
    let steps = request_wrapper::field(body.value(), "steps", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    for step in steps {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if let Some(reference) = step.get("action_ref") {
            explain_owned_node(nodes, reference, s)?;
        }
        if let Some(refs) = step.get("basis_refs") {
            for reference in refs.as_array().ok_or(ServiceError::IntegrityFailure)? {
                explain_owned_node(nodes, reference, s)?;
            }
        }
    }
    Ok(())
}

async fn explain_action(
    tx: &mut SqliteTx,
    key: &OperationKey,
    nodes: &[local_explain_family::Node],
    index: usize,
    decision_id: &str,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let node = nodes.get(index).ok_or(ServiceError::IntegrityFailure)?;
    require(node.1.kind == "workflow-action", "INTEGRITY_FAILURE")?;
    let previous = node
        .0
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let producer = prefix::next(tx, key, previous, node.0, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    request_wrapper::same_text(
        request_wrapper::text(producer.wrappers().operation(), "schema", s)?,
        "ledger-workflow-economic-operation/10",
        s,
    )?;
    require(
        request_wrapper::text(producer.wrappers().receipt(), "result_kind", s)? != "semantic_alias",
        "INTEGRITY_FAILURE",
    )?;
    let results = request_wrapper::field(producer.wrappers().receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut produced = false;
    for result in results {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let (r, _owner) = current::source::from_value_local(result, s)?;
        if local_original_work::same_ref(&r, &node.1, s)? {
            produced = true;
        }
    }
    require(produced, "INTEGRITY_FAILURE")?;
    let body = s
        .parse_representation(node.2.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let schema = request_wrapper::text(body.value(), "schema", s)?;
    require(
        matches!(
            schema,
            "ledger-workflow-action/1" | "ledger-workflow-cost-observation-action/1"
        ),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    request_wrapper::same_text(
        request_wrapper::text(body.value(), "decision_id", s)?,
        decision_id,
        s,
    )?;
    if let Some(value) = body.value().get("inverse_of") {
        let inverse_index = explain_owned_node(nodes, value, s)?;
        let original = s
            .parse_representation(nodes[inverse_index].2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        require(
            nodes[inverse_index].1.kind == "workflow-action",
            "INTEGRITY_FAILURE",
        )?;
        request_wrapper::same_text(
            request_wrapper::text(original.value(), "decision_id", s)?,
            decision_id,
            s,
        )?;
    }
    for field in ["inverse_of", "slot_terms_ref", "role_ref"] {
        if let Some(value) = body.value().get(field) {
            explain_owned_node(nodes, value, s)?;
        }
    }
    if let Some(refs) = body.value().get("basis_refs") {
        for value in refs.as_array().ok_or(ServiceError::IntegrityFailure)? {
            explain_owned_node(nodes, value, s)?;
        }
    }
    Ok(())
}

// Build one producer-attributed decision entry only after its containing graph
// and full-prefix custody have been verified. The caller prepays response
// Value storage and retains that owner through canonical response rendering.
async fn explain_entry(
    tx: &mut SqliteTx,
    key: &OperationKey,
    nodes: &[local_explain_family::Node],
    index: usize,
    s: &mut OriginalAuditSession,
) -> Result<Value> {
    let node = nodes.get(index).ok_or(ServiceError::IntegrityFailure)?;
    require(node.1.kind == "workflow-decision", "INTEGRITY_FAILURE")?;
    let before = node
        .0
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let origin = prefix::next(tx, key, before, node.0, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    require(
        request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)? != "semantic_alias",
        "INTEGRITY_FAILURE",
    )?;
    let results = request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut produced = false;
    for value in results {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let (r, _owner) = current::source::from_value_local(value, s)?;
        if local_original_work::same_ref(&r, &node.1, s)? {
            produced = true;
        }
    }
    require(produced, "INTEGRITY_FAILURE")?;
    let reference = wire::reference(&node.1, s)?;
    let reference = s
        .parse_representation(reference.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    origin.require_member(reference.value(), s)?;
    let pointer = origin.wrappers().origin_pointer(s)?;
    let pointer = s
        .parse_representation(pointer.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let decision = s
        .parse_representation(node.2.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let evaluation_ref = request_wrapper::field(decision.value(), "evaluation_ref", s)?;
    let evaluation_index = explain_owned_node(nodes, evaluation_ref, s)?;
    let evaluation = s
        .parse_representation(
            nodes[evaluation_index].2.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    let revision = wire::counter_string(
        node.0
            .try_into()
            .map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )?;
    let revision = s
        .parse_representation(revision.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    Ok(serde_json::json!({
        "producer_commit_sequence": revision.value(),
        "operation_key": request_wrapper::field(origin.wrappers().operation(), "key", s)?,
        "receipt": origin.wrappers().receipt(),
        "manifest_ref": request_wrapper::field(pointer.value(), "manifest_ref", s)?,
        "decision_ref": serde_json::to_value(&node.1).map_err(|_| ServiceError::IntegrityFailure)?,
        "evaluation_ref": evaluation_ref,
        "binding_ref": request_wrapper::field(evaluation.value(), "binding_ref", s)?,
        "fact_schema_ref": request_wrapper::field(evaluation.value(), "fact_schema_ref", s)?,
        "compiler_ref": request_wrapper::field(evaluation.value(), "compiler_ref", s)?,
        "compiled_plan_ref": request_wrapper::field(evaluation.value(), "compiled_plan_ref", s)?,
        "actions_appended": request_wrapper::field(decision.value(), "actions_appended", s)?,
        "active_action_refs": request_wrapper::field(decision.value(), "active_action_refs", s)?
    }))
}

// All displayed bodies have already been compared to the original fold and
// re-canonicalized. Meter aggregate tree/output storage before cloning any.
struct ExplainOutput<'a> {
    subject: &'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    root_head: &'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    through: i64,
    nodes: &'a [local_explain_family::Node],
    marks: &'a [u8],
    paths: &'a [(
        i64,
        ledgerlab_core::policy::workflow::transition_constructor::SelectedReportPath,
    )],
    claim_indices: &'a [usize],
}
async fn explain_render(
    tx: &mut SqliteTx,
    key: &OperationKey,
    output: ExplainOutput<'_>,
    s: &mut OriginalAuditSession,
) -> Result<Value> {
    let ExplainOutput {
        subject,
        root_head,
        through,
        nodes,
        marks,
        paths,
        claim_indices,
    } = output;
    let control = nodes
        .len()
        .checked_mul(2 * std::mem::size_of::<usize>())
        .ok_or(ServiceError::Unavailable)?;
    let _control_owner = s.reserve_external_payload(control).map_err(core_error)?;
    s.semantic_work().write_bytes(control).map_err(core_error)?;
    let mut records = Vec::new();
    let mut decisions = Vec::new();
    records
        .try_reserve_exact(nodes.len())
        .map_err(|_| ServiceError::Unavailable)?;
    decisions
        .try_reserve_exact(nodes.len())
        .map_err(|_| ServiceError::Unavailable)?;
    require(
        records.capacity() <= nodes.len() && decisions.capacity() <= nodes.len(),
        "INTEGRITY_FAILURE",
    )?;
    let mut bytes = 4096usize;
    for (i, node) in nodes.iter().enumerate() {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if marks[i] == 0 {
            continue;
        }
        bytes = bytes
            .checked_add(node.2.bytes().len())
            .and_then(|n| n.checked_add(1024))
            .ok_or(ServiceError::Unavailable)?;
        records.push(i);
        if node.1.kind == "workflow-decision" {
            let previous = node
                .0
                .checked_sub(1)
                .ok_or(ServiceError::IntegrityFailure)?;
            let origin = prefix::next(tx, key, previous, node.0, s)
                .await?
                .ok_or(ServiceError::IntegrityFailure)?
                .checked_wrappers(tx, s)
                .await?;
            let receipt = s
                .canonical_representation(
                    origin.wrappers().receipt(),
                    RepresentationKind::ReceiptFrame,
                )
                .map_err(core_error)?;
            bytes = bytes
                .checked_add(receipt.bytes().len())
                .and_then(|n| n.checked_add(4096))
                .ok_or(ServiceError::Unavailable)?;
            decisions.push(i);
        }
    }
    // Canonical full Ref order is digest, id, then kind. Insertion sorting
    // keeps each comparison metered and deterministic.
    for i in 1..records.len() {
        let mut j = i;
        while j > 0 {
            let a = &nodes[records[j - 1]].1;
            let b = &nodes[records[j]].1;
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            let n = a
                .digest
                .len()
                .checked_add(a.id.len())
                .and_then(|v| v.checked_add(a.kind.len()))
                .and_then(|v| v.checked_add(b.digest.len()))
                .and_then(|v| v.checked_add(b.id.len()))
                .and_then(|v| v.checked_add(b.kind.len()))
                .ok_or(ServiceError::Unavailable)?;
            s.semantic_work().scan_bytes(n).map_err(core_error)?;
            if (&a.digest, &a.id, &a.kind) <= (&b.digest, &b.id, &b.kind) {
                break;
            }
            records.swap(j - 1, j);
            j -= 1;
        }
    }
    for i in 1..decisions.len() {
        let mut j = i;
        while j > 0 {
            let a = &nodes[decisions[j - 1]];
            let b = &nodes[decisions[j]];
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            let n =
                a.1.digest
                    .len()
                    .checked_add(a.1.id.len())
                    .and_then(|v| v.checked_add(a.1.kind.len()))
                    .and_then(|v| v.checked_add(b.1.digest.len()))
                    .and_then(|v| v.checked_add(b.1.id.len()))
                    .and_then(|v| v.checked_add(b.1.kind.len()))
                    .ok_or(ServiceError::Unavailable)?;
            s.semantic_work().scan_bytes(n).map_err(core_error)?;
            if (a.0, &a.1.digest, &a.1.id, &a.1.kind) <= (b.0, &b.1.digest, &b.1.id, &b.1.kind) {
                break;
            }
            decisions.swap(j - 1, j);
            j -= 1;
        }
    }
    for (sequence, path) in paths {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        for &index in claim_indices {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            if *sequence == nodes[index].0
                && local_original_work::same_ref(&path.claim, &nodes[index].1, s)?
            {
                let refs_bytes = path
                    .links
                    .len()
                    .checked_add(1)
                    .and_then(|n| n.checked_mul(1024))
                    .ok_or(ServiceError::Unavailable)?;
                bytes = bytes
                    .checked_add(refs_bytes)
                    .ok_or(ServiceError::Unavailable)?;
            }
        }
    }
    s.semantic_work()
        .arithmetic(
            ledgerlab_core::validation_work::ArithmeticOperation::Multiply,
            &[&bytes.into(), &48usize.into()],
        )
        .map_err(core_error)?;
    let allocated = bytes.checked_mul(48).ok_or(ServiceError::Unavailable)?;
    let _response_owner = s.reserve_external_payload(allocated).map_err(core_error)?;
    s.semantic_work()
        .write_bytes(allocated)
        .map_err(core_error)?;
    let mut entries = Vec::with_capacity(decisions.len());
    for index in decisions {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        entries.push(explain_entry(tx, key, nodes, index, s).await?);
    }
    let mut bodies = Vec::with_capacity(records.len());
    for index in records {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let node = &nodes[index];
        let body = s
            .parse_representation(node.2.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        bodies.push(serde_json::json!({"ref":node.1,"body":body.value()}));
    }
    let mut selected_paths = Vec::new();
    for (sequence, path) in paths {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        for &index in claim_indices {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            if *sequence == nodes[index].0
                && local_original_work::same_ref(&path.claim, &nodes[index].1, s)?
            {
                let seq = wire::counter_string(
                    (*sequence)
                        .try_into()
                        .map_err(|_| ServiceError::IntegrityFailure)?,
                    s,
                )?;
                let seq = s
                    .parse_representation(seq.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                selected_paths.push(serde_json::json!({"claim_ref":path.claim,"producer_commit_sequence":seq.value(),"link_refs":path.links}));
            }
        }
    }
    let snapshot = wire::counter_string(
        through
            .try_into()
            .map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )?;
    let snapshot = s
        .parse_representation(snapshot.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let reply = serde_json::json!({
        "schema":"ledger-workflow-billing-explanation/1",
        "coverage":"component_work_family_decision_histories_and_selected_links",
        "logical_store_id":key.logical_store_id,
        "scope":key.scope,
        "subject_ref":subject,
        "snapshot_commit_sequence":snapshot.value(),
        "root_current_decision_ref":root_head,
        "entries":entries,
        "selected_paths":selected_paths,
        "records":bodies
    });
    s.semantic_work()
        .arithmetic(
            ledgerlab_core::validation_work::ArithmeticOperation::Multiply,
            &[&bytes.into(), &6usize.into()],
        )
        .map_err(core_error)?;
    let buffer_bound = bytes
        .checked_mul(6)
        .ok_or(ServiceError::Unavailable)?
        .min(8388608);
    let _buffer_owner = s
        .reserve_external_payload(buffer_bound)
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(buffer_bound)
        .map_err(core_error)?;
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(buffer_bound)
        .map_err(|_| ServiceError::Unavailable)?;
    require(buffer.capacity() <= buffer_bound, "INTEGRITY_FAILURE")?;
    let mut writer = ExplainBoundedWriter {
        buffer,
        maximum: buffer_bound,
    };
    if serde_json::to_writer(&mut writer, &reply).is_err() {
        return if buffer_bound == 8388608 {
            Err(reject("RESPONSE_BUDGET_EXCEEDED"))
        } else {
            Err(ServiceError::IntegrityFailure)
        };
    }
    let _canonical = s
        .canonical_representation(&reply, RepresentationKind::OperationFrame)
        .map_err(core_error)?;
    Ok(reply)
}

struct ExplainBoundedWriter {
    buffer: Vec<u8>,
    maximum: usize,
}
impl std::io::Write for ExplainBoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self
            .buffer
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("response size"))?;
        if end > self.maximum {
            return Err(std::io::Error::other("response size"));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

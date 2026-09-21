use crate::store::{errors::StoreError, records::*};
use sqlx::{Row, SqliteConnection};

pub(super) async fn journal(
    conn: &mut SqliteConnection,
    record: &JournalRecord,
) -> Result<(), StoreError> {
    let s = &record.scope;
    let b = &record.canonical;
    match &record.row {
        JournalRow::Document { id, kind } => {
            let inserted = sqlx::query("INSERT INTO documents (tenant,environment,id,kind,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,1) ON CONFLICT (tenant,environment,id) DO NOTHING")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(kind)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(&mut *conn).await?.rows_affected();
            if inserted == 0 {
                // BEGIN IMMEDIATE serializes writers; never mutate an immutable
                // document or silently accept a same-ID collision.
                let existing = sqlx::query("SELECT kind,canonical_bytes,content_hash,schema_version FROM documents WHERE tenant=? AND environment=? AND id=?")
                    .bind(&s.tenant).bind(&s.environment).bind(id).fetch_one(conn).await?;
                if existing.try_get::<String, _>(0)? != *kind
                    || existing.try_get::<Vec<u8>, _>(1)? != b.canonical_bytes
                    || existing.try_get::<String, _>(2)? != b.content_hash
                    || existing.try_get::<i64, _>(3)? != 1
                {
                    return Err(StoreError::Integrity("immutable document collision"));
                }
            }
        }
        JournalRow::Party {
            id,
            role_metadata_doc,
        } => {
            sqlx::query("INSERT INTO parties (tenant,environment,id,role_metadata_doc,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(role_metadata_doc)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::SourceGrant {
            id,
            principal_id,
            source,
            grant_doc,
        } => {
            sqlx::query("INSERT INTO source_grants (tenant,environment,id,principal_id,source,grant_doc,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(principal_id).bind(source).bind(grant_doc)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Binding {
            id,
            agreement_id,
            version,
            policy_doc,
            assent_doc,
            roles_doc,
            context_doc,
            currency,
            scale,
        } => {
            sqlx::query("INSERT INTO bindings (tenant,environment,id,agreement_id,version,policy_doc,assent_doc,roles_doc,context_doc,currency,scale,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(agreement_id).bind(version).bind(policy_doc).bind(assent_doc).bind(roles_doc).bind(context_doc).bind(currency).bind(scale)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Event {
            id,
            source,
            external_id,
            operation_id,
            kind,
            chain_id,
            decision_id,
            ingress_hash,
            claim_facts_hash,
            ingress_bytes,
            occurred_us,
            received_us,
        } => {
            sqlx::query("INSERT INTO events (tenant,environment,id,source,external_id,operation_id,kind,chain_id,decision_id,ingress_hash,claim_facts_hash,ingress_bytes,occurred_us,received_us,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(source).bind(external_id).bind(operation_id).bind(kind).bind(chain_id).bind(decision_id).bind(ingress_hash).bind(claim_facts_hash).bind(ingress_bytes).bind(occurred_us).bind(received_us)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Snapshot {
            id,
            event_id,
            document_id,
            purpose,
        } => {
            sqlx::query("INSERT INTO snapshots (tenant,environment,id,event_id,document_id,purpose,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(event_id).bind(document_id).bind(purpose)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::DeliveryKey {
            source,
            external_id,
            ingress_hash,
            canonical_event_id,
            kind,
            observed_us,
        } => {
            sqlx::query("INSERT INTO delivery_keys (tenant,environment,source,external_id,ingress_hash,canonical_event_id,kind,observed_us,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(source).bind(external_id).bind(ingress_hash).bind(canonical_event_id).bind(kind).bind(observed_us)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Claim {
            id,
            source,
            operation_id,
            kind,
            token,
            facts_hash,
            event_id,
        } => {
            sqlx::query("INSERT INTO claims (tenant,environment,id,source,operation_id,kind,token,facts_hash,event_id,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(source).bind(operation_id).bind(kind).bind(token).bind(facts_hash).bind(event_id)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Effect {
            id,
            agreement_id,
            component,
            claim_id,
            namespace,
            facts_hash,
            action_id,
            match_key_bytes,
        } => {
            sqlx::query("INSERT INTO effects (tenant,environment,id,agreement_id,component,claim_id,namespace,facts_hash,action_id,match_key_bytes,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(agreement_id).bind(component).bind(claim_id).bind(namespace).bind(facts_hash).bind(action_id).bind(match_key_bytes)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Action {
            id,
            event_id,
            decision_id,
            effect_id,
            obligation_id,
            kind,
            book,
            component,
            binding_id,
            snapshot_doc,
            roles_doc,
            currency,
            scale,
            atoms,
            reverses,
            allocation_parent,
        } => {
            sqlx::query("INSERT INTO actions (tenant,environment,id,event_id,decision_id,effect_id,obligation_id,kind,book,component,binding_id,snapshot_doc,roles_doc,currency,scale,atoms,reverses,allocation_parent,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(event_id).bind(decision_id).bind(effect_id).bind(obligation_id).bind(kind).bind(book).bind(component).bind(binding_id).bind(snapshot_doc).bind(roles_doc).bind(currency).bind(scale).bind(atoms).bind(reverses).bind(allocation_parent)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::ActionSource {
            action_id,
            event_id,
        } => {
            sqlx::query("INSERT INTO action_sources (tenant,environment,action_id,event_id,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(action_id).bind(event_id)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::ActionDependency {
            action_id,
            input_action_id,
        } => {
            sqlx::query("INSERT INTO action_dependencies (tenant,environment,action_id,input_action_id,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(action_id).bind(input_action_id)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Explanation {
            id,
            event_id,
            ordinal,
            code,
            rule_id,
        } => {
            sqlx::query("INSERT INTO explanations (tenant,environment,id,event_id,ordinal,code,rule_id,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(event_id).bind(ordinal).bind(code).bind(rule_id)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Intention {
            id,
            event_id,
            obligation_id,
            destination_id,
            idempotency_key,
        } => {
            sqlx::query("INSERT INTO intentions (tenant,environment,id,event_id,obligation_id,destination_id,idempotency_key,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(event_id).bind(obligation_id).bind(destination_id).bind(idempotency_key)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::ControlTransition {
            id,
            control_kind,
            control_id,
            from_revision,
            to_revision,
            from_event_count,
            to_event_count,
            event_id,
            document_id,
        } => {
            sqlx::query("INSERT INTO control_transitions (tenant,environment,id,control_kind,control_id,from_revision,to_revision,from_event_count,to_event_count,event_id,document_id,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(control_kind).bind(control_id).bind(from_revision).bind(to_revision).bind(from_event_count).bind(to_event_count).bind(event_id).bind(document_id)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::ChainRevision {
            chain_id,
            revision,
            event_id,
            decision_id,
        } => {
            sqlx::query("INSERT INTO chain_revisions (tenant,environment,chain_id,revision,event_id,decision_id,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(chain_id).bind(revision).bind(event_id).bind(decision_id)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Manifest {
            id,
            event_id,
            chain_id,
            revision,
            decision_hash,
        } => {
            sqlx::query("INSERT INTO decision_manifests (tenant,environment,id,event_id,chain_id,revision,decision_hash,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(event_id).bind(chain_id).bind(revision).bind(decision_hash)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
        JournalRow::Receipt {
            id,
            event_id,
            decision_id,
        } => {
            sqlx::query("INSERT INTO accepted_receipts (tenant,environment,id,event_id,decision_id,canonical_bytes,content_hash,schema_version) VALUES (?,?,?,?,?,?,?,1)")
                .bind(&s.tenant).bind(&s.environment)
                .bind(id).bind(event_id).bind(decision_id)
                .bind(&b.canonical_bytes).bind(&b.content_hash).execute(conn).await?;
        }
    }
    Ok(())
}

pub(super) async fn operation(conn: &mut SqliteConnection, op: &WriteOp) -> Result<(), StoreError> {
    match op {
        WriteOp::Journal(r) => journal(conn, r).await?,
        WriteOp::SeedInstallation(r) => {
            sqlx::query("INSERT INTO installation (singleton,tenant,environment,logical_store_id,mode,admission,dispatch_hold,dispatch_enabled,logical_schema,generation) VALUES (1,?,?,?,?,?,?,?,1,?)")
                .bind(&r.scope.tenant).bind(&r.scope.environment).bind(&r.logical_store_id).bind(&r.mode).bind(&r.admission).bind(r.dispatch_hold).bind(r.dispatch_enabled).bind(r.generation).execute(conn).await?;
        }
        WriteOp::SeedChain(r) => {
            sqlx::query("INSERT INTO chains (tenant,environment,id,customer,currency,scale,binding_set_doc,context_doc,revision,event_count) VALUES (?,?,?,?,?,?,?,?,?,?)")
                .bind(&r.scope.tenant).bind(&r.scope.environment).bind(&r.id).bind(&r.customer).bind(&r.currency).bind(r.scale).bind(&r.binding_set_doc).bind(&r.context_doc).bind(r.revision).bind(r.event_count).execute(conn).await?;
        }
        WriteOp::SeedAuthority(r) => {
            sqlx::query("INSERT INTO authority_heads (tenant,environment,id,grant_id,revision,active) VALUES (?,?,?,?,?,?)")
                .bind(&r.scope.tenant).bind(&r.scope.environment).bind(&r.id).bind(&r.grant_id).bind(r.revision).bind(r.active).execute(conn).await?;
        }
        WriteOp::SeedBinding(r) => {
            sqlx::query("INSERT INTO binding_heads (tenant,environment,id,selector_doc,binding_id,revision,active) VALUES (?,?,?,?,?,?,?)")
                .bind(&r.scope.tenant).bind(&r.scope.environment).bind(&r.id).bind(&r.selector_doc).bind(&r.binding_id).bind(r.revision).bind(r.active).execute(conn).await?;
        }
        WriteOp::AdvanceChain(r) => {
            let n=sqlx::query("UPDATE chains SET revision=?,event_count=? WHERE tenant=? AND environment=? AND id=? AND revision=? AND event_count=? AND ?=revision+1 AND ?=event_count+1 AND EXISTS (SELECT 1 FROM control_transitions t WHERE t.tenant=chains.tenant AND t.environment=chains.environment AND t.control_kind='chain' AND t.control_id=chains.id AND t.from_revision=chains.revision AND t.to_revision=? AND t.from_event_count=chains.event_count AND t.to_event_count=?)")
                .bind(r.to_revision).bind(r.to_event_count).bind(&r.scope.tenant).bind(&r.scope.environment).bind(&r.id).bind(r.from_revision).bind(r.from_event_count).bind(r.to_revision).bind(r.to_event_count).bind(r.to_revision).bind(r.to_event_count).execute(conn).await?.rows_affected();
            if n != 1 {
                return Err(StoreError::Integrity(
                    "chain compare-and-swap or transition mismatch",
                ));
            }
        }
        WriteOp::HoldDelivery(r) => {
            sqlx::query("INSERT INTO delivery_state (tenant,environment,intention_id,state,attempts,next_attempt_us,generation) VALUES (?,?,?,'held',0,?,0)")
                .bind(&r.scope.tenant).bind(&r.scope.environment).bind(&r.intention_id).bind(r.next_attempt_us).execute(conn).await?;
        }
    }
    Ok(())
}

use crate::store::{errors::StoreError, records::*};
use tokio_postgres::GenericClient;

pub(super) async fn journal<C: GenericClient + Sync>(
    conn: &C,
    record: &JournalRecord,
) -> Result<(), StoreError> {
    let s = &record.scope;
    let b = &record.canonical;
    match &record.row {
        JournalRow::Document { id, kind } => {
            let inserted = conn.execute("INSERT INTO ledgerlab.documents (tenant,environment,id,kind,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,1) ON CONFLICT (tenant,environment,id) DO NOTHING", &[&s.tenant,&s.environment,id,kind,&b.canonical_bytes,&b.content_hash]).await?;
            if inserted == 0 {
                // SERIALIZABLE either observes the immutable winner or aborts
                // with a serialization conflict for a whole-transaction retry.
                let existing = conn.query_one("SELECT kind,canonical_bytes,content_hash,schema_version FROM ledgerlab.documents WHERE tenant=$1 AND environment=$2 AND id=$3", &[&s.tenant,&s.environment,id]).await?;
                if existing.try_get::<_, String>(0)? != *kind
                    || existing.try_get::<_, Vec<u8>>(1)? != b.canonical_bytes
                    || existing.try_get::<_, String>(2)? != b.content_hash
                    || existing.try_get::<_, i64>(3)? != 1
                {
                    return Err(StoreError::Integrity("immutable document collision"));
                }
            }
        }
        JournalRow::Party {
            id,
            role_metadata_doc,
        } => {
            conn.execute("INSERT INTO ledgerlab.parties (tenant,environment,id,role_metadata_doc,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,1)", &[&(&s.tenant),&(&s.environment),&(id),&(role_metadata_doc),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::SourceGrant {
            id,
            principal_id,
            source,
            grant_doc,
        } => {
            conn.execute("INSERT INTO ledgerlab.source_grants (tenant,environment,id,principal_id,source,grant_doc,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1)", &[&(&s.tenant),&(&s.environment),&(id),&(principal_id),&(source),&(grant_doc),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
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
            conn.execute("INSERT INTO ledgerlab.bindings (tenant,environment,id,agreement_id,version,policy_doc,assent_doc,roles_doc,context_doc,currency,scale,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,1)", &[&(&s.tenant),&(&s.environment),&(id),&(agreement_id),&(version),&(policy_doc),&(assent_doc),&(roles_doc),&(context_doc),&(currency),&(scale),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
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
            conn.execute("INSERT INTO ledgerlab.events (tenant,environment,id,source,external_id,operation_id,kind,chain_id,decision_id,ingress_hash,claim_facts_hash,ingress_bytes,occurred_us,received_us,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,1)", &[&(&s.tenant),&(&s.environment),&(id),&(source),&(external_id),&(operation_id),&(kind),&(chain_id),&(decision_id),&(ingress_hash),&(claim_facts_hash),&(ingress_bytes),&(occurred_us),&(received_us),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::Snapshot {
            id,
            event_id,
            document_id,
            purpose,
        } => {
            conn.execute("INSERT INTO ledgerlab.snapshots (tenant,environment,id,event_id,document_id,purpose,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1)", &[&(&s.tenant),&(&s.environment),&(id),&(event_id),&(document_id),&(purpose),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::DeliveryKey {
            source,
            external_id,
            ingress_hash,
            canonical_event_id,
            kind,
            observed_us,
        } => {
            conn.execute("INSERT INTO ledgerlab.delivery_keys (tenant,environment,source,external_id,ingress_hash,canonical_event_id,kind,observed_us,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,1)", &[&(&s.tenant),&(&s.environment),&(source),&(external_id),&(ingress_hash),&(canonical_event_id),&(kind),&(observed_us),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
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
            conn.execute("INSERT INTO ledgerlab.claims (tenant,environment,id,source,operation_id,kind,token,facts_hash,event_id,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,1)", &[&(&s.tenant),&(&s.environment),&(id),&(source),&(operation_id),&(kind),&(token),&(facts_hash),&(event_id),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
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
            conn.execute("INSERT INTO ledgerlab.effects (tenant,environment,id,agreement_id,component,claim_id,namespace,facts_hash,action_id,match_key_bytes,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,1)", &[&(&s.tenant),&(&s.environment),&(id),&(agreement_id),&(component),&(claim_id),&(namespace),&(facts_hash),&(action_id),&(match_key_bytes),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
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
            conn.execute("INSERT INTO ledgerlab.actions (tenant,environment,id,event_id,decision_id,effect_id,obligation_id,kind,book,component,binding_id,snapshot_doc,roles_doc,currency,scale,atoms,reverses,allocation_parent,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,1)", &[&(&s.tenant),&(&s.environment),&(id),&(event_id),&(decision_id),&(effect_id),&(obligation_id),&(kind),&(book),&(component),&(binding_id),&(snapshot_doc),&(roles_doc),&(currency),&(scale),&(atoms),&(reverses),&(allocation_parent),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::ActionSource {
            action_id,
            event_id,
        } => {
            conn.execute("INSERT INTO ledgerlab.action_sources (tenant,environment,action_id,event_id,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,1)", &[&(&s.tenant),&(&s.environment),&(action_id),&(event_id),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::ActionDependency {
            action_id,
            input_action_id,
        } => {
            conn.execute("INSERT INTO ledgerlab.action_dependencies (tenant,environment,action_id,input_action_id,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,1)", &[&(&s.tenant),&(&s.environment),&(action_id),&(input_action_id),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::Explanation {
            id,
            event_id,
            ordinal,
            code,
            rule_id,
        } => {
            conn.execute("INSERT INTO ledgerlab.explanations (tenant,environment,id,event_id,ordinal,code,rule_id,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,1)", &[&(&s.tenant),&(&s.environment),&(id),&(event_id),&(ordinal),&(code),&(rule_id),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::Intention {
            id,
            event_id,
            obligation_id,
            destination_id,
            idempotency_key,
        } => {
            conn.execute("INSERT INTO ledgerlab.intentions (tenant,environment,id,event_id,obligation_id,destination_id,idempotency_key,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,1)", &[&(&s.tenant),&(&s.environment),&(id),&(event_id),&(obligation_id),&(destination_id),&(idempotency_key),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
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
            conn.execute("INSERT INTO ledgerlab.control_transitions (tenant,environment,id,control_kind,control_id,from_revision,to_revision,from_event_count,to_event_count,event_id,document_id,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,1)", &[&(&s.tenant),&(&s.environment),&(id),&(control_kind),&(control_id),&(from_revision),&(to_revision),&(from_event_count),&(to_event_count),&(event_id),&(document_id),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::ChainRevision {
            chain_id,
            revision,
            event_id,
            decision_id,
        } => {
            conn.execute("INSERT INTO ledgerlab.chain_revisions (tenant,environment,chain_id,revision,event_id,decision_id,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1)", &[&(&s.tenant),&(&s.environment),&(chain_id),&(revision),&(event_id),&(decision_id),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::Manifest {
            id,
            event_id,
            chain_id,
            revision,
            decision_hash,
        } => {
            conn.execute("INSERT INTO ledgerlab.decision_manifests (tenant,environment,id,event_id,chain_id,revision,decision_hash,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,1)", &[&(&s.tenant),&(&s.environment),&(id),&(event_id),&(chain_id),&(revision),&(decision_hash),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
        JournalRow::Receipt {
            id,
            event_id,
            decision_id,
        } => {
            conn.execute("INSERT INTO ledgerlab.accepted_receipts (tenant,environment,id,event_id,decision_id,canonical_bytes,content_hash,schema_version) VALUES ($1,$2,$3,$4,$5,$6,$7,1)", &[&(&s.tenant),&(&s.environment),&(id),&(event_id),&(decision_id),&(&b.canonical_bytes),&(&b.content_hash)]).await?;
        }
    }
    Ok(())
}

pub(crate) async fn operation<C: GenericClient + Sync>(
    conn: &C,
    op: &WriteOp,
) -> Result<(), StoreError> {
    match op {
        WriteOp::Journal(r) => journal(conn, r).await?,
        WriteOp::SeedInstallation(r) => {
            conn.execute("INSERT INTO ledgerlab.installation (singleton,tenant,environment,logical_store_id,mode,admission,dispatch_hold,dispatch_enabled,logical_schema,generation) VALUES (1,$1,$2,$3,$4,$5,$6,$7,1,$8)", &[&(&r.scope.tenant),&(&r.scope.environment),&(&r.logical_store_id),&(&r.mode),&(&r.admission),&(i64::from(r.dispatch_hold)),&(i64::from(r.dispatch_enabled)),&(r.generation)]).await?;
        }
        WriteOp::SeedChain(r) => {
            conn.execute("INSERT INTO ledgerlab.chains (tenant,environment,id,customer,currency,scale,binding_set_doc,context_doc,revision,event_count) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)", &[&(&r.scope.tenant),&(&r.scope.environment),&(&r.id),&(&r.customer),&(&r.currency),&(r.scale),&(&r.binding_set_doc),&(&r.context_doc),&(r.revision),&(r.event_count)]).await?;
        }
        WriteOp::SeedAuthority(r) => {
            conn.execute("INSERT INTO ledgerlab.authority_heads (tenant,environment,id,grant_id,revision,active) VALUES ($1,$2,$3,$4,$5,$6)", &[&(&r.scope.tenant),&(&r.scope.environment),&(&r.id),&(&r.grant_id),&(r.revision),&(i64::from(r.active))]).await?;
        }
        WriteOp::SeedBinding(r) => {
            conn.execute("INSERT INTO ledgerlab.binding_heads (tenant,environment,id,selector_doc,binding_id,revision,active) VALUES ($1,$2,$3,$4,$5,$6,$7)", &[&(&r.scope.tenant),&(&r.scope.environment),&(&r.id),&(&r.selector_doc),&(&r.binding_id),&(r.revision),&(i64::from(r.active))]).await?;
        }
        WriteOp::AdvanceChain(r) => {
            let n=conn.execute("UPDATE ledgerlab.chains SET revision=$1,event_count=$2 WHERE tenant=$3 AND environment=$4 AND id=$5 AND revision=$6 AND event_count=$7 AND $8=revision+1 AND $9=event_count+1 AND EXISTS (SELECT 1 FROM ledgerlab.control_transitions t WHERE t.tenant=chains.tenant AND t.environment=chains.environment AND t.control_kind='chain' AND t.control_id=chains.id AND t.from_revision=chains.revision AND t.to_revision=$10 AND t.from_event_count=chains.event_count AND t.to_event_count=$11)", &[&(r.to_revision),&(r.to_event_count),&(&r.scope.tenant),&(&r.scope.environment),&(&r.id),&(r.from_revision),&(r.from_event_count),&(r.to_revision),&(r.to_event_count),&(r.to_revision),&(r.to_event_count)]).await?;
            if n != 1 {
                return Err(StoreError::Integrity(
                    "chain compare-and-swap or transition mismatch",
                ));
            }
        }
        WriteOp::HoldDelivery(r) => {
            conn.execute("INSERT INTO ledgerlab.delivery_state (tenant,environment,intention_id,state,attempts,next_attempt_us,generation) VALUES ($1,$2,$3,'held',0,$4,0)", &[&(&r.scope.tenant),&(&r.scope.environment),&(&r.intention_id),&(r.next_attempt_us)]).await?;
        }
    }
    Ok(())
}

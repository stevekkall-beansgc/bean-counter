//! Local R3 host. Paths, authority sources and the original-base verifier are
//! trusted embedding-host configuration, never selected by submitted commands.
use crate::{
    service::{
        accept::{adjudication::*, outcome},
        store_error,
    },
    store::{
        adjudication::*,
        outcomes::*,
        ports::AcceptanceTx,
        sqlite::{SqliteAdjudicationStore, SqliteStore, SqliteTx},
    },
    ServiceError,
};
use ledgerlab_core::adjudication::{
    self as r3, commands as wire,
    runtime::{self as rt, accounting::Worksheet, points::State},
    types::{Count, Id, Time},
    ParsedCommand, Validate,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::Instant;
mod original;
pub use original::{
    OriginalAuthorityGrant, OriginalBaseAttestation, OriginalBaseProposal,
    OriginalBaseProvisioning, OriginalBaseVerifier, OriginalBaseView, OriginalBinding,
    OriginalHeadView, OriginalPermission, OriginalRecordReference, OriginalVerificationContext,
};
#[cfg(test)]
mod tests;
fn core<T>(v: ledgerlab_core::Result<T>) -> Result<T, ServiceError> {
    v.map_err(|e| ServiceError::Rejection(e.code.into()))
}
fn require(ok: bool, code: &str) -> Result<(), ServiceError> {
    if ok {
        Ok(())
    } else {
        Err(ServiceError::Rejection(code.into()))
    }
}
/// Authenticated host values. No request-supplied permission flags are accepted.
#[derive(Clone, Debug)]
pub struct HostContext {
    pub principal: Id,
    pub source: r3::types::Source,
    pub observed_at: Time,
}
/// Fixed local journal entry. This is control-plane configuration, not request data.
#[derive(Clone, Debug)]
pub struct JournalConfig {
    pub database: PathBuf,
    pub anchor: PathBuf,
    pub store: Id,
    pub scope: wire::Scope,
    pub registration: Id,
    pub host: Id,
    /// Trusted target for this enrollment, including pre-enrollment reads.
    pub target: Id,
    pub resources: wire::Resource,
    pub ceiling: wire::Resource,
    pub legacy_pages: u32,
    pub backing_bytes: Count,
    pub authority_source: Id,
    pub authority_id: Id,
}
impl JournalConfig {
    fn identity(&self) -> JournalIdentity {
        JournalIdentity {
            store: self.store.clone(),
            scope: self.scope.clone(),
            registration: self.registration.clone(),
            host: self.host.clone(),
        }
    }
}
/// A bounded immutable request containing exactly kind/key/payload. Host authority
/// fields are constructed from current observations and cannot be supplied here.
#[derive(Clone, Debug)]
pub struct CommandProposal {
    value: Value,
}
impl CommandProposal {
    pub fn parse(bytes: &[u8]) -> Result<Self, ServiceError> {
        let v = core(ledgerlab_core::canonical::parse_bounded(
            bytes,
            r3::COMMAND_BYTES,
        ))?;
        require(
            v.as_object().is_some_and(|m| {
                m.len() == 3
                    && ["kind", "key", "payload"]
                        .iter()
                        .all(|k| m.contains_key(*k))
            }),
            "COMMAND_PROPOSAL",
        )?;
        require(
            core(r3::canonical_bytes(&v, r3::COMMAND_BYTES))? == bytes,
            "COMMAND_PROPOSAL",
        )?;
        Ok(Self { value: v })
    }
}
struct Entry {
    store: SqliteStore,
    configured: SqliteAdjudicationStore,
    journal: JournalIdentity,
    authority: HeadKey,
    target: Id,
}
pub struct LocalHost {
    entries: BTreeMap<String, Entry>,
    sources: Vec<wire::AuthoritySource>,
    verifier: Arc<dyn OriginalBaseVerifier>,
    original_authority_head: Id,
}
impl LocalHost {
    /// Trusted control-plane creation of empty sandbox journals with dispatch held.
    /// Existing nonempty paths are refused by the store; no data is overwritten.
    pub async fn create_sandbox(
        configs: Vec<JournalConfig>,
        sources: Vec<wire::AuthoritySource>,
        original_authority_head: Id,
        verifier: Arc<dyn OriginalBaseVerifier>,
    ) -> Result<Self, ServiceError> {
        require(!configs.is_empty() && configs.len() <= 5, "HOST_REGISTRY")?;
        for c in &configs {
            let store = SqliteStore::create_fenced(
                &c.database,
                crate::store::records::Installation {
                    scope: crate::store::records::Scope {
                        tenant: c.scope.0.as_str().into(),
                        environment: c.scope.1.as_str().into(),
                    },
                    logical_store_id: c.store.as_str().into(),
                    mode: "sandbox".into(),
                    admission: "open".into(),
                    dispatch_hold: true,
                    dispatch_enabled: false,
                    generation: 0,
                },
                &c.anchor,
            )
            .await
            .map_err(store_error)?;
            store.close().await;
        }
        Self::open(configs, sources, original_authority_head, verifier).await
    }
    /// Open a fixed registry of fenced journals with a mandatory host verifier.
    /// No permissive default or per-request verifier exists.
    pub async fn open(
        configs: Vec<JournalConfig>,
        sources: Vec<wire::AuthoritySource>,
        original_authority_head: Id,
        verifier: Arc<dyn OriginalBaseVerifier>,
    ) -> Result<Self, ServiceError> {
        require(!configs.is_empty() && configs.len() <= 5, "HOST_REGISTRY")?;
        validate_sources(&sources)?;
        core(original_authority_head.validate())?;
        let mut entries = BTreeMap::new();
        for c in configs {
            let j = c.identity();
            core(c.scope.validate())?;
            for id in [
                &c.store,
                &c.registration,
                &c.host,
                &c.target,
                &c.authority_source,
                &c.authority_id,
            ] {
                core(id.validate())?;
            }
            require(!entries.contains_key(c.host.as_str()), "HOST_REGISTRY")?;
            if let Some(e) = entries.values().next() {
                let e: &Entry = e;
                require(
                    e.journal.store == j.store
                        && e.journal.scope == j.scope
                        && e.journal.registration == j.registration
                        && e.target == c.target,
                    "HOST_REGISTRY",
                )?;
            }
            let store = SqliteStore::open_fenced(&c.database, &c.anchor)
                .await
                .map_err(store_error)?;
            let configured = store
                .provision_adjudication_with_ceiling(
                    j.clone(),
                    c.resources,
                    c.ceiling,
                    c.legacy_pages,
                    c.backing_bytes,
                )
                .await
                .map_err(store_error)?;
            let authority = HeadKey {
                journal: j.clone(),
                kind: HeadKind::Authority,
                full_key: core(rt::index_key(
                    *b"AUTHCURR",
                    &[
                        c.authority_source.as_str().as_bytes(),
                        c.authority_id.as_str().as_bytes(),
                    ],
                ))?,
            };
            entries.insert(
                j.host.as_str().into(),
                Entry {
                    store,
                    configured,
                    journal: j,
                    authority,
                    target: c.target,
                },
            );
        }
        Ok(Self {
            entries,
            sources,
            verifier,
            original_authority_head,
        })
    }
    /// Trusted host setup only. The request API never calls this method.
    pub async fn install_authority(
        &self,
        host: &Id,
        source: wire::AuthoritySource,
        expected: Option<Count>,
    ) -> Result<(), ServiceError> {
        let e = self.entry(host)?;
        e.store
            .provision_adjudication_authority(&e.journal, &source, expected)
            .await
            .map_err(store_error)?;
        Ok(())
    }
    pub async fn provision_original(
        &self,
        host: &Id,
        setup: OriginalBaseProvisioning,
    ) -> Result<(), ServiceError> {
        let e = self.entry(host)?;
        require(e.journal.host == e.journal.store, "WRONG_OWNER")?;
        setup
            .install(&e.store, &e.journal, &self.original_authority_head)
            .await
    }
    fn entry(&self, host: &Id) -> Result<&Entry, ServiceError> {
        self.entries
            .get(host.as_str())
            .ok_or(ServiceError::Rejection("SOURCE_HOST_UNCONFIGURED".into()))
    }
    pub async fn execute(
        &self,
        host: &Id,
        proposal: &CommandProposal,
        context: HostContext,
        base: Option<&OriginalBaseProposal>,
        timeout: Duration,
    ) -> Result<wire::CommandResult, ServiceError> {
        validate_context(&context)?;
        let e = self.entry(host)?;
        let deadline = Instant::now() + timeout.min(Duration::from_secs(30));
        let key: wire::Delivery = serde_json::from_value(proposal.value["key"].clone())
            .map_err(|_| ServiceError::Rejection("COMMAND_PROPOSAL".into()))?;
        core(key.validate())?;
        require(key.0 == e.journal.scope, "WRONG_OWNER")?;
        let kind = proposal.value["kind"]
            .as_str()
            .ok_or(ServiceError::Rejection("COMMAND_PROPOSAL".into()))?;
        let (discovery, prefix, saved) = discover(e, &key, kind, deadline).await?;
        let (source, body) = authority_source(&discovery)?;
        require(
            body["principal"] == context.principal.as_str(),
            "HOST_PRINCIPAL",
        )?;
        let permission = permission(&proposal.value, saved);
        authorize_host(&body, &context, e, permission)?;
        if kind == "ENROLL" {
            require(
                proposal.value["payload"]["target"] == e.target.as_str(),
                "HOST_TARGET",
            )?;
        }
        let mut value = proposal.value.clone();
        value["authority"] = json!({"principal":context.principal,"permission":permission,"document":source.body_hash,"revision":body["revision"],"observed_at":context.observed_at,"head":prefix.root(),"command":"0".repeat(64)});
        let mut command = core(ParsedCommand::parse(&core(r3::canonical_bytes(
            &value,
            r3::COMMAND_BYTES,
        ))?))?;
        value["authority"]["command"] = json!(core(rt::command_digest(command.command()))?);
        command = core(ParsedCommand::parse(&core(r3::canonical_bytes(
            &value,
            r3::COMMAND_BYTES,
        ))?))?;
        let original = if kind == "ENROLL" && !saved {
            Some(
                base.ok_or(ServiceError::Rejection("ORIGINAL_BASE_REQUIRED".into()))?
                    .command(&context, &self.original_authority_head)?,
            )
        } else {
            None
        };
        let h = RequestHost {
            host: self,
            context,
            discovery,
            original,
        };
        run(&e.configured, &h, e.journal.clone(), command, deadline).await
    }
    /// Export a proof only from a registered actual primary. Paths never come from
    /// the locator. A successful immutable read is authorized at the final
    /// authority snapshot; a changed authorization during export is refused.
    /// Revocation after that final check does not retract the completed read.
    pub async fn source_proof(
        &self,
        host: &Id,
        ordinal: Count,
        kind: wire::FactKind,
        key: wire::ProofFullKey,
        context: HostContext,
        timeout: Duration,
    ) -> Result<wire::Proof, ServiceError> {
        validate_context(&context)?;
        let e = self.entry(host)?;
        let deadline = Instant::now() + timeout.min(Duration::from_secs(30));
        let dummy = wire::Delivery(
            e.journal.scope.clone(),
            context.source.clone(),
            core(Id::parse("host-source-read"))?,
        );
        let (h, _, _) = discover(e, &dummy, "ENROLL", deadline).await?;
        let (_, body) = authority_source(&h)?;
        authorize_host(&body, &context, e, "read")?;
        let source = e
            .store
            .adjudication_source(&e.journal, ordinal, &kind, &key)
            .await
            .map_err(store_error)?;
        recheck(e, &dummy, "ENROLL", &h, &context, "read", deadline).await?;
        Ok(source.proof().clone())
    }
    /// Trusted host fence discovery for a later REPLACE_WRITER proposal. This
    /// returns no capability and reserves no replacement; execute reauthorizes.
    pub async fn writer_fence(
        &self,
        host: &Id,
        context: HostContext,
        timeout: Duration,
    ) -> Result<r3::types::Digest, ServiceError> {
        validate_context(&context)?;
        let e = self.entry(host)?;
        let key = wire::Delivery(
            e.journal.scope.clone(),
            context.source.clone(),
            core(Id::parse("host-fence-read"))?,
        );
        let deadline = Instant::now() + timeout.min(Duration::from_secs(30));
        let (head, _, _) = discover(e, &key, "REPLACE_WRITER", deadline).await?;
        let (_, body) = authority_source(&head)?;
        authorize_host(&body, &context, e, "replace")?;
        let fence = e
            .configured
            .writer_fence(deadline)
            .await
            .map_err(store_error)?;
        recheck(
            e,
            &key,
            "REPLACE_WRITER",
            &head,
            &context,
            "replace",
            deadline,
        )
        .await?;
        Ok(fence)
    }
    pub async fn close(self) {
        for e in self.entries.into_values() {
            e.store.close().await;
        }
    }
}
fn permission(v: &Value, saved: bool) -> &'static str {
    if saved {
        return "read";
    }
    match v["kind"].as_str().unwrap_or("") {
        "ENROLL" => "enroll",
        "RECEIVE" | "SUPPLEMENT" => "submit",
        "BEGIN" | "CLOSE" | "ABORT" => "close",
        "CORRECT" => "correct",
        "REPLACE_WRITER" => "replace",
        "DECIDE" if v["payload"]["path"] == "ADJUSTMENT" => "adjust",
        "DECIDE" => "decide",
        _ => "capacity",
    }
}
fn validate_sources(sources: &[wire::AuthoritySource]) -> Result<(), ServiceError> {
    require(sources.len() <= 83, "AUTH_SOURCE_COUNT")?;
    let mut total = 0;
    let mut identities = BTreeMap::new();
    for s in sources {
        let raw = core(r3::proofs::decode_base64(&s.body, 16384))?;
        core(s.validate())?;
        require(
            raw.len() as u128 == s.bytes.value() && r3::raw_sha256(&raw) == s.body_hash,
            "AUTH_SOURCE_HASH",
        )?;
        let b: wire::AuthoritySourceBody = core(r3::parse_exact(&raw, 16384))?;
        let v = serde_json::to_value(b).map_err(|_| ServiceError::IntegrityFailure)?;
        let key = core(r3::canonical_bytes(
            &json!([v["source"], v["id"], v["revision"]]),
            4096,
        ))?;
        require(
            identities.insert(key, s.body_hash.clone()).is_none(),
            "AUTH_SOURCE_IDENTITY_CONFLICT",
        )?;
        total += raw.len();
    }
    require(total <= 524288, "AUTH_SOURCE_BYTES")
}
fn authority_source(h: &ObservedHead) -> Result<(wire::AuthoritySource, Value), ServiceError> {
    let raw = h
        .value
        .as_deref()
        .ok_or(ServiceError::Rejection("HOST_AUTHORITY_MISSING".into()))?;
    let state: State = serde_json::from_slice(raw).map_err(|_| ServiceError::IntegrityFailure)?;
    let State::AuthorityCurrent(s) = state else {
        return Err(ServiceError::IntegrityFailure);
    };
    validate_sources(std::slice::from_ref(&s))?;
    let body = core(r3::proofs::decode_base64(&s.body, 16384))?;
    let v = core(ledgerlab_core::canonical::parse_bounded(&body, 16384))?;
    require(v["kind"] == "AUTHORIZATION", "HOST_AUTHORITY")?;
    Ok((s, v))
}
fn validate_context(c: &HostContext) -> Result<(), ServiceError> {
    core(c.principal.validate())?;
    core(c.source.validate())?;
    core(c.observed_at.validate())
}
fn authorize_host(
    body: &Value,
    c: &HostContext,
    e: &Entry,
    permission: &str,
) -> Result<(), ServiceError> {
    require(
        body["principal"] == c.principal.as_str()
            && body["scope"] == json!(e.journal.scope)
            && body["target"] == e.target.as_str()
            && body["permissions"]
                .as_array()
                .is_some_and(|p| p.contains(&json!(permission)))
            && body["starts_at"]
                .as_str()
                .is_some_and(|t| t <= c.observed_at.as_str())
            && body["ends_at"]
                .as_str()
                .is_some_and(|t| c.observed_at.as_str() < t),
        "HOST_READ_AUTHORITY",
    )
}
async fn recheck(
    e: &Entry,
    key: &wire::Delivery,
    kind: &str,
    before: &ObservedHead,
    context: &HostContext,
    permission: &str,
    deadline: Instant,
) -> Result<(), ServiceError> {
    let (after, _, _) = discover(e, key, kind, deadline).await?;
    require(
        before.revision == after.revision && before.value == after.value,
        "HOST_AUTHORITY_CHANGED",
    )?;
    let (_, body) = authority_source(&after)?;
    authorize_host(&body, context, e, permission)
}
async fn discover(
    e: &Entry,
    key: &wire::Delivery,
    kind: &str,
    deadline: Instant,
) -> Result<(ObservedHead, TrustedJournalHead, bool), ServiceError> {
    let w = core(Worksheet::frozen())?;
    let t = core(w.template(kind))?;
    let work = WorkRequest {
        journal: e.journal.clone(),
        owner: core(Id::parse("host-discovery"))?,
        transition: r3::raw_sha256(b"host-discovery"),
        mandatory: !matches!(
            kind,
            "PREPARE_ENROLL"
                | "ENROLL"
                | "LOCAL_GRANT"
                | "REGISTER_GRANT"
                | "ISSUE"
                | "DECIDE"
                | "CORRECT"
                | "SUPPLEMENT"
                | "EXTEND_RESOURCES"
                | "REPLACE_WRITER"
        ),
        maximum: core(t.resources())?,
    };
    let mut tx = e
        .configured
        .begin_adjudication(&work, deadline)
        .await
        .map_err(store_error)?;
    let guards = vec![
        Guard::Legacy(OutcomeLock {
            class: OutcomeLockClass::Admission,
            key: core(r3::canonical_bytes(&json!([e.journal.scope]), 4096))?,
            mode: OutcomeLockMode::Write,
        }),
        Guard::R3 {
            class: GuardClass::EnrollmentNamespace,
            host: e.journal.host.clone(),
            key: e.authority.full_key.clone(),
        },
    ];
    let enrollment = HeadKey {
        journal: e.journal.clone(),
        kind: HeadKind::Enrollment,
        full_key: core(rt::index_key(
            *b"ENROLL__",
            &[e.journal.registration.as_str().as_bytes()],
        ))?,
    };
    let result = async {
        tx.lock_adjudication(&guards).await.map_err(store_error)?;
        let saved = tx
            .lookup_adjudication(&e.journal, key)
            .await
            .map_err(store_error)?
            .is_some();
        let Resolution::Complete(i) = tx
            .resolve_adjudication(&ResolveRequest {
                journal: e.journal.clone(),
                key: key.clone(),
                guards,
                objects: vec![],
                heads: vec![e.authority.clone(), enrollment.clone()],
            })
            .await
            .map_err(store_error)?
        else {
            return Err(ServiceError::IntegrityFailure);
        };
        if let Some(raw) = i
            .heads
            .iter()
            .find(|h| h.key == enrollment)
            .and_then(|h| h.value.as_deref())
        {
            let state: State =
                serde_json::from_slice(raw).map_err(|_| ServiceError::IntegrityFailure)?;
            match state {
                State::Enrollment(v) => require(v.terms.target == e.target, "HOST_TARGET")?,
                State::Preparation(_) => {}
                _ => return Err(ServiceError::IntegrityFailure),
            }
        }
        let h = i
            .heads
            .into_iter()
            .find(|h| h.key == e.authority)
            .ok_or(ServiceError::IntegrityFailure)?;
        Ok((h, i.prefix, saved))
    }
    .await;
    tx.rollback().await.map_err(store_error)?;
    result
}
struct RequestHost<'a> {
    host: &'a LocalHost,
    context: HostContext,
    discovery: ObservedHead,
    original: Option<outcome::OutcomeCommand>,
}
impl AdjudicationAuthority for RequestHost<'_> {
    fn current(
        &self,
        c: &ParsedCommand,
        i: &LockedInputs,
        access: AuthorityAccess,
    ) -> Result<AuthorityObservation, ServiceError> {
        let head = i
            .heads
            .iter()
            .find(|h| h.key == self.discovery.key)
            .unwrap_or(&self.discovery);
        let (s, body) = authority_source(head)?;
        require(
            body["principal"] == self.context.principal.as_str(),
            "HOST_PRINCIPAL",
        )?;
        let v = core(rt::command_value(c.command()))?;
        authorize_host(
            &body,
            &self.context,
            self.host.entry(&i.prefix.journal().host)?,
            permission(&v, matches!(access, AuthorityAccess::ReadSavedResult)),
        )?;
        let mut sources = Vec::new();
        for exact in &self.host.sources {
            let raw = core(r3::proofs::decode_base64(&exact.body, 16384))?;
            let old = core(ledgerlab_core::canonical::parse_bounded(&raw, 16384))?;
            // Current host authorization comes only from its locked head, even
            // when trusted immutable configuration still retains an old revision.
            if old["source"] != body["source"] || old["id"] != body["id"] {
                sources.push(exact.clone());
            }
        }
        sources.push(s.clone());
        core(AuthorityObservation::from_backend(
            self.context.principal.clone(),
            core(rt::command_digest(c.command()))?,
            s.body_hash,
            core(Count::parse(
                body["revision"]
                    .as_str()
                    .ok_or(ServiceError::IntegrityFailure)?,
            ))?,
            self.context.observed_at.clone(),
            serde_json::from_value(json!(permission(
                &v,
                matches!(access, AuthorityAccess::ReadSavedResult)
            )))
            .map_err(|_| ServiceError::IntegrityFailure)?,
            sources,
            vec![head.clone()],
        ))
    }
}
impl AdjudicationHost<SqliteTx> for RequestHost<'_> {
    fn guards(&self, _: &ParsedCommand, _: &JournalIdentity) -> Result<Vec<Guard>, ServiceError> {
        self.original
            .as_ref()
            .map(|c| {
                outcome::fresh_base_locks(c).map(|v| v.into_iter().map(Guard::Legacy).collect())
            })
            .unwrap_or(Ok(vec![]))
    }
    async fn source(&self, r: &SourceRequest) -> Result<VerifiedSource, ServiceError> {
        match r {
            SourceRequest::Exact(p) => {
                let e = self.host.entry(&p.host)?;
                require(
                    p.store == e.journal.store
                        && p.scope == e.journal.scope
                        && p.registration == e.journal.registration,
                    "SOURCE_JOURNAL",
                )?;
                e.store
                    .adjudication_exact_source(p)
                    .await
                    .map_err(store_error)
            }
            SourceRequest::Enrollment { journal } => {
                let e = self.host.entry(&journal.host)?;
                require(e.journal == *journal, "SOURCE_JOURNAL")?;
                e.store
                    .adjudication_enrollment_source(journal)
                    .await
                    .map_err(store_error)
            }
        }
    }
    async fn fresh_base(
        &self,
        tx: &mut SqliteTx,
        terms: &wire::Enroll,
        inputs: &LockedInputs,
    ) -> Result<FreshBaseAcceptance, ServiceError> {
        original::prepare(self, tx, terms, inputs).await
    }
}

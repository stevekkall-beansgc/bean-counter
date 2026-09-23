//! Offline receipt intake over an already enrolled, activated, fenced local journal.
//! This handle has no peer transport. Receipt acknowledgement is not commercial ALLOW.
use crate::{
    service::{accept::adjudication::*, store_error},
    store::{
        adjudication::*,
        sqlite::{SqliteAdjudicationStore, SqliteStore, SqliteTx},
    },
    ServiceError,
};
use ledgerlab_core::adjudication::{
    self as r3, commands as wire,
    runtime::{self as rt, points::State},
    types::{Count, Id, Time},
    ParsedCommand, Validate,
};
use std::{path::PathBuf, time::Duration};
use tokio::time::Instant;

/// Trusted embedding-host configuration, never selected by a submitted request.
/// Allocation values must exactly match the previously provisioned local profile.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    pub database: PathBuf,
    pub anchor: PathBuf,
    pub central_store: Id,
    pub scope: wire::Scope,
    pub registration: Id,
    pub gateway: Id,
    pub resources: wire::Resource,
    pub legacy_pages: u32,
    pub backing_bytes: Count,
    pub authority_source: Id,
    pub authority_id: Id,
}
/// Values supplied by the authenticated host and its clock, not event fields.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayContext {
    pub principal: Id,
    pub observed_at: Time,
}
pub struct OfflineGateway {
    store: SqliteStore,
    configured: SqliteAdjudicationStore,
    journal: JournalIdentity,
    authority: HeadKey,
}
fn core<T>(result: ledgerlab_core::Result<T>) -> Result<T, ServiceError> {
    result.map_err(|e| ServiceError::Rejection(e.code.into()))
}
impl OfflineGateway {
    #[cfg(test)]
    pub(crate) fn test_store(&self) -> &SqliteStore {
        &self.store
    }
    /// Reopen the same authoritative journal and its independently retained anchor.
    /// Enrollment, grants and activation must already have committed locally.
    pub async fn open(config: GatewayConfig) -> Result<Self, ServiceError> {
        let journal = JournalIdentity {
            store: config.central_store.clone(),
            scope: config.scope.clone(),
            registration: config.registration.clone(),
            host: config.gateway.clone(),
        };
        let authority = HeadKey {
            journal: journal.clone(),
            kind: HeadKind::Authority,
            full_key: core(rt::index_key(
                *b"AUTHCURR",
                &[
                    config.authority_source.as_str().as_bytes(),
                    config.authority_id.as_str().as_bytes(),
                ],
            ))?,
        };
        let store = SqliteStore::open_fenced(&config.database, &config.anchor)
            .await
            .map_err(store_error)?;
        let configured = store
            .provision_adjudication(
                journal.clone(),
                config.resources.clone(),
                config.legacy_pages,
                config.backing_bytes,
            )
            .await
            .map_err(store_error)?;
        Ok(Self {
            store,
            configured,
            journal,
            authority,
        })
    }
    /// Accept one exact bounded receipt request. A retry returns its retained receipt;
    /// new requests consume only the token's existing local grant allocation.
    pub async fn receive(
        &self,
        key: wire::Delivery,
        payload: wire::Receive,
        context: GatewayContext,
        timeout: Duration,
    ) -> Result<wire::CommandResult, ServiceError> {
        core(context.principal.validate())?;
        core(context.observed_at.validate())?;
        core(key.validate())?;
        core(payload.validate())?;
        if payload.gateway != self.journal.host || key.0 != self.journal.scope {
            return Err(ServiceError::Rejection("WRONG_OWNER".into()));
        }
        let deadline = Instant::now() + timeout.min(Duration::from_secs(30));
        let (discovery, saved, prefix) = self
            .store
            .adjudication_gateway_lookup(
                &self.journal,
                &self.authority,
                Some(key.clone()),
                deadline,
            )
            .await
            .map_err(store_error)?;
        let host = LocalHost { discovery, context };
        let permission = if saved.is_some() {
            wire::AuthorityPermission::Read
        } else {
            wire::AuthorityPermission::Submit
        };
        host.authorize(&prefix, permission.clone())?;
        let mut value = serde_json::json!({"kind":"RECEIVE","key":key,"payload":payload,"authority":host.authority(permission)?});
        value["authority"]["head"] = serde_json::json!(prefix.root);
        // command_digest excludes the host authority observation.
        value["authority"]["command"] = serde_json::json!(core(rt::hash(
            "command",
            &serde_json::json!([value["kind"], value["key"], value["payload"]["submission"]])
        ))?);
        let command = core(ParsedCommand::parse(&core(r3::canonical_bytes(
            &value,
            r3::COMMAND_BYTES,
        ))?))?;
        self.execute(&host, command, deadline).await
    }
    /// Resolve a lost acknowledgement without changing identities or appending work.
    /// Missing means no saved result in the selected authoritative local snapshot.
    pub async fn status(
        &self,
        key: wire::Delivery,
        context: GatewayContext,
        timeout: Duration,
    ) -> Result<Option<wire::CommandResult>, ServiceError> {
        core(context.principal.validate())?;
        core(context.observed_at.validate())?;
        core(key.validate())?;
        if key.0 != self.journal.scope {
            return Err(ServiceError::Rejection("WRONG_OWNER".into()));
        }
        let deadline = Instant::now() + timeout.min(Duration::from_secs(30));
        let (discovery, saved, prefix) = self
            .store
            .adjudication_gateway_lookup(&self.journal, &self.authority, Some(key), deadline)
            .await
            .map_err(store_error)?;
        let host = LocalHost { discovery, context };
        host.authorize(&prefix, wire::AuthorityPermission::Read)?;
        let Some(saved) = saved else {
            return Ok(None);
        };
        let mut value = core(ledgerlab_core::canonical::parse_bounded(
            &saved.command,
            r3::COMMAND_BYTES,
        ))?;
        if value["kind"] != "RECEIVE" {
            return Err(ServiceError::Rejection("NOT_GATEWAY_RECEIPT".into()));
        }
        value["authority"] = host.authority(wire::AuthorityPermission::Read)?;
        value["authority"]["head"] = serde_json::json!(prefix.root);
        value["authority"]["command"] = serde_json::json!(core(rt::hash(
            "command",
            &serde_json::json!([value["kind"], value["key"], value["payload"]["submission"]])
        ))?);
        let command = core(ParsedCommand::parse(&core(r3::canonical_bytes(
            &value,
            r3::COMMAND_BYTES,
        ))?))?;
        self.execute(&host, command, deadline).await.map(Some)
    }
    async fn execute(
        &self,
        host: &LocalHost,
        command: ParsedCommand,
        deadline: Instant,
    ) -> Result<wire::CommandResult, ServiceError> {
        run(
            &self.configured,
            host,
            self.journal.clone(),
            command,
            deadline,
        )
        .await
    }

    pub async fn close(self) {
        self.store.close().await;
    }
}
struct LocalHost {
    discovery: ObservedHead,
    context: GatewayContext,
}
fn source(
    head: &ObservedHead,
) -> Result<(wire::AuthoritySource, wire::AuthoritySourceBody), ServiceError> {
    let raw = head
        .value
        .as_deref()
        .ok_or(ServiceError::IntegrityFailure)?;
    let state: State = serde_json::from_slice(raw).map_err(|_| ServiceError::IntegrityFailure)?;
    let State::AuthorityCurrent(source) = state else {
        return Err(ServiceError::IntegrityFailure);
    };
    let bytes = core(r3::proofs::decode_base64(&source.body, 16384))?;
    core(source.validate())?;
    if bytes.len() as u128 != source.bytes.value() || r3::raw_sha256(&bytes) != source.body_hash {
        return Err(ServiceError::IntegrityFailure);
    }
    let body = core(r3::parse_exact(&bytes, 16384))?;
    Ok((source, body))
}
impl LocalHost {
    fn authorize(
        &self,
        prefix: &wire::ExpectedPrefix,
        permission: wire::AuthorityPermission,
    ) -> Result<(), ServiceError> {
        let (_, body) = source(&self.discovery)?;
        let wire::AuthoritySourceBody::Authorization {
            principal,
            scope,
            target,
            permissions,
            starts_at,
            ends_at,
            ..
        } = body
        else {
            return Err(ServiceError::IntegrityFailure);
        };
        let requested = match permission {
            wire::AuthorityPermission::Read => {
                wire::AuthoritySourceBodyAuthorizationPermissionsItem::Read
            }
            wire::AuthorityPermission::Submit => {
                wire::AuthoritySourceBodyAuthorizationPermissionsItem::Submit
            }
            _ => return Err(ServiceError::IntegrityFailure),
        };
        if principal != self.context.principal
            || scope != prefix.scope
            || target != prefix.target
            || !permissions.contains(&requested)
            || starts_at.as_str() > self.context.observed_at.as_str()
            || self.context.observed_at.as_str() >= ends_at.as_str()
        {
            return Err(ServiceError::Rejection("GATEWAY_AUTHORITY".into()));
        }
        Ok(())
    }
    fn authority(
        &self,
        permission: wire::AuthorityPermission,
    ) -> Result<serde_json::Value, ServiceError> {
        let (source, body) = source(&self.discovery)?;
        let wire::AuthoritySourceBody::Authorization { revision, .. } = body else {
            return Err(ServiceError::IntegrityFailure);
        };
        Ok(
            serde_json::json!({"principal":self.context.principal,"permission":permission,"document":source.body_hash,"revision":revision,"observed_at":self.context.observed_at,"command":"0".repeat(64),"head":r3::raw_sha256(self.discovery.value.as_deref().ok_or(ServiceError::IntegrityFailure)?)}),
        )
    }
}
impl AdjudicationAuthority for LocalHost {
    fn current(
        &self,
        command: &ParsedCommand,
        inputs: &LockedInputs,
        access: AuthorityAccess,
    ) -> Result<AuthorityObservation, ServiceError> {
        let observed = inputs
            .heads
            .iter()
            .find(|h| h.key == self.discovery.key)
            .unwrap_or(&self.discovery);
        let (source, body) = source(observed)?;
        let wire::AuthoritySourceBody::Authorization {
            principal,
            revision,
            ..
        } = body
        else {
            return Err(ServiceError::IntegrityFailure);
        };
        if principal != self.context.principal {
            return Err(ServiceError::Rejection("GATEWAY_PRINCIPAL".into()));
        }
        core(AuthorityObservation::from_backend(
            principal,
            core(rt::command_digest(command.command()))?,
            source.body_hash.clone(),
            revision,
            self.context.observed_at.clone(),
            match access {
                AuthorityAccess::NewTransition => wire::AuthorityPermission::Submit,
                AuthorityAccess::ReadSavedResult => wire::AuthorityPermission::Read,
            },
            vec![source],
            vec![observed.clone()],
        ))
    }
}
impl AdjudicationHost<SqliteTx> for LocalHost {
    fn guards(&self, _: &ParsedCommand, _: &JournalIdentity) -> Result<Vec<Guard>, ServiceError> {
        Ok(vec![])
    }
    async fn source(&self, _: &SourceRequest) -> Result<VerifiedSource, ServiceError> {
        Err(ServiceError::Unavailable)
    }
    async fn fresh_base(
        &self,
        _: &mut SqliteTx,
        _: &wire::Enroll,
        _: &LockedInputs,
    ) -> Result<FreshBaseAcceptance, ServiceError> {
        Err(ServiceError::IntegrityFailure)
    }
}

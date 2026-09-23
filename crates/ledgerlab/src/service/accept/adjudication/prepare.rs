//! Construct one complete validated plan from bounded locked point observations.
use super::*;
use ledgerlab_core::adjudication::{
    self as r3,
    runtime::{self as rt, points as p, transition as tr},
    Validate,
};
use ledgerlab_core::{Error, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
fn require(ok: bool, code: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error {
            code,
            detail: "R3 planning".into(),
        })
    }
}
fn scalar(n: usize) -> Result<Count> {
    Count::new(n as u128)
}
fn key_kind(k: p::PointKind) -> HeadKind {
    match k {
        p::PointKind::Enrollment => HeadKind::Enrollment,
        p::PointKind::Authority => HeadKind::Authority,
        p::PointKind::Grant => HeadKind::Grant,
        p::PointKind::GrantRegistry => HeadKind::GrantRegistry,
        p::PointKind::Token => HeadKind::Token,
        p::PointKind::Allocation => HeadKind::Allocation,
        p::PointKind::Receipt => HeadKind::Receipt,
        p::PointKind::Round => HeadKind::Round,
        p::PointKind::Gateway => HeadKind::Gateway,
        p::PointKind::Family => HeadKind::Family,
        p::PointKind::Case => HeadKind::Case,
        p::PointKind::Entitlement => HeadKind::Entitlement,
        p::PointKind::Supplier => HeadKind::Supplier,
        p::PointKind::Adjustment => HeadKind::Adjustment,
        p::PointKind::Resource => HeadKind::Resource,
        p::PointKind::Counter => HeadKind::Counter,
        p::PointKind::VerifiedCursor => HeadKind::VerifiedCursor,
        p::PointKind::Delivery => HeadKind::Delivery,
    }
}
fn point_kind(k: HeadKind) -> p::PointKind {
    match k {
        HeadKind::Enrollment => p::PointKind::Enrollment,
        HeadKind::Authority => p::PointKind::Authority,
        HeadKind::Grant => p::PointKind::Grant,
        HeadKind::GrantRegistry => p::PointKind::GrantRegistry,
        HeadKind::Token => p::PointKind::Token,
        HeadKind::Allocation => p::PointKind::Allocation,
        HeadKind::Receipt => p::PointKind::Receipt,
        HeadKind::Round => p::PointKind::Round,
        HeadKind::Gateway => p::PointKind::Gateway,
        HeadKind::Family => p::PointKind::Family,
        HeadKind::Case => p::PointKind::Case,
        HeadKind::Entitlement => p::PointKind::Entitlement,
        HeadKind::Supplier => p::PointKind::Supplier,
        HeadKind::Adjustment => p::PointKind::Adjustment,
        HeadKind::Resource => p::PointKind::Resource,
        HeadKind::Counter => p::PointKind::Counter,
        HeadKind::VerifiedCursor => p::PointKind::VerifiedCursor,
        HeadKind::Delivery => p::PointKind::Delivery,
    }
}
pub(crate) enum Prepared {
    Need(Vec<HeadKey>),
    Plan(Box<ValidatedAdjudicationPlan>),
    NeedEnrollment {
        journal: JournalIdentity,
        registration: Id,
    },
    NeedSealScan {
        round: Count,
        gateway: Id,
        cutoff: Count,
        high: Count,
    },
}
fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        out.push(A[(c[0] >> 2) as usize] as char);
        out.push(A[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char);
        out.push(if c.len() > 1 {
            A[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[(c[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
fn origin(j: &JournalIdentity, n: Count) -> wire::ObjectOrigin {
    wire::ObjectOrigin {
        store: j.store.clone(),
        scope: j.scope.clone(),
        registration: j.registration.clone(),
        host: j.host.clone(),
        ordinal: n,
    }
}
fn object(
    origin: wire::ObjectOrigin,
    kind: wire::FactKind,
    key: &Value,
    body: &Value,
) -> Result<wire::RetainedObject> {
    let raw = r3::canonical_bytes(body, 262144)?;
    let full_key = serde_json::from_value(serde_json::to_value(key).map_err(|_| Error {
        code: "KEY",
        detail: "serialize".into(),
    })?)
    .map_err(|_| Error {
        code: "KEY",
        detail: "full identity".into(),
    })?;
    Ok(wire::RetainedObject {
        origin,
        kind,
        full_key,
        body: b64(&raw),
        body_hash: r3::raw_sha256(&raw),
        bytes: scalar(raw.len())?,
    })
}
fn object_identity(o: &wire::RetainedObject) -> Result<Vec<u8>> {
    r3::canonical_bytes(
        &json!([o.origin, o.kind, o.full_key, o.body_hash, o.bytes]),
        r3::COMMAND_BYTES,
    )
}
fn head(j: &JournalIdentity, p: p::Point) -> HeadKey {
    HeadKey {
        journal: j.clone(),
        kind: key_kind(p.kind),
        full_key: p.key,
    }
}
pub(super) fn parsed_state(raw: &[u8]) -> Result<p::State> {
    let v = ledgerlab_core::canonical::parse_bounded(raw, r3::COMMAND_BYTES)?;
    require(
        r3::canonical_bytes(&v, r3::COMMAND_BYTES)? == raw,
        "STATE_CANONICAL",
    )?;
    serde_json::from_value(v).map_err(|e| Error {
        code: "STATE",
        detail: e.to_string(),
    })
}

pub(crate) fn prepare_locked<A: AdjudicationAuthority>(
    command: &ParsedCommand,
    inputs: &LockedInputs,
    capability: &CommitCapability,
    authority: &A,
    base: Option<&FreshBaseAcceptance>,
    guards: &[Guard],
) -> Result<Prepared> {
    prepare_with_seal(command, inputs, capability, authority, base, guards, None)
}
pub(super) fn prepare_with_seal<A: AdjudicationAuthority>(
    command: &ParsedCommand,
    inputs: &LockedInputs,
    capability: &CommitCapability,
    authority: &A,
    base: Option<&FreshBaseAcceptance>,
    guards: &[Guard],
    seal: Option<&super::seal::VerifiedSealScan>,
) -> Result<Prepared> {
    if let Some(scan) = seal {
        scan.check(capability)?;
    }
    require(
        inputs.journal == *capability.journal()
            && inputs.journal == *inputs.prefix.journal()
            && capability.recovered_through().root() == inputs.prefix.root(),
        "CAPABILITY_BINDING",
    )?;
    let cv = rt::command_value(command.command())?;
    let kind = cv["kind"].as_str().ok_or_else(|| Error {
        code: "COMMAND",
        detail: "kind".into(),
    })?;
    require(
        cv["key"][0]
            == serde_json::to_value(&inputs.journal.scope).map_err(|_| Error {
                code: "SCOPE",
                detail: "scope".into(),
            })?,
        "SCOPE",
    )?;
    require(
        cv["authority"]["head"] == inputs.prefix.root().as_str(),
        "AUTH_HEAD",
    )?;
    let observations: Vec<_> = inputs
        .heads
        .iter()
        .map(|h| {
            require(h.key.journal == inputs.journal, "HEAD_HOST")?;
            require(h.revision.is_some() == h.value.is_some(), "HEAD_ABSENCE")?;
            Ok(p::Observation {
                point: p::Point {
                    kind: point_kind(h.key.kind),
                    key: h.key.full_key.clone(),
                },
                revision: h.revision,
                state: h.value.as_deref().map(parsed_state).transpose()?,
            })
        })
        .collect::<Result<_>>()?;
    let target = if kind == "ENROLL" {
        cv["payload"]["target"]
            .as_str()
            .map(Id::parse)
            .transpose()?
    } else {
        observations.iter().find_map(|o| match &o.state {
            Some(p::State::Enrollment(e)) => Some(e.terms.target.clone()),
            _ => None,
        })
    };
    let target = target.or_else(|| {
        inputs
            .sources
            .iter()
            .find(|s| s.object().kind == wire::FactKind::Enrollment)
            .and_then(|s| {
                let bytes = r3::proofs::VerifiedObjectBytes::check(s.object().clone()).ok()?;
                let v = ledgerlab_core::canonical::parse_bounded(bytes.bytes(), r3::COMMAND_BYTES)
                    .ok()?;
                Id::parse(v["payload"]["target"].as_str()?).ok()
            })
    });
    let auth = authority
        .current(command, inputs, AuthorityAccess::NewTransition)
        .map_err(|e| Error {
            code: "AUTH_HOST",
            detail: e.to_string(),
        })?;
    let mut authority_needs = Vec::new();
    for observed in &auth.current_heads {
        require(observed.key.journal == inputs.journal, "AUTHORITY_HOST")?;
        if let Some(locked) = inputs.heads.iter().find(|h| h.key == observed.key) {
            require(
                locked.revision == observed.revision && locked.value == observed.value,
                "AUTH_CURRENT_HEAD",
            )?;
        } else {
            authority_needs.push(observed.key.clone());
        }
    }
    if !authority_needs.is_empty() {
        return Ok(Prepared::Need(authority_needs));
    }
    let sources = authority::Sources::new(&auth.exact_sources)?;
    sources.current(
        command,
        &auth,
        target.as_ref(),
        AuthorityAccess::NewTransition,
    )?;
    sources.enrollment(command)?;
    let used = sources.used.borrow();
    require(
        used.len() <= if kind == "ENROLL" { 83 } else { 1 },
        "AUTH_SOURCE_COUNT",
    )?;
    let next = inputs.prefix.ordinal().checked_add(Count::new(1)?)?;
    let own_origin = origin(&inputs.journal, next);
    let mut objects = Vec::new();
    let mut dependencies = BTreeSet::new();
    let mut authority_writes = Vec::new();
    let mut needs = Vec::new();
    // Exact source identity precedes hash deduplication. Reuse names first local
    // introduction as a dependency and rejects coherently replaced same identity.
    for (digest, (row, body)) in &sources.records {
        if !used.contains(digest) {
            continue;
        }
        let key = json!([body["source"], body["id"], body["revision"]]);
        let components = [
            body["source"].as_str().unwrap_or(""),
            body["id"].as_str().unwrap_or(""),
            body["revision"].as_str().unwrap_or(""),
        ];
        let point = p::Point {
            kind: p::PointKind::Authority,
            key: rt::index_key(*b"AUTHDOC_", &components.map(str::as_bytes))?,
        };
        let Some(observed) = observations.iter().find(|o| o.point == point) else {
            needs.push(head(&inputs.journal, point));
            continue;
        };
        match &observed.state {
            Some(p::State::Authority(old)) => {
                require(
                    old.source.body_hash.as_str() == digest
                        && old.source.bytes == row.bytes
                        && old.source.body == row.body,
                    "AUTH_SOURCE_IDENTITY_CONFLICT",
                )?;
                dependencies.insert(old.segment.as_str().to_owned());
            }
            None => {
                let value = object(own_origin.clone(), wire::FactKind::Authority, &key, body)?;
                objects.push(value);
                authority_writes.push((
                    point,
                    p::AuthorityState {
                        source: row.clone(),
                        origin: own_origin.clone(),
                        segment: Digest::parse(&"0".repeat(64))?,
                    },
                ));
            }
            _ => {
                return Err(Error {
                    code: "AUTHORITY_HEAD",
                    detail: "wrong typed head".into(),
                })
            }
        }
    }
    if !needs.is_empty() {
        return Ok(Prepared::Need(needs));
    }
    for source in &inputs.sources {
        objects.push(source.object().clone());
        dependencies.insert(source.proof().segment.as_str().to_owned());
    }
    if kind == "ENROLL" {
        let b = base.ok_or_else(|| Error {
            code: "ORIGINAL_BASE",
            detail: "fresh atomic plan required".into(),
        })?;
        require(
            cv["payload"]["base_receipt"] == b.receipt().as_str()
                && cv["payload"]["base_manifest"] == b.manifest().as_str(),
            "ORIGINAL_BASE_BINDING",
        )?;
        require(
            b.exact_members().iter().all(|o| o.origin == own_origin),
            "ORIGINAL_ORIGIN",
        )?;
        objects.extend_from_slice(b.exact_members());
    }
    let known: BTreeSet<_> = inputs
        .retained
        .iter()
        .map(object_identity)
        .collect::<Result<_>>()?;
    let mut identities = BTreeSet::new();
    objects
        .retain(|o| object_identity(o).is_ok_and(|k| !known.contains(&k) && identities.insert(k)));
    let own_fact = matches!(
        kind,
        "PREPARE_ENROLL"
            | "ENROLL"
            | "LOCAL_GRANT"
            | "ISSUE"
            | "RECEIVE"
            | "RETURN_UNUSED"
            | "RECONCILE"
            | "BEGIN"
            | "SEALED"
            | "CLOSE"
            | "INSTALL"
    );
    let source_facts: Vec<_> = inputs
        .sources
        .iter()
        .map(|s| tr::SourceFact {
            proof: s.proof().clone(),
            object: s.object().clone(),
        })
        .collect();
    if kind == "ENROLL" {
        for preparation in &inputs.sources {
            if preparation.proof().fact_kind == wire::FactKind::EnrollPreparation {
                require(
                    preparation.authorizing_target() == target.as_ref(),
                    "PREPARATION_AUTH_TARGET",
                )?;
            }
        }
    }
    let transition = tr::step(&tr::Input {
        command: command.command(),
        store: &inputs.journal.store,
        registration: &inputs.journal.registration,
        host: &inputs.journal.host,
        prior_root: inputs.prefix.root(),
        next_ordinal: next,
        writer_epoch: capability.epoch(),
        observations: &observations,
        sources: &source_facts,
        introduced_objects: objects.len() + usize::from(own_fact),
        seal: seal.map(|s| s.value()),
    });
    let transition = match transition {
        Ok(x) => x,
        Err(e) if e.code == "NEED_ENROLLMENT" => {
            return Ok(Prepared::NeedEnrollment {
                journal: JournalIdentity {
                    host: inputs.journal.store.clone(),
                    ..inputs.journal.clone()
                },
                registration: inputs.journal.registration.clone(),
            })
        }
        Err(e) if e.code == "NEED_SEAL_SCAN" => {
            let n = Count::parse(cv["payload"]["round"].as_str().expect("validated round"))?;
            let gateway = inputs.journal.host.clone();
            let r = observations
                .iter()
                .find_map(|o| match &o.state {
                    Some(p::State::Round(r)) if r.begin.round == n => Some(r),
                    _ => None,
                })
                .ok_or_else(|| Error {
                    code: "ROUND",
                    detail: "scan binding".into(),
                })?;
            let cutoff = r
                .gateways
                .iter()
                .find(|g| g.gateway == gateway)
                .ok_or_else(|| Error {
                    code: "GATEWAY",
                    detail: "scan binding".into(),
                })?
                .cutoff;
            let high = observations
                .iter()
                .find_map(|o| match &o.state {
                    Some(p::State::Gateway(g)) if g.namespace.gateway == gateway => Some(g.receipt),
                    _ => None,
                })
                .ok_or_else(|| Error {
                    code: "GATEWAY",
                    detail: "scan high water".into(),
                })?;
            return Ok(Prepared::NeedSealScan {
                round: n,
                gateway,
                cutoff,
                high,
            });
        }
        Err(e) => return Err(e),
    };
    let delta = match transition {
        tr::Step::Need(points) => {
            return Ok(Prepared::Need(
                points
                    .into_iter()
                    .map(|p| head(&inputs.journal, p))
                    .collect(),
            ))
        }
        tr::Step::Ready(d) => d,
    };
    if let Some((fact, key)) = &delta.fact {
        objects.push(object(
            own_origin.clone(),
            fact.clone(),
            &json!(key),
            &json!({"payload":cv["payload"],"effects":delta.effects}),
        )?);
    }
    objects.sort_by_cached_key(|o| {
        r3::canonical_bytes(o, r3::SEGMENT_BYTES).expect("bounded checked object")
    });
    let root = rt::hash(
        "replay",
        &json!([
            inputs.prefix.root(),
            rt::command_digest(command.command())?,
            delta.effects
        ]),
    )?;
    let result = wire::CommandResult {
        status: wire::CommandResultStatus::Committed,
        code: kind.into(),
        effects: delta.effects.clone(),
        root,
    };
    let segment = wire::Segment {
        host: inputs.journal.host.clone(),
        profile: wire::SegmentProfile::CentralAdjudicationR31,
        ordinal: next,
        previous: inputs.prefix.segment().clone(),
        previous_root: inputs.prefix.root().clone(),
        command: command.command().clone(),
        result,
        dependencies: dependencies
            .iter()
            .map(|s| Digest::parse(s))
            .collect::<Result<_>>()?,
        objects,
    };
    segment.validate()?;
    let exact_segment = r3::canonical_bytes(&segment, r3::SEGMENT_BYTES)?;
    let mut seen = BTreeSet::new();
    let trusted = segment
        .objects
        .iter()
        .filter(|o| seen.insert(o.body_hash.as_str()))
        .try_fold(0usize, |sum, o| {
            usize::try_from(o.bytes.value())
                .ok()
                .and_then(|n| sum.checked_add(n))
                .ok_or_else(|| Error {
                    code: "TRUST_BYTES",
                    detail: "overflow".into(),
                })
        })?;
    require(
        command.bytes().len()
            + r3::canonical_bytes(&segment.result, r3::SEGMENT_BYTES)?.len()
            + trusted
            <= r3::INTRODUCED_TRUST_BYTES,
        "TRUST_BYTES",
    )?;
    let work = rt::accounting::Worksheet::frozen()?;
    let template = work.template(kind)?;
    require(
        exact_segment.len() as u64 <= template.segment_bytes
            && command.bytes().len()
                + r3::canonical_bytes(&segment.result, r3::SEGMENT_BYTES)?.len()
                + trusted
                <= template.new_trusted_bytes as usize
            && segment.objects.len() + 1 <= template.records as usize,
        "PAID_ENVELOPE",
    )?;
    require(kind == "ENROLL" || base.is_none(), "UNEXPECTED_BASE")?;
    let before = observations
        .iter()
        .find_map(|o| match &o.state {
            Some(p::State::Resource(r)) => Some(r),
            _ => None,
        })
        .ok_or_else(|| Error {
            code: "RESOURCE_HOST",
            detail: "before account".into(),
        })?;
    let after = delta
        .mutations
        .iter()
        .find_map(|m| match &m.state {
            p::State::Resource(r) => Some(r),
            _ => None,
        })
        .ok_or_else(|| Error {
            code: "RESOURCE_HOST",
            detail: "after account".into(),
        })?;
    let actual = after.used.checked_sub(&before.used)?;
    let counter_actual = after.q.checked_sub(&before.q)?;
    let conversion = ReservationConversion {
        owner: Id::parse(&delta.funding.owner)?,
        slot: Id::parse(kind)?,
        discharged: actual.clone(),
        actual,
        counter_reserved: template.counters()?,
        counter_actual,
    };
    let closure = segment.result.effects.iter().find_map(|e| match e {
        wire::Effect::Closure { body } => Some(body.clone()),
        _ => None,
    });
    let mut writes = delta
        .mutations
        .iter()
        .map(|m| {
            Ok(HeadWrite {
                key: head(&inputs.journal, m.point.clone()),
                expected: m.prior,
                revision: m.revision,
                value: r3::canonical_bytes(&m.state, r3::COMMAND_BYTES)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let segment_id = rt::hash("segment", &segment)?;
    for (point, mut state) in authority_writes {
        state.segment = segment_id.clone();
        writes.push(HeadWrite {
            key: head(&inputs.journal, point),
            expected: None,
            revision: Count::new(1)?,
            value: r3::canonical_bytes(&p::State::Authority(Box::new(state)), r3::COMMAND_BYTES)?,
        });
    }
    Ok(Prepared::Plan(Box::new(ValidatedAdjudicationPlan {
        journal: inputs.journal.clone(),
        command: command.clone(),
        prior: inputs.prefix.clone(),
        segment,
        exact_segment,
        sources: inputs.sources.clone(),
        guards: guards.to_vec(),
        observed: inputs.heads.clone(),
        writes,
        resources: vec![conversion],
        indices: vec![],
        base: base.cloned().map(Box::new),
        closure,
        held_intentions: vec![],
    })))
}

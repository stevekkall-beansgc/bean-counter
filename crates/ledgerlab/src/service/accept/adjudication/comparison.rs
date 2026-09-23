//! Detached, incremental comparison of an authoritative central history.
//! Each call owns only a bounded SELECT lease; no acceptance/write port is held.
use super::*;
use crate::{
    service::{retained::base as b, store_error},
    store::sqlite::SqliteStore,
    ServiceError,
};
use ledgerlab_core::adjudication::{
    self as r3,
    runtime::{self as rt, points as p, transition as tr},
    types::Atoms,
    Validate,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::Instant;
// Maximum admitted segment in the frozen R3 worksheet, enforced before growth.
const ADMITTED_SEGMENT_BYTES: usize = 3_411_620;
type Result<T> = std::result::Result<T, ServiceError>;
fn core<T>(r: ledgerlab_core::Result<T>) -> Result<T> {
    r.map_err(|e| ServiceError::Rejection(e.code.into()))
}
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ServiceError::IntegrityFailure)
    }
}
fn zero() -> wire::ReadBudget {
    wire::ReadBudget {
        bytes: Count::ZERO,
        pages: Count::ZERO,
        segments: Count::ZERO,
    }
}
fn bounded(b: &wire::ReadBudget) -> wire::ReadBudget {
    let max = maximum_budget();
    wire::ReadBudget {
        bytes: b.bytes.min(max.bytes),
        pages: b.pages.min(max.pages),
        segments: b.segments.min(max.segments),
    }
}
fn journal(e: &wire::ExpectedPrefix) -> JournalIdentity {
    JournalIdentity {
        store: e.store.clone(),
        scope: e.scope.clone(),
        registration: e.registration.clone(),
        host: e.host.clone(),
    }
}
fn head(j: &JournalIdentity, point: &p::Point) -> HeadKey {
    HeadKey {
        journal: j.clone(),
        kind: super::prepare::key_kind(point.kind),
        full_key: point.key.clone(),
    }
}
fn maximum_budget() -> wire::ReadBudget {
    wire::ReadBudget {
        bytes: Count::new(16 * 1024 * 1024).expect("limit"),
        pages: Count::new(1 << 24).expect("limit"),
        segments: Count::new(4096).expect("limit"),
    }
}

/// Nonposting report continuation. It cannot be serialized, cloned or supplied
/// as an acceptance plan. Dropping it leaves no authoritative comparison state.
pub struct SqliteComparison<'a> {
    store: &'a SqliteStore,
    expected: wire::ExpectedPrefix,
    policy: wire::ComparisonPolicy,
    coverage: Vec<wire::Coverage>,
    progress: Progress,
    initialization: wire::ReadBudget,
    partial: Partial,
    cursor: Option<wire::ReadCursor>,
    session: Digest,
    finished: bool,
    // Last field: buffers/progress are dropped before releasing admission.
    _workspace: tokio::sync::OwnedSemaphorePermit,
}
#[derive(Clone)]
struct Progress {
    next: Count,
    previous: Digest,
    root: Digest,
    enrollment: Option<p::EnrollmentState>,
    resolution: Option<wire::Family>,
    actual: i128,
    alternative: i128,
    supplier: i128,
    resolution_used: bool,
    witnessed_coverage: Vec<wire::Coverage>,
    failure: Option<(wire::Case, String)>,
    unsupported: Option<String>,
}
#[derive(Clone, Default)]
struct Partial {
    hash: Option<Digest>,
    total: Option<usize>,
    bytes: Vec<u8>,
}
impl<'a> SqliteComparison<'a> {
    /// Work charged during explicit trusted session preparation. Report responses
    /// separately meter each advance; preparation is never hidden in a tiny call.
    pub fn preparation_measured(&self) -> &wire::ReadBudget {
        &self.initialization
    }
    pub(crate) async fn from_store(
        store: &'a SqliteStore,
        request: &wire::ComparisonRequest,
        preparation_budget: &wire::ReadBudget,
    ) -> Result<Self> {
        core(request.validate())?;
        require(request.cursor.is_none() && request.expected.host == request.expected.store)?;
        let workspace = store.reserve_comparison().map_err(store_error)?;
        let mut read = store
            .begin_adjudication_read(
                &SnapshotSelection {
                    journal: journal(&request.expected),
                    historical: Some(request.expected.clone()),
                },
                &bounded(preparation_budget),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .map_err(store_error)?;
        require(
            read.expected_prefix().expected() == &request.expected
                && read.lease().retained_bytes == Count::ZERO,
        )?;
        let initialized = read.measured().await.map_err(store_error)?;
        read.finish().await.map_err(store_error)?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let serial = NEXT
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |n| n.checked_add(1),
            )
            .map_err(|_| ServiceError::Unavailable)?;
        let session = core(rt::hash(
            "replay",
            &json!([
                "comparison-session",
                std::process::id(),
                serial,
                request.expected,
                request.policy,
                request.coverage
            ]),
        ))?;
        Ok(Self {
            store,
            _workspace: workspace,
            expected: request.expected.clone(),
            policy: request.policy.clone(),
            coverage: request.coverage.clone(),
            progress: Progress {
                next: core(Count::new(1))?,
                previous: core(Digest::parse(&"0".repeat(64)))?,
                root: core(Digest::parse(&"0".repeat(64)))?,
                enrollment: None,
                resolution: None,
                actual: 0,
                alternative: 0,
                supplier: 0,
                resolution_used: false,
                witnessed_coverage: vec![],
                failure: None,
                unsupported: None,
            },
            initialization: initialized,
            partial: Partial::default(),
            cursor: None,
            session,
            finished: false,
        })
    }
    /// Evaluate at most one central segment. Budget is per call, not a lifetime
    /// history ceiling. Resume requires the exact last cursor and same policy/scope.
    pub async fn advance(
        &mut self,
        request: &wire::ComparisonRequest,
    ) -> Result<wire::ComparisonResponse> {
        core(request.validate())?;
        require(
            !self.finished
                && request.expected == self.expected
                && request.policy == self.policy
                && request.coverage == self.coverage
                && request.cursor == self.cursor,
        )?;
        let budget = bounded(&request.budget);
        // Snapshot membership/enrollment is immutable at this selected prefix.
        // A call too small to pay the known initialization performs no I/O.
        if budget.bytes < self.initialization.bytes
            || budget.pages < self.initialization.pages
            || budget.segments == Count::ZERO
        {
            return self.incomplete(zero());
        }
        let mut read = self
            .store
            .begin_adjudication_read(
                &SnapshotSelection {
                    journal: journal(&self.expected),
                    historical: Some(self.expected.clone()),
                },
                &budget,
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .map_err(store_error)?;
        let mut progress = self.progress.clone();
        let mut partial = self.partial.clone();
        let result = async {
            if partial.hash.is_none() {
                partial.hash = Some(
                    read.segment_hash(progress.next)
                        .await
                        .map_err(store_error)?,
                );
            }
            let hash = partial
                .hash
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            while partial.total != Some(partial.bytes.len()) {
                let used = read.measured().await.map_err(store_error)?;
                let left = budget.bytes.value().saturating_sub(used.bytes.value());
                if left <= 64 || budget.pages.value().saturating_sub(used.pages.value()) < 3 {
                    return Err(ServiceError::ReadBudgetExhausted);
                }
                let offset = partial.bytes.len() % 4096;
                let max = (4096 - offset).min((left - 64) as usize);
                let page = read
                    .segment_page(&IndexedPageRequest {
                        address: r3::reads::PageAddress {
                            segment: hash.clone(),
                            page: core(Count::new((partial.bytes.len() / 4096) as u128))?,
                        },
                        offset: offset as u16,
                        max_bytes: max as u16,
                    })
                    .await
                    .map_err(store_error)?;
                core(page.validate())?;
                require(
                    page.total_bytes.value() <= ADMITTED_SEGMENT_BYTES as u128
                        && !page.bytes.is_empty()
                        && partial.bytes.len() + page.bytes.len()
                            <= page.total_bytes.value() as usize,
                )?;
                require(
                    partial
                        .total
                        .is_none_or(|n| n == page.total_bytes.value() as usize),
                )?;
                partial.total = Some(page.total_bytes.value() as usize);
                partial.bytes.extend(page.bytes);
            }
            let segment: wire::Segment = core(r3::parse_exact(&partial.bytes, r3::SEGMENT_BYTES))?;
            core(segment.validate())?;
            require(core(rt::hash("segment", &segment))? == *hash)?;
            Self::read_step(
                &mut progress,
                &self.expected,
                &self.policy,
                &self.coverage,
                &mut read,
                segment,
                hash.clone(),
            )
            .await
        }
        .await;
        let charged = read.measured().await.map_err(store_error);
        let cleanup = read.finish().await.map_err(store_error);
        let measured = charged?;
        cleanup?;
        match result {
            Ok(()) => {
                self.progress = progress;
                self.partial = Partial::default();
            }
            Err(ServiceError::ReadBudgetExhausted) => {
                self.partial = partial;
                return self.incomplete(measured);
            }
            Err(e) => return Err(e),
        }
        if self.progress.next > self.expected.ordinal {
            self.finished = true; // Terminal integrity/coverage errors also close this session.
            require(
                self.progress.previous == self.expected.segment
                    && self.progress.root == self.expected.root
                    && self.progress.enrollment.is_some(),
            )?;
            self.check_coverage()?;
            self.finished = true;
            if let Some((at_case, reason)) = &self.progress.failure {
                return Ok(wire::ComparisonResponse::PolicyFailure {
                    at_case: at_case.clone(),
                    reason: reason.clone(),
                    measured: measured.clone(),
                });
            }
            if let Some(reason) = &self.progress.unsupported {
                return Ok(wire::ComparisonResponse::Unsupported {
                    reason: reason.clone(),
                    measured: measured.clone(),
                });
            }
            return Ok(wire::ComparisonResponse::Comparable {
                expected: self.expected.clone(),
                actual: core(Atoms::new(self.progress.actual))?,
                alternative: core(Atoms::new(self.progress.alternative))?,
                difference: core(Atoms::new(
                    self.progress
                        .alternative
                        .checked_sub(self.progress.actual)
                        .ok_or(ServiceError::IntegrityFailure)?,
                ))?,
                supplier_booked: core(Count::new(
                    u128::try_from(self.progress.supplier)
                        .map_err(|_| ServiceError::IntegrityFailure)?,
                ))?,
                coverage: self.coverage.clone(),
                measured: measured.clone(),
            });
        }
        self.incomplete(measured)
    }
    fn incomplete(&mut self, measured: wire::ReadBudget) -> Result<wire::ComparisonResponse> {
        let continuation = core(rt::hash(
            "replay",
            &json!([
                self.session,
                self.progress.next,
                self.progress.root,
                self.partial.bytes.len(),
                r3::raw_sha256(&self.partial.bytes)
            ]),
        ))?;
        let cursor = wire::ReadCursor {
            expected: self.expected.clone(),
            ordinal: core(self.progress.next.checked_sub(Count::new(1).expect("one")))?,
            byte_offset: core(Count::new(self.partial.bytes.len() as u128))?,
            verified_root: self.progress.root.clone(),
            continuation,
        };
        self.cursor = Some(cursor.clone());
        Ok(wire::ComparisonResponse::Incomplete {expected:self.expected.clone(),reason:"BOUNDED_STEP; nonposting central prefix; offline work outside stated cutoffs remains unknown".into(),cursor:Box::new(cursor),measured})
    }
    async fn read_step<T: AdjudicationReadTx>(
        progress: &mut Progress,
        expected: &wire::ExpectedPrefix,
        policy: &wire::ComparisonPolicy,
        coverage: &[wire::Coverage],
        read: &mut T,
        segment: wire::Segment,
        hash: Digest,
    ) -> Result<()> {
        require(
            segment.host == expected.host
                && segment.ordinal == progress.next
                && segment.previous == progress.previous
                && segment.previous_root == progress.root
                && segment.result.status == wire::CommandResultStatus::Committed,
        )?;
        let value = core(rt::command_value(&segment.command))?;
        require(
            segment.result.code == value["kind"]
                && value["authority"]["head"] == progress.root.as_str(),
        )?;
        require(
            segment.result.root
                == core(rt::hash(
                    "replay",
                    &json!([
                        progress.root,
                        core(rt::command_digest(&segment.command))?,
                        segment.result.effects
                    ]),
                ))?,
        )?;
        // Every retained source, evidence and copied fact in this chronology is
        // read under its complete identity and verified, including non-economic steps.
        for object in &segment.objects {
            verify_object(read, object).await?;
        }
        let parsed = core(ParsedCommand::parse(&core(r3::canonical_bytes(
            &segment.command,
            r3::COMMAND_BYTES,
        ))?))?;
        let sources = read_authority(read, &value, progress.next).await?;
        let set = core(authority::Sources::new(&sources))?;
        let a: wire::Authority = serde_json::from_value(value["authority"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let observation = AuthorityObservation {
            principal: a.principal,
            command: a.command,
            document: a.document,
            revision: a.revision,
            observed_at: a.observed_at,
            permission: a.permission,
            exact_sources: sources,
            current_heads: vec![],
        };
        core(set.current(
            &parsed,
            &observation,
            Some(&expected.target),
            AuthorityAccess::NewTransition,
        ))?;
        if let wire::Command::Enroll { payload, .. } = &segment.command {
            require(
                progress.enrollment.is_none()
                    && progress.next.value() == 1
                    && payload.target == expected.target
                    && core(rt::hash("enrollment", payload))? == expected.enrollment,
            )?;
            core(set.enrollment(&parsed))?;
            verify_base(&segment.objects, payload, segment.ordinal)?;
            let resolutions: Vec<_> = payload
                .families
                .iter()
                .filter(|f| {
                    f.key.2.as_str() == "resolution" && f.book == wire::FamilyTermsBook::Retail
                })
                .collect();
            if resolutions.len() != 1 {
                progress.unsupported = Some(
                    "The amount candidate requires one retained retail resolution family".into(),
                );
            } else {
                progress.resolution = Some(resolutions[0].key.clone());
            }
            require(coverage.len() == payload.gateways.len())?;
            let mut gateways = BTreeSet::new();
            for entry in coverage {
                let g = coverage_gateway(entry);
                require(
                    gateways.insert(g.as_str())
                        && payload.gateways.iter().any(|item| item.gateway == *g),
                )?;
            }
            progress.actual = i128::try_from(payload.base_atoms.value())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            progress.alternative = progress.actual;
            progress.supplier = i128::try_from(payload.supplier_booked.value())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            progress.enrollment = Some(p::EnrollmentState {
                terms: payload.clone(),
                enrollment: expected.enrollment.clone(),
                active_round: None,
                last_round: Count::ZERO,
            });
        } else {
            require(progress.enrollment.is_some())?;
        }
        if matches!(
            segment.command,
            wire::Command::Decide { .. } | wire::Command::Correct { .. }
        ) {
            let enrollment = progress
                .enrollment
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            core(set.economics(&parsed, &enrollment.terms))?;
            let mut observations = vec![];
            let states = loop {
                match core(tr::replay_economics(
                    &segment.command,
                    &expected.host,
                    enrollment,
                    &observations,
                ))? {
                    tr::EconomicReplay::Need(points) => {
                        require(observations.len() + points.len() <= 34)?;
                        for point in points {
                            require(!observations.iter().any(|(p, _)| p == &point))?;
                            let state = read
                                .historical_state(
                                    &head(&journal(expected), &point),
                                    core(progress.next.checked_sub(Count::new(1).expect("one")))?,
                                )
                                .await
                                .map_err(store_error)?
                                .ok_or(ServiceError::IntegrityFailure)?;
                            observations.push((point, state));
                        }
                    }
                    tr::EconomicReplay::Checked { states, effects } => {
                        require(effects == segment.result.effects)?;
                        break states;
                    }
                }
            };
            for (point, state) in states {
                require(
                    read.historical_state(&head(&journal(expected), &point), progress.next)
                        .await
                        .map_err(store_error)?
                        == Some(state),
                )?;
            }
            if progress.failure.is_none() && progress.unsupported.is_none() {
                let mut candidate = segment.command.clone();
                let mut alternative_enrollment = enrollment.clone();
                let resolution = progress
                    .resolution
                    .as_ref()
                    .ok_or(ServiceError::IntegrityFailure)?;
                let original = alternative_enrollment
                    .terms
                    .families
                    .iter()
                    .find(|f| &f.key == resolution)
                    .ok_or(ServiceError::IntegrityFailure)?
                    .ordinary_atoms;
                let changed = core(Atoms::new(
                    i128::try_from(policy.resolution_atoms.value())
                        .map_err(|_| ServiceError::IntegrityFailure)?,
                ))?;
                let mut substituted = false;
                if let wire::Command::Decide { payload, .. } = &mut candidate {
                    if payload.case.0 == *resolution
                        && payload.path == wire::DecidePath::Ordinary
                        && payload.verdict == wire::DecideVerdict::Allow
                    {
                        payload.signed_atoms = changed;
                        substituted = true;
                    }
                }
                if matches!(&candidate,wire::Command::Correct{payload,..} if payload.case.0==*resolution)
                    && changed != original
                {
                    progress.unsupported=Some("Changing a corrected resolution award would change retained correction terms".into());
                }
                for f in &mut alternative_enrollment.terms.families {
                    if f.key == *resolution {
                        f.ordinary_atoms = changed
                    }
                }
                for (_, state) in &mut observations {
                    if let p::State::Family(f) = state {
                        if f.terms.key == *resolution {
                            f.terms.ordinary_atoms = changed;
                            if progress.resolution_used {
                                f.ordinary_positive = policy.resolution_atoms;
                            }
                        }
                    }
                }
                if progress.unsupported.is_none() {
                    match tr::replay_economics(
                        &candidate,
                        &expected.host,
                        &alternative_enrollment,
                        &observations,
                    ) {
                        Ok(tr::EconomicReplay::Checked { .. }) => {}
                        Ok(tr::EconomicReplay::Need(_)) => {
                            return Err(ServiceError::IntegrityFailure)
                        }
                        Err(e) => {
                            progress.failure = Some((
                                serde_json::from_value(value["payload"]["case"].clone())
                                    .map_err(|_| ServiceError::IntegrityFailure)?,
                                format!("{} at central ordinal {}", e.code, progress.next.value()),
                            ));
                        }
                    }
                }
                if substituted && progress.failure.is_none() && progress.unsupported.is_none() {
                    require(!progress.resolution_used)?;
                    progress.resolution_used = true;
                    progress.alternative = progress
                        .alternative
                        .checked_add(changed.value() - original.value())
                        .ok_or(ServiceError::IntegrityFailure)?;
                }
            }
        }
        for effect in &segment.result.effects {
            match effect {
                wire::Effect::Action { body } => {
                    require(matches!(
                        segment.command,
                        wire::Command::Decide { .. } | wire::Command::Correct { .. }
                    ))?;
                    match body.book {
                        wire::ActionBook::Retail => {
                            progress.actual = progress
                                .actual
                                .checked_add(body.signed_atoms.value())
                                .ok_or(ServiceError::IntegrityFailure)?;
                            progress.alternative = progress
                                .alternative
                                .checked_add(body.signed_atoms.value())
                                .ok_or(ServiceError::IntegrityFailure)?;
                        }
                        wire::ActionBook::Supplier => {
                            progress.supplier = progress
                                .supplier
                                .checked_add(body.signed_atoms.value())
                                .ok_or(ServiceError::IntegrityFailure)?
                        }
                    }
                }
                wire::Effect::Closure { body } => {
                    require(
                        body.predecessor == segment.previous_root
                            && body.enrollment == expected.enrollment,
                    )?;
                    require(matches!(segment.command, wire::Command::Close { .. }))?;
                    let cert_point = core(p::Point::id(
                        p::PointKind::Round,
                        *b"CERTIFIC",
                        &body.round.value().to_string(),
                    ))?;
                    require(
                        read.historical_state(
                            &head(&journal(expected), &cert_point),
                            progress.next,
                        )
                        .await
                        .map_err(store_error)?
                            == Some(p::State::Certificate(Box::new(body.clone()))),
                    )?;
                    for family in &body.unavailable {
                        let point = core(p::Point::family(family))?;
                        let Some(p::State::Family(state)) = read
                            .historical_state(&head(&journal(expected), &point), progress.next)
                            .await
                            .map_err(store_error)?
                        else {
                            return Err(ServiceError::IntegrityFailure);
                        };
                        if state.first_closure == Some(core(rt::hash("closure", body))?) {
                            require(
                                read.family_certificate(family, expected)
                                    .await
                                    .map_err(store_error)?
                                    == Some(body.clone()),
                            )?;
                        }
                    }
                    for requested in coverage {
                        if body.cutoffs.contains(requested)
                            && !progress.witnessed_coverage.contains(requested)
                        {
                            progress.witnessed_coverage.push(requested.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        progress.previous = hash;
        progress.root = segment.result.root;
        progress.next = core(progress.next.checked_add(Count::new(1).expect("one")))?;
        Ok(())
    }
    fn check_coverage(&self) -> Result<()> {
        let terms = &self
            .progress
            .enrollment
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?
            .terms;
        require(self.coverage.len() == terms.gateways.len())?;
        let mut seen = BTreeSet::new();
        for coverage in &self.coverage {
            let gateway = coverage_gateway(coverage);
            require(
                seen.insert(gateway.as_str())
                    && terms.gateways.iter().any(|g| g.gateway == *gateway),
            )?;
            match coverage {
                wire::Coverage::UnknownGatewayCoverage { .. } => {}
                wire::Coverage::CompleteGatewayCutoff { .. } => {
                    require(self.progress.witnessed_coverage.contains(coverage))?
                }
                wire::Coverage::Unreconciled { .. } => {
                    return Err(ServiceError::Rejection(
                        "UNSUPPORTED_COVERAGE_OBSERVATION".into(),
                    ))
                }
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn test_partial_total(&self) -> usize {
        self.partial.total.expect("page metadata")
    }
    #[cfg(test)]
    pub(crate) fn test_progress(&self) -> (u128, i128, i128, i128) {
        (
            self.progress.next.value() - 1,
            self.progress.actual,
            self.progress.alternative,
            self.progress.supplier,
        )
    }
}
fn coverage_gateway(c: &wire::Coverage) -> &Id {
    match c {
        wire::Coverage::CompleteGatewayCutoff { gateway, .. }
        | wire::Coverage::Unreconciled { gateway, .. }
        | wire::Coverage::UnknownGatewayCoverage { gateway } => gateway,
    }
}
async fn verify_object<T: AdjudicationReadTx>(
    read: &mut T,
    object: &wire::RetainedObject,
) -> Result<()> {
    let verified = core(r3::proofs::VerifiedObjectBytes::check(object.clone()))?;
    let mut offset = 0;
    while offset < verified.bytes().len() {
        let bytes = read
            .object_page(&ObjectPageRequest {
                origin: object.origin.clone(),
                kind: object.kind.clone(),
                key: core(r3::canonical_bytes(&object.full_key, 4096))?,
                hash: object.body_hash.clone(),
                offset: core(Count::new(offset as u128))?,
                max_bytes: 4096,
            })
            .await
            .map_err(store_error)?;
        require(
            !bytes.is_empty()
                && verified.bytes().get(offset..offset + bytes.len()) == Some(bytes.as_slice()),
        )?;
        offset += bytes.len();
    }
    Ok(())
}
async fn read_authority<T: AdjudicationReadTx>(
    read: &mut T,
    value: &Value,
    at: Count,
) -> Result<Vec<wire::AuthoritySource>> {
    fn references(value: &Value, hashes: &mut BTreeSet<String>) {
        match value {
            Value::Object(m) => {
                for (key, v) in m {
                    if matches!(key.as_str(), "assent" | "payer_delegation") {
                        if let Some(s) = v.as_str() {
                            hashes.insert(s.into());
                        }
                    }
                    references(v, hashes)
                }
            }
            Value::Array(v) => {
                for x in v {
                    references(x, hashes)
                }
            }
            _ => {}
        }
    }
    let mut hashes = BTreeSet::new();
    hashes.insert(
        value["authority"]["document"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .into(),
    );
    references(&value["payload"], &mut hashes);
    require(hashes.len() <= 83)?;
    let mut sources = Vec::new();
    for hash in hashes {
        sources.push(
            read.authority_source(&core(Digest::parse(&hash))?, at)
                .await
                .map_err(store_error)?,
        )
    }
    Ok(sources)
}
fn verify_base(
    objects: &[wire::RetainedObject],
    terms: &wire::Enroll,
    ordinal: Count,
) -> Result<()> {
    require(
        objects
            .iter()
            .filter(|o| o.kind == wire::FactKind::OriginalBase)
            .all(|o| {
                o.origin.store == terms.store
                    && o.origin.host == terms.store
                    && o.origin.scope == terms.scope
                    && o.origin.registration == terms.registration
                    && o.origin.ordinal == ordinal
            }),
    )?;
    let base_objects: Vec<_> = objects
        .iter()
        .filter(|o| o.kind == wire::FactKind::OriginalBase)
        .collect();
    require(!base_objects.is_empty() && base_objects.len() <= 128)?;
    let total = base_objects.iter().try_fold(0u128, |sum, o| {
        require(o.bytes.value() <= r3::COMMAND_BYTES as u128)?;
        sum.checked_add(o.bytes.value())
            .ok_or(ServiceError::IntegrityFailure)
    })?;
    require(total <= 1_048_576)?;
    let raw: Vec<_> = objects
        .iter()
        .filter(|o| o.kind == wire::FactKind::OriginalBase)
        .map(|o| {
            core(r3::proofs::VerifiedObjectBytes::check(o.clone())).map(|v| v.bytes().to_vec())
        })
        .collect::<Result<_>>()?;
    require(!raw.is_empty() && raw.len() <= 128)?;
    let records = b::Records::new(&raw, json!(terms.scope))?;
    let acceptance = records.one("base-acceptance")?;
    let members = acceptance["body"]["members"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    require(members.len() + 1 == raw.len() && members.len() <= 128)?;
    let mut ids = BTreeSet::new();
    for member in members {
        require(
            ids.insert(
                member["id"]
                    .as_str()
                    .ok_or(ServiceError::IntegrityFailure)?,
            ),
        )?;
        records.deref(member)?;
    }
    let base = b::decode_base(&records, &b::reference(acceptance))?;
    require(base.acceptance["body"]["target"] == terms.target.as_str())?;
    require(
        r3::raw_sha256(&core(r3::canonical_bytes(
            &base.snapshot,
            r3::COMMAND_BYTES,
        ))?) == terms.base_manifest,
    )?;
    let mut retail = 0i128;
    let mut supplier = 0i128;
    for row in &records.rows {
        if row["kind"] == "base-posting" {
            let n = row["body"]["amount"]["atoms"]
                .as_str()
                .and_then(|s| s.parse::<i128>().ok())
                .ok_or(ServiceError::IntegrityFailure)?;
            let sum = match row["body"]["book"].as_str() {
                Some("retail") => &mut retail,
                Some("supplier") => &mut supplier,
                _ => return Err(ServiceError::IntegrityFailure),
            };
            *sum = sum.checked_add(n).ok_or(ServiceError::IntegrityFailure)?;
        }
    }
    require(
        retail == terms.base_atoms.value() as i128
            && supplier == terms.supplier_booked.value() as i128
            && core(r3::canonical_bytes(&base.acceptance, r3::COMMAND_BYTES))
                .map(|b| r3::raw_sha256(&b))?
                == terms.base_receipt,
    )
}

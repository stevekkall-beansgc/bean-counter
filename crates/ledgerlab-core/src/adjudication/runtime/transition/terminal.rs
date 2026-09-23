//! Paid reconciliation and bounded-topology finish transitions. Closure updates
//! family heads and one certificate; it never visits or rewrites pending cases.
use super::*;
fn rid(n: Count) -> Result<Id> {
    Id::parse(&n.value().to_string())
}
fn sorted<T: serde::Serialize>(values: &mut [T]) -> Result<()> {
    let mut error = None;
    values.sort_by_cached_key(|v| match r3::canonical_bytes(v, r3::COMMAND_BYTES) {
        Ok(b) => b,
        Err(e) => {
            error = Some(e);
            Vec::new()
        }
    });
    error.map_or(Ok(()), Err)
}
fn state(v: &mut View<'_>, n: Count) -> Result<RoundState> {
    match v.load(&round(n)?)? {
        State::Round(r) => Ok(*r),
        _ => Err(fail("ROUND_UNKNOWN")),
    }
}
fn selected(e: &EnrollmentState, begin: &w::Begin) -> Result<String> {
    let index = e
        .terms
        .families
        .iter()
        .position(|f| begin.families.contains(&f.key))
        .ok_or_else(|| fail("CLOSE_RIGHT"))?;
    owner("close", &index)
}
fn central(v: &mut View<'_>) -> Result<EnrollmentState> {
    let e = v.enrolled()?;
    require(e.terms.store == *v.input.host, "CENTRAL_HOST")?;
    Ok(e)
}
fn begin_source(
    v: &View<'_>,
    proof: &w::Proof,
    n: Count,
    e: &EnrollmentState,
) -> Result<(w::Begin, w::RoundBegin)> {
    require(proof.host == e.terms.store, "PROOF_HOST")?;
    let s = v.source(proof, &[w::FactKind::Begin], &rid(n)?)?;
    let b: w::Begin = s.payload()?;
    let effects = s.effects()?;
    let [w::Effect::RoundBegin { body }] = effects.as_slice() else {
        return Err(fail("BEGIN_PROOF"));
    };
    require(
        b.round == n && body.round == n && b.predecessor == body.predecessor,
        "BEGIN_BINDING",
    )?;
    Ok((b, body.clone()))
}
fn round_from_begin(
    e: &EnrollmentState,
    begin: w::Begin,
    body: w::RoundBegin,
) -> Result<RoundState> {
    require(
        matches!(
            (&begin.mode, &body.mode),
            (w::BeginMode::FinishOnly, w::RoundBeginMode::FinishOnly)
                | (w::BeginMode::Cancellable, w::RoundBeginMode::Cancellable)
        ),
        "ROUND_MODE",
    )?;
    require(
        begin.gateways.len() == body.cutoffs.len()
            && begin
                .gateways
                .iter()
                .all(|g| body.cutoffs.iter().filter(|c| c.gateway == *g).count() == 1),
        "BEGIN_CUTOFFS",
    )?;
    let o = if begin.mode == w::BeginMode::FinishOnly {
        selected(e, &begin)?
    } else {
        owner("optional-round", &begin.round)?
    };
    Ok(RoundState {
        owner: o,
        begin,
        gateways: body
            .cutoffs
            .into_iter()
            .map(|c| RoundGateway {
                gateway: c.gateway,
                cutoff: c.cutoff,
                predecessor: c.gateway_predecessor,
                seal: None,
                observation: None,
                acknowledged: false,
            })
            .collect(),
        status: RoundStatus::Draining,
        closed_at: None,
    })
}
pub(super) fn apply(v: &mut View<'_>, d: &mut Delta, work: &Worksheet) -> Result<usize> {
    let i = v.input;
    match i.command {
        w::Command::ExtendResources { payload: p, .. } => {
            require(p.host == *i.host, "RESOURCE_HOST")?;
            // Admin costs must fit existing admission before the extension.
            v.reserve(&d.funding.owner, &[d.funding.slot.clone()], work)?;
            let point = account(i.host)?;
            let State::Resource(mut a) = v.load(&point)? else {
                return Err(fail("RESOURCE_HOST"));
            };
            let enlarged = a.provisioned.checked_add(&p.resources)?;
            require(
                enlarged
                    .dimensions()
                    .iter()
                    .zip(i.resource_ceiling.dimensions())
                    .all(|(need, backed)| *need <= backed),
                "RESOURCE_BACKING",
            )?;
            a.provisioned = enlarged;
            a.validate()?;
            v.put(point, State::Resource(a))?;
            d.funding.release = true;
            Ok(6)
        }
        w::Command::ReplaceWriter { payload: p, .. } => {
            let mut g = v.gw(i.host)?;
            require(
                p.gateway == *i.host
                    && p.old_epoch == g.epoch
                    && g.epoch == i.writer_epoch
                    && p.new_epoch == g.epoch.checked_add(one())?,
                "WRITER_EPOCH",
            )?;
            require(
                p.journal_head == *i.prior_root && i.writer_fence == Some(&p.fence),
                "FENCE_PROOF",
            )?;
            v.reserve(&d.funding.owner, &[d.funding.slot.clone()], work)?;
            g.epoch = p.new_epoch;
            v.put(gateway(i.host)?, State::Gateway(Box::new(g)))?;
            d.funding.release = true;
            Ok(6)
        }
        w::Command::PrepareRound { payload: p, .. } => {
            let e = v.enrolled()?;
            require(
                p.gateway == *i.host && p.proof.host == e.terms.store,
                "PROOF_HOST",
            )?;
            let retained: w::Enroll = v
                .source(&p.proof, &[w::FactKind::Enrollment], i.registration)?
                .payload()?;
            require(
                retained == e.terms && p.enrollment == e.enrollment,
                "ROUND_ENROLLMENT",
            )?;
            let g = v.gw(i.host)?;
            require(
                g.mode == GatewayMode::Open
                    && g.installed == p.predecessor
                    && p.round > p.predecessor,
                "ROUND_PREPARATION",
            )?;
            let key = Id::parse(hash("namespace", &json!([p.gateway, p.round]))?.as_str())?;
            let point = id(PointKind::Round, *b"RND_PREP", key.as_str())?;
            v.absent(&point)?;
            v.reserve(&d.funding.owner, &[d.funding.slot.clone()], work)?;
            v.reserve(
                &owner("optional-round", &p.round)?,
                work.bundle("cancel_gateway")?,
                work,
            )?;
            d.funding.release = true;
            v.put(point, State::RoundPreparation(p.clone()))?;
            d.fact = Some((w::FactKind::RoundPreparation, key));
            Ok(7)
        }
        w::Command::RetireGrant { payload: p, .. } => {
            central(v)?;
            let mut g = v.gr(&p.grant, true)?;
            require(
                g.status == GrantStatus::RegisteredUnclaimed && g.token.is_none(),
                "GRANT_CLAIMED",
            )?;
            g.status = GrantStatus::RetiredUnclaimed;
            d.funding.owner = owner("grant-central", &p.grant)?;
            d.funding.release = true;
            v.put(grant(&p.grant, true)?, State::Grant(Box::new(g)))?;
            d.fact = Some((w::FactKind::Retirement, p.grant.clone()));
            Ok(5)
        }
        w::Command::Supplement { payload: p, .. } => {
            central(v)?;
            let point = Point::case(&p.case)?;
            let State::Case(mut case) = v.load(&point)? else {
                return Err(fail("CASE"));
            };
            require(
                matches!(
                    case.status,
                    CaseStatus::OrdinaryPending | CaseStatus::AdjustmentPending
                ),
                "CASE_FINAL",
            )?;
            let mut evidence = BTreeMap::new();
            for item in case
                .input
                .evidence
                .iter()
                .chain(&case.supplements)
                .chain(&p.evidence)
            {
                let bytes = r3::proofs::decode_base64(&item.body, 4096)?;
                require(r3::raw_sha256(&bytes) == item.sha256, "EVIDENCE_HASH")?;
                if let Some(old) = evidence.insert(item.sha256.clone(), item.clone()) {
                    require(old == *item, "EVIDENCE_IDENTITY")?;
                }
            }
            require(evidence.len() <= 16, "EVIDENCE_LIMIT")?;
            case.supplements = evidence
                .into_values()
                .filter(|item| {
                    !case
                        .input
                        .evidence
                        .iter()
                        .any(|original| original.sha256 == item.sha256)
                })
                .collect();
            sorted(&mut case.supplements)?;
            v.reserve(&d.funding.owner, &[d.funding.slot.clone()], work)?;
            d.funding.release = true;
            v.put(point, State::Case(case))?;
            Ok(6)
        }
        w::Command::ReturnUnused { payload: p, .. } => {
            let e = v.enrolled()?;
            require(
                p.gateway == *i.host && p.proof.host == e.terms.store,
                "PROOF_HOST",
            )?;
            let issue: w::Issue = v
                .source(&p.proof, &[w::FactKind::Claim], &p.token)?
                .payload()?;
            require(
                issue.token.id == p.token
                    && issue.token.gateway == *i.host
                    && issue.token.claim == p.claim,
                "CLAIM_PROOF",
            )?;
            let mut t = match v.read(&token(&p.token)?)? {
                Some(State::Token(t)) => *t,
                None => new_token(issue.token.clone()),
                _ => return Err(fail("TOKEN_STATE")),
            };
            let mut grant_state = v.gr(&issue.grant, false)?;
            require(
                t.token == issue.token
                    && matches!(t.status, TokenStatus::Issued | TokenStatus::Active)
                    && !grant_state.terminal
                    && grant_state.grant.gateway == *i.host
                    && grant_state.token.as_ref().is_none_or(|t| *t == p.token),
                "TOKEN_STATE",
            )?;
            t.status = TokenStatus::ReturnedUnused;
            grant_state.status = GrantStatus::Claimed;
            grant_state.token = Some(p.token.clone());
            d.funding.owner = owner("grant-local", &issue.grant)?;
            v.put(
                grant(&issue.grant, false)?,
                State::Grant(Box::new(grant_state)),
            )?;
            v.put(
                Point::position(PointKind::Allocation, i.host, t.token.allocation)?,
                State::Position(p.token.clone()),
            )?;
            v.put(token(&p.token)?, State::Token(Box::new(t)))?;
            d.fact = Some((w::FactKind::ReturnedUnused, p.token.clone()));
            Ok(6)
        }
        w::Command::Reconcile { payload: p, .. } => {
            central(v)?;
            let mut t = v.tok(&p.token)?;
            require(
                !t.reconciled && p.proof.host == t.token.gateway,
                "UNRECONCILED",
            )?;
            let source = v.source(
                &p.proof,
                &[
                    w::FactKind::Receipt,
                    w::FactKind::Alias,
                    w::FactKind::ReturnedUnused,
                ],
                &p.token,
            )?;
            if source.object.kind == w::FactKind::ReturnedUnused {
                let returned: w::ReturnUnused = source.payload()?;
                require(
                    returned.token == p.token
                        && returned.gateway == t.token.gateway
                        && returned.claim == t.token.claim
                        && !t.imported
                        && t.status == TokenStatus::Issued,
                    "RETURN_BINDING",
                )?;
                t.status = TokenStatus::ReturnedUnused;
            } else {
                let received: w::Receive = source.payload()?;
                require(
                    t.imported
                        && received.token == p.token
                        && t.submission.as_ref() == Some(&received.submission)
                        && t.delivery.as_ref() == Some(&received.delivery),
                    "UNRECONCILED",
                )?;
            }
            t.reconciled = true;
            d.funding.owner = owner("token", &p.token)?;
            v.put(token(&p.token)?, State::Token(Box::new(t)))?;
            d.fact = Some((w::FactKind::Reconciliation, p.token.clone()));
            Ok(5)
        }
        w::Command::Advance { payload: p, .. } => {
            central(v)?;
            let mut g = v.gw(&p.gateway)?;
            require(
                p.through == g.allocation_prefix.checked_add(one())?,
                "PREFIX_GAP",
            )?;
            let State::Position(tid) = v.load(&Point::position(
                PointKind::Allocation,
                &p.gateway,
                p.through,
            )?)?
            else {
                return Err(fail("ALLOCATION_MEMBERSHIP"));
            };
            let mut t = v.tok(&tid)?;
            require(
                t.token.gateway == p.gateway
                    && t.token.allocation == p.through
                    && t.reconciled
                    && !t.advanced,
                "UNRECONCILED",
            )?;
            t.advanced = true;
            g.allocation_prefix = p.through;
            d.funding.owner = owner("token", &tid)?;
            d.funding.release = t.status != TokenStatus::NewCase || t.receipt_advanced;
            v.put(token(&tid)?, State::Token(Box::new(t)))?;
            v.put(gateway(&p.gateway)?, State::Gateway(Box::new(g)))?;
            Ok(5)
        }
        w::Command::AdvanceReceipt { payload: p, .. } => {
            central(v)?;
            let mut g = v.gw(&p.gateway)?;
            require(
                p.through == g.receipt_prefix.checked_add(one())?,
                "RECEIPT_PREFIX_GAP",
            )?;
            let State::Position(tid) =
                v.load(&Point::position(PointKind::Receipt, &p.gateway, p.through)?)?
            else {
                return Err(fail("RECEIPT_MEMBERSHIP"));
            };
            let mut t = v.tok(&tid)?;
            require(
                t.token.gateway == p.gateway
                    && t.receipt.as_ref().is_some_and(|r| r.position == p.through)
                    && t.imported
                    && t.status == TokenStatus::NewCase
                    && !t.receipt_advanced,
                "RECEIPT_NOT_IMPORTED",
            )?;
            t.receipt_advanced = true;
            g.receipt_prefix = p.through;
            d.funding.owner = owner("token", &tid)?;
            d.funding.release = t.advanced;
            v.put(token(&tid)?, State::Token(Box::new(t)))?;
            v.put(gateway(&p.gateway)?, State::Gateway(Box::new(g)))?;
            Ok(5)
        }
        w::Command::LocalTerminal { payload: p, .. } => {
            let e = v.enrolled()?;
            let mut g = v.gr(&p.grant, false)?;
            require(
                p.gateway == *i.host && p.proof.host == e.terms.store && !g.terminal,
                "GRANT_STATE",
            )?;
            if p.proof.fact_kind == w::FactKind::Retirement {
                let retired: w::RetireGrant = v
                    .source(&p.proof, &[w::FactKind::Retirement], &p.grant)?
                    .payload()?;
                require(
                    retired.grant == p.grant
                        && g.token.is_none()
                        && g.status == GrantStatus::LocalHeld,
                    "TERMINAL_PROOF",
                )?;
                g.status = GrantStatus::RetiredUnclaimed;
            } else {
                let t = g.token.clone().ok_or_else(|| fail("TERMINAL_PROOF"))?;
                let rec: w::Reconcile = v
                    .source(&p.proof, &[w::FactKind::Reconciliation], &t)?
                    .payload()?;
                require(rec.token == t, "TERMINAL_PROOF")?;
            }
            g.terminal = true;
            d.funding.owner = owner("grant-local", &p.grant)?;
            d.funding.release = true;
            v.put(grant(&p.grant, false)?, State::Grant(Box::new(g)))?;
            Ok(5)
        }
        w::Command::Begin { payload: p, .. } => {
            let mut e = central(v)?;
            require(
                e.active_round.is_none()
                    && p.predecessor == e.last_round
                    && p.round == e.last_round.checked_add(one())?,
                "ROUND_PREDECESSOR",
            )?;
            if p.mode == w::BeginMode::FinishOnly {
                require(p.preparations.is_empty(), "FINISH_PREPARATION")?;
            } else {
                require(
                    p.preparations.len() == p.gateways.len(),
                    "ROUND_PREPARATIONS",
                )?;
                for g in &p.gateways {
                    let matches: Vec<_> = p.preparations.iter().filter(|q| q.host == *g).collect();
                    require(matches.len() == 1, "ROUND_PREPARATION_HOST")?;
                    let key = Id::parse(hash("namespace", &json!([g, p.round]))?.as_str())?;
                    let prepared: w::PrepareRound = v
                        .source(matches[0], &[w::FactKind::RoundPreparation], &key)?
                        .payload()?;
                    require(
                        prepared.gateway == *g
                            && prepared.round == p.round
                            && prepared.enrollment == e.enrollment
                            && prepared.predecessor == v.gw(g)?.acknowledged,
                        "ROUND_PREPARATION_BINDING",
                    )?;
                }
                v.reserve(
                    &owner("optional-round", &p.round)?,
                    work.bundle("cancel_central")?,
                    work,
                )?;
            }
            for family in &p.families {
                let State::Family(f) = v.load(&Point::family(family)?)? else {
                    return Err(fail("FAMILY"));
                };
                require(!f.closed, "CLOSE_RIGHT")?;
            }
            let mut cutoffs = Vec::new();
            for g in &p.gateways {
                let gw = v.gw(g)?;
                cutoffs.push(w::RoundBeginCutoffsItem {
                    gateway: g.clone(),
                    cutoff: gw.allocation,
                    gateway_predecessor: gw.acknowledged,
                });
            }
            sorted(&mut cutoffs)?;
            let body = w::RoundBegin {
                round: p.round,
                predecessor: p.predecessor,
                mode: if p.mode == w::BeginMode::FinishOnly {
                    w::RoundBeginMode::FinishOnly
                } else {
                    w::RoundBeginMode::Cancellable
                },
                cutoffs,
            };
            let r = round_from_begin(&e, p.clone(), body.clone())?;
            d.funding.owner = r.owner.clone();
            v.absent(&round(p.round)?)?;
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            e.active_round = Some(p.round);
            v.put(enrollment(i.registration)?, State::Enrollment(Box::new(e)))?;
            d.effects.push(w::Effect::RoundBegin { body });
            d.fact = Some((w::FactKind::Begin, rid(p.round)?));
            Ok(if p.mode == w::BeginMode::FinishOnly {
                5
            } else {
                6
            })
        }
        w::Command::SealBegin { payload: p, .. } => {
            let e = v.enrolled()?;
            let (b, body) = begin_source(v, &p.proof, p.round, &e)?;
            let r = round_from_begin(&e, b, body)?;
            let selected = r
                .gateways
                .iter()
                .find(|g| g.gateway == *i.host)
                .ok_or_else(|| fail("GATEWAY"))?;
            let mut gw = v.gw(i.host)?;
            require(
                p.gateway == *i.host
                    && p.predecessor == selected.predecessor
                    && gw.installed == selected.predecessor
                    && gw.mode == GatewayMode::Open,
                "ROUND_STALE",
            )?;
            gw.mode = GatewayMode::Sealing;
            gw.active = Some(p.round);
            d.funding.owner = r.owner.clone();
            v.absent(&round(p.round)?)?;
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            v.put(gateway(i.host)?, State::Gateway(Box::new(gw)))?;
            Ok(5)
        }
        w::Command::Sealed { payload: p, .. } => {
            v.enrolled()?;
            let mut r = state(v, p.round)?;
            let mut gw = v.gw(i.host)?;
            require(
                p.gateway == *i.host
                    && gw.active == Some(p.round)
                    && gw.mode == GatewayMode::Sealing,
                "ROUND_STALE",
            )?;
            let selected = r
                .gateways
                .iter_mut()
                .find(|g| g.gateway == *i.host)
                .ok_or_else(|| fail("GATEWAY"))?;
            let scan = i.seal.ok_or_else(|| fail("NEED_SEAL_SCAN"))?;
            require(
                scan.round == p.round
                    && scan.gateway == *i.host
                    && scan.cutoff == selected.cutoff
                    && scan.receipt_high == gw.receipt,
                "SEAL_BINDING",
            )?;
            selected.seal = Some(scan.clone());
            gw.mode = GatewayMode::Sealed;
            d.funding.owner = r.owner.clone();
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            v.put(gateway(i.host)?, State::Gateway(Box::new(gw)))?;
            d.effects.push(w::Effect::Seal { body: scan.clone() });
            d.fact = Some((w::FactKind::Seal, rid(p.round)?));
            Ok(5)
        }
        w::Command::Drain { payload: p, .. } => {
            central(v)?;
            let mut r = state(v, p.round)?;
            require(
                r.status == RoundStatus::Draining && p.proof.host == p.gateway,
                "ROUND_STALE",
            )?;
            let s = v.source(&p.proof, &[w::FactKind::Seal], &rid(p.round)?)?;
            let effects = s.effects()?;
            let [w::Effect::Seal { body }] = effects.as_slice() else {
                return Err(fail("SEAL_PROOF"));
            };
            let selected = r
                .gateways
                .iter_mut()
                .find(|g| g.gateway == p.gateway)
                .ok_or_else(|| fail("GATEWAY"))?;
            let gw = v.gw(&p.gateway)?;
            require(
                body.gateway == p.gateway
                    && body.round == p.round
                    && body.cutoff == selected.cutoff
                    && gw.allocation_prefix >= selected.cutoff
                    && gw.receipt_prefix >= body.receipt_high
                    && selected.seal.is_none(),
                "UNRECONCILED_FENCE",
            )?;
            selected.seal = Some(body.clone());
            selected.observation = Some(p.proof.trusted_observation_ref.clone());
            d.funding.owner = r.owner.clone();
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            Ok(5)
        }
        w::Command::Ready { payload: p, .. } => {
            central(v)?;
            let mut r = state(v, p.round)?;
            require(
                r.status == RoundStatus::Draining
                    && r.gateways
                        .iter()
                        .all(|g| g.seal.is_some() && g.observation.is_some()),
                "UNRECONCILED_FENCE",
            )?;
            r.status = RoundStatus::Ready;
            d.funding.owner = r.owner.clone();
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            Ok(5)
        }
        w::Command::Close {
            payload: p,
            authority: a,
            ..
        } => close(v, d, p, a),
        w::Command::Abort { payload: p, .. } => {
            let mut e = central(v)?;
            let mut r = state(v, p.round)?;
            require(
                r.begin.mode == w::BeginMode::Cancellable
                    && matches!(r.status, RoundStatus::Draining | RoundStatus::Ready),
                "ABORT_REFUSED",
            )?;
            r.status = RoundStatus::Aborted;
            d.funding.owner = r.owner.clone();
            if r.gateways.is_empty() {
                e.active_round = None;
                e.last_round = p.round;
                d.funding.release = true;
                v.put(enrollment(i.registration)?, State::Enrollment(Box::new(e)))?;
            }
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            d.fact = Some((w::FactKind::Terminal, rid(p.round)?));
            Ok(5)
        }
        w::Command::Install { payload: p, .. } => {
            let e = v.enrolled()?;
            let (b, body) = begin_source(v, &p.begin, p.round, &e)?;
            let mut r = round_from_begin(&e, b, body)?;
            let selected = r
                .gateways
                .iter()
                .find(|g| g.gateway == p.gateway)
                .ok_or_else(|| fail("GATEWAY"))?;
            let mut gw = v.gw(i.host)?;
            require(
                p.gateway == *i.host
                    && p.proof.host == e.terms.store
                    && gw.installed == selected.predecessor
                    && gw.active.is_none_or(|n| n == p.round),
                "ROUND_STALE",
            )?;
            let source = v.source(&p.proof, &[w::FactKind::Terminal], &rid(p.round)?)?;
            let effects = source.effects()?;
            match p.outcome {
                w::InstallOutcome::Committed => {
                    let [w::Effect::Closure { body: certificate }] = effects.as_slice() else {
                        return Err(fail("TERMINAL_PROOF"));
                    };
                    require(
                        certificate.round == p.round
                            && certificate.enrollment == e.enrollment
                            && certificate.families == r.begin.families,
                        "TERMINAL_BINDING",
                    )?;
                    if gw.clock_floor.as_str() < certificate.closed_at.as_str() {
                        gw.clock_floor = certificate.closed_at.clone();
                    }
                    r.status = RoundStatus::Committed;
                    r.closed_at = Some(certificate.closed_at.clone());
                }
                w::InstallOutcome::Aborted => {
                    let aborted: w::Abort = source.payload()?;
                    require(
                        aborted.round == p.round
                            && effects.is_empty()
                            && r.begin.mode == w::BeginMode::Cancellable,
                        "TERMINAL_BINDING",
                    )?;
                    r.status = RoundStatus::Aborted;
                }
            }
            gw.installed = p.round;
            gw.active = None;
            gw.mode = GatewayMode::Open;
            d.funding.owner = r.owner.clone();
            d.funding.release = true;
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            v.put(gateway(i.host)?, State::Gateway(Box::new(gw)))?;
            d.fact = Some((w::FactKind::Installation, rid(p.round)?));
            Ok(5)
        }
        w::Command::AckInstall { payload: p, .. } => {
            let mut e = central(v)?;
            let mut r = state(v, p.round)?;
            require(
                matches!(r.status, RoundStatus::Committed | RoundStatus::Aborted)
                    && p.proof.host == p.gateway,
                "INSTALL_UNKNOWN",
            )?;
            let installed: w::Install = v
                .source(&p.proof, &[w::FactKind::Installation], &rid(p.round)?)?
                .payload()?;
            require(
                installed.round == p.round
                    && installed.gateway == p.gateway
                    && installed.outcome
                        == if r.status == RoundStatus::Committed {
                            w::InstallOutcome::Committed
                        } else {
                            w::InstallOutcome::Aborted
                        },
                "INSTALL_BINDING",
            )?;
            let selected = r
                .gateways
                .iter_mut()
                .find(|g| g.gateway == p.gateway)
                .ok_or_else(|| fail("GATEWAY"))?;
            require(!selected.acknowledged, "INSTALL_UNKNOWN")?;
            selected.acknowledged = true;
            let mut gw = v.gw(&p.gateway)?;
            require(gw.acknowledged == selected.predecessor, "ROUND_STALE")?;
            gw.acknowledged = p.round;
            d.funding.owner = r.owner.clone();
            if r.gateways.iter().all(|g| g.acknowledged) {
                e.active_round = None;
                e.last_round = p.round;
                d.funding.release = true;
                v.put(enrollment(i.registration)?, State::Enrollment(Box::new(e)))?;
            }
            v.put(gateway(&p.gateway)?, State::Gateway(Box::new(gw)))?;
            v.put(round(p.round)?, State::Round(Box::new(r)))?;
            Ok(6)
        }
        _ => {
            let _ = work;
            Err(fail("TRANSITION_NOT_IN_FIRST_CHECKPOINT"))
        }
    }
}
fn close(v: &mut View<'_>, d: &mut Delta, p: &w::Close, a: &w::Authority) -> Result<usize> {
    let i = v.input;
    let mut e = central(v)?;
    let mut r = state(v, p.round)?;
    require(
        r.status == RoundStatus::Ready && p.closed_at == a.observed_at,
        "NOT_READY",
    )?;
    let mut families = Vec::new();
    let mut heads = Vec::new();
    for terms in &e.terms.families {
        let State::Family(f) = v.load(&Point::family(&terms.key)?)? else {
            return Err(fail("FAMILY"));
        };
        heads.push(w::FamilyHead {
            family: terms.key.clone(),
            terms: hash("enrollment", terms)?,
            closed: f.closed,
            unavailable: f.unavailable,
            entitlement: f.entitlement.clone(),
        });
        families.push(*f);
    }
    for f in &mut families {
        if r.begin.families.contains(&f.terms.key) {
            require(!f.closed, "CLOSE_RIGHT")?;
            f.closed = true;
            f.unavailable = true;
            f.closed_at = Some(p.closed_at.clone());
        }
    }
    loop {
        let mut changed = false;
        for n in 0..families.len() {
            if !families[n].unavailable
                && families[n].terms.prerequisites.iter().any(|j| {
                    families[*j as usize].unavailable
                        && matches!(
                            families[*j as usize].entitlement,
                            w::EntitlementHead::Unconsumed {}
                        )
                })
            {
                families[n].unavailable = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut before = Vec::new();
    let mut after = Vec::new();
    for supplier in &e.terms.suppliers {
        let relevant: Vec<_> = families
            .iter()
            .filter(|f| f.terms.supplier_pool == supplier.id)
            .collect();
        if !relevant.is_empty()
            && relevant
                .iter()
                .any(|f| r.begin.families.contains(&f.terms.key))
            && relevant.iter().all(|f| f.closed)
        {
            let key = id(PointKind::Supplier, *b"SUPPLIER", supplier.id.as_str())?;
            let State::Supplier(mut s) = v.load(&key)? else {
                return Err(fail("SUPPLIER"));
            };
            before.push(s.clone());
            s.released = s.released.checked_add(s.held)?;
            s.held = Count::ZERO;
            after.push(s.clone());
            v.put(key, State::Supplier(s))?;
        }
    }
    let mut coverage = Vec::new();
    for selected in &r.gateways {
        let gw = v.gw(&selected.gateway)?;
        let seal = selected
            .seal
            .as_ref()
            .ok_or_else(|| fail("UNRECONCILED_FENCE"))?;
        coverage.push(w::Coverage::CompleteGatewayCutoff {
            gateway: selected.gateway.clone(),
            cutoff: selected.cutoff,
            allocation_prefix: gw.allocation_prefix,
            receipt_high: seal.receipt_high,
            receipt_prefix: gw.receipt_prefix,
            disposition_root: seal.disposition_root.clone(),
            receipt_root: seal.receipt_root.clone(),
            observation: selected
                .observation
                .clone()
                .ok_or_else(|| fail("UNRECONCILED_FENCE"))?,
        });
    }
    let mut unavailable: Vec<_> = families
        .iter()
        .filter(|f| f.unavailable)
        .map(|f| f.terms.key.clone())
        .collect();
    sorted(&mut heads)?;
    sorted(&mut before)?;
    sorted(&mut after)?;
    sorted(&mut coverage)?;
    sorted(&mut unavailable)?;
    let certificate = w::Certificate {
        predecessor: i.prior_root.clone(),
        enrollment: e.enrollment.clone(),
        family_heads: heads,
        families: r.begin.families.clone(),
        unavailable,
        supplier_before: before,
        supplier_after: after,
        round: p.round,
        cutoffs: coverage,
        closed_at: p.closed_at.clone(),
    };
    certificate.validate()?;
    let digest = hash("closure", &certificate)?;
    for mut f in families {
        if f.unavailable && f.first_closure.is_none() {
            f.first_closure = Some(digest.clone());
        }
        let point = Point::family(&f.terms.key)?;
        if v.read(&point)? != Some(State::Family(Box::new(f.clone()))) {
            v.put(point, State::Family(Box::new(f)))?;
        }
    }
    let count = 6 + r.begin.families.len() + certificate.supplier_after.len();
    d.funding.owner = r.owner.clone();
    r.status = RoundStatus::Committed;
    r.closed_at = Some(p.closed_at.clone());
    if r.gateways.is_empty() {
        e.active_round = None;
        e.last_round = p.round;
        d.funding.release = true;
        v.put(enrollment(i.registration)?, State::Enrollment(Box::new(e)))?;
    }
    v.put(round(p.round)?, State::Round(Box::new(r)))?;
    v.put(
        id(PointKind::Round, *b"CERTIFIC", &p.round.value().to_string())?,
        State::Certificate(Box::new(certificate.clone())),
    )?;
    d.effects.push(w::Effect::Closure { body: certificate });
    d.fact = Some((w::FactKind::Terminal, rid(p.round)?));
    Ok(count)
}

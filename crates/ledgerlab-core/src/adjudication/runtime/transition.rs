//! Pure incremental first-path transitions over explicitly observed point rows.
mod terminal;
use super::{accounting::Worksheet, command_digest, command_value, hash, points::*};
use crate::adjudication::{
    self as r3, commands as w,
    types::{Atoms, Count, Digest, Id, Time},
    Validate,
};
use crate::{Error, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn fail(code: &'static str) -> Error {
    Error::new(code, "incremental R3 transition")
}
fn require(ok: bool, code: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(fail(code))
    }
}
fn one() -> Count {
    Count::new(1).expect("one")
}
fn id(kind: PointKind, tag: [u8; 8], s: &str) -> Result<Point> {
    Point::id(kind, tag, s)
}
fn enrollment(reg: &Id) -> Result<Point> {
    id(PointKind::Enrollment, *b"ENROLL__", reg.as_str())
}
fn gateway(g: &Id) -> Result<Point> {
    id(PointKind::Gateway, *b"GATEWAY_", g.as_str())
}
fn token(t: &Id) -> Result<Point> {
    id(PointKind::Token, *b"TOKEN___", t.as_str())
}
fn grant(g: &Id, central: bool) -> Result<Point> {
    id(
        if central {
            PointKind::GrantRegistry
        } else {
            PointKind::Grant
        },
        *b"GRANT___",
        g.as_str(),
    )
}
fn round(n: Count) -> Result<Point> {
    id(PointKind::Round, *b"ROUND___", &n.value().to_string())
}
fn account(host: &Id) -> Result<Point> {
    id(PointKind::Resource, *b"RESOURCE", host.as_str())
}
fn allocation(owner: &str) -> Result<Point> {
    id(PointKind::Resource, *b"OWNER___", owner)
}
fn owner(kind: &str, key: &impl serde::Serialize) -> Result<String> {
    Ok(hash("namespace", &json!([kind, key]))?.as_str().to_owned())
}
fn decode<T: DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| Error::new("STATE", e.to_string()))
}
#[derive(Clone, Debug)]
pub struct SourceFact {
    pub proof: w::Proof,
    pub object: w::RetainedObject,
}
impl SourceFact {
    fn body(&self) -> Result<Value> {
        let bytes = r3::proofs::VerifiedObjectBytes::check(self.object.clone())?;
        crate::canonical::parse_bounded(bytes.bytes(), 262144)
    }
    fn payload<T: DeserializeOwned>(&self) -> Result<T> {
        decode(self.body()?["payload"].clone())
    }
    fn effects(&self) -> Result<Vec<w::Effect>> {
        decode(self.body()?["effects"].clone())
    }
}
pub struct Input<'a> {
    pub command: &'a w::Command,
    pub store: &'a Id,
    pub registration: &'a Id,
    pub host: &'a Id,
    pub prior_root: &'a Digest,
    pub next_ordinal: Count,
    pub writer_epoch: Count,
    pub observations: &'a [Observation],
    pub sources: &'a [SourceFact],
    pub introduced_objects: usize,
    pub seal: Option<&'a w::Seal>,
}
#[derive(Clone, Debug)]
pub struct Funding {
    pub owner: String,
    pub slot: String,
    pub release: bool,
    pub actual_receipt: bool,
}
#[derive(Clone, Debug)]
pub struct Delta {
    pub mutations: Vec<Mutation>,
    pub effects: Vec<w::Effect>,
    pub funding: Funding,
    pub fact: Option<(w::FactKind, Id)>,
    pub enrollment: Option<w::Enroll>,
}
#[derive(Clone, Debug)]
pub enum Step {
    Need(Vec<Point>),
    Ready(Box<Delta>),
}
struct View<'a> {
    input: &'a Input<'a>,
    pending: BTreeMap<Point, State>,
    missing: Vec<Point>,
}
impl View<'_> {
    fn read(&mut self, p: &Point) -> Result<Option<State>> {
        if let Some(s) = self.pending.get(p) {
            return Ok(Some(s.clone()));
        }
        if let Some(o) = self.input.observations.iter().find(|o| o.point == *p) {
            return Ok(o.state.clone());
        }
        self.missing.push(p.clone());
        Err(fail("NEED_POINT"))
    }
    fn load(&mut self, p: &Point) -> Result<State> {
        self.read(p)?.ok_or_else(|| fail("MISSING_STATE"))
    }
    fn put(&mut self, p: Point, state: State) -> Result<()> {
        self.read(&p)?;
        self.pending.insert(p, state);
        Ok(())
    }
    fn absent(&mut self, p: &Point) -> Result<()> {
        require(self.read(p)?.is_none(), "IDENTITY_CONFLICT")
    }
    fn enrolled(&mut self) -> Result<EnrollmentState> {
        match self.load(&enrollment(self.input.registration)?)? {
            State::Enrollment(v) => Ok(*v),
            State::Preparation(preparation) => {
                let source = self
                    .input
                    .sources
                    .iter()
                    .find(|s| {
                        s.object.kind == w::FactKind::Enrollment
                            && s.proof.host == *self.input.store
                    })
                    .ok_or_else(|| fail("NEED_ENROLLMENT"))?;
                let source = self.source(
                    &source.proof,
                    &[w::FactKind::Enrollment],
                    self.input.registration,
                )?;
                let terms: w::Enroll = source.payload()?;
                let mut intent = serde_json::to_value(&terms).map_err(|_| fail("ENROLLMENT"))?;
                intent
                    .as_object_mut()
                    .ok_or_else(|| fail("ENROLLMENT"))?
                    .remove("preparations");
                require(
                    terms.store == preparation.store
                        && terms.scope == preparation.scope
                        && terms.registration == preparation.registration
                        && terms.gateways.contains(&preparation.namespace)
                        && hash("enrollment", &intent)? == preparation.intent,
                    "ENROLLMENT_INTENT",
                )?;
                let state = EnrollmentState {
                    enrollment: hash("enrollment", &terms)?,
                    terms,
                    active_round: None,
                    last_round: Count::ZERO,
                };
                self.put(
                    enrollment(self.input.registration)?,
                    State::Enrollment(Box::new(state.clone())),
                )?;
                Ok(state)
            }
            _ => Err(fail("NOT_ENROLLED")),
        }
    }
    fn gw(&mut self, g: &Id) -> Result<GatewayState> {
        match self.load(&gateway(g)?)? {
            State::Gateway(v) => Ok(*v),
            _ => Err(fail("GATEWAY")),
        }
    }
    fn tok(&mut self, t: &Id) -> Result<TokenState> {
        match self.load(&token(t)?)? {
            State::Token(v) => Ok(*v),
            _ => Err(fail("TOKEN_STATE")),
        }
    }
    fn gr(&mut self, g: &Id, central: bool) -> Result<GrantState> {
        match self.load(&grant(g, central)?)? {
            State::Grant(v) => Ok(*v),
            _ => Err(fail("GRANT_STATE")),
        }
    }
    fn source(&self, p: &w::Proof, kind: &[w::FactKind], key: &Id) -> Result<&SourceFact> {
        let cv = command_value(self.input.command)?;
        require(
            p.store == *self.input.store
                && p.registration == *self.input.registration
                && serde_json::to_value(&p.scope).map_err(|_| fail("SCOPE"))? == cv["key"][0],
            "PROOF_BINDING",
        )?;
        self.input
            .sources
            .iter()
            .find(|s| {
                s.proof == *p
                    && kind.contains(&s.object.kind)
                    && r3::canonical_bytes(&s.object.full_key, r3::COMMAND_BYTES).ok()
                        == r3::canonical_bytes(key, r3::COMMAND_BYTES).ok()
            })
            .ok_or_else(|| fail("PROOF_MEMBER"))
    }
    fn reserve(&mut self, own: &str, bundle: &[String], work: &Worksheet) -> Result<()> {
        let ap = account(self.input.host)?;
        let op = allocation(own)?;
        self.absent(&op)?;
        let State::Resource(mut a) = self.load(&ap)? else {
            return Err(fail("RESOURCE_HOST"));
        };
        let hold = a.reserve(own.into(), bundle, work)?;
        self.put(ap, State::Resource(a))?;
        self.put(op, State::Allocation(hold))
    }
}
fn new_token(t: w::Token) -> TokenState {
    TokenState {
        token: t,
        status: TokenStatus::Issued,
        imported: false,
        reconciled: false,
        advanced: false,
        receipt_advanced: false,
        receipt: None,
        delivery: None,
        submission: None,
    }
}
fn new_gateway(n: w::Namespace, epoch: Count) -> GatewayState {
    GatewayState {
        namespace: n,
        epoch,
        mode: GatewayMode::Open,
        active: None,
        installed: Count::ZERO,
        acknowledged: Count::ZERO,
        clock_floor: Time::parse("0001-01-01T00:00:00.000000Z").expect("minimum time"),
        allocation: Count::ZERO,
        receipt: Count::ZERO,
        allocation_prefix: Count::ZERO,
        receipt_prefix: Count::ZERO,
    }
}

pub fn step(input: &Input<'_>) -> Result<Step> {
    input.command.validate()?;
    require(
        input.observations.len() <= 256 && input.sources.len() <= 6,
        "RESOLUTION_BOUND",
    )?;
    let mut view = View {
        input,
        pending: BTreeMap::new(),
        missing: vec![],
    };
    match execute(&mut view) {
        Ok(mut d) => {
            for (point, state) in view.pending {
                let old = input
                    .observations
                    .iter()
                    .find(|o| o.point == point)
                    .ok_or_else(|| fail("NEED_POINT"))?;
                d.mutations.push(Mutation {
                    point,
                    prior: old.revision,
                    revision: old.revision.unwrap_or(Count::ZERO).checked_add(one())?,
                    state,
                });
            }
            Ok(Step::Ready(Box::new(d)))
        }
        Err(e) if e.code == "NEED_POINT" => Ok(Step::Need(view.missing)),
        Err(e) => Err(e),
    }
}
fn execute(v: &mut View<'_>) -> Result<Delta> {
    let i = v.input;
    let cv = command_value(i.command)?;
    let kind = cv["kind"].as_str().ok_or_else(|| fail("COMMAND"))?;
    let work = Worksheet::frozen()?;
    let mut terminal_indexes = None;
    let mut prepaid = false;
    let mut d = Delta {
        mutations: vec![],
        effects: vec![],
        funding: Funding {
            owner: owner("command", &json!([i.host, cv["key"]]))?,
            slot: kind.into(),
            release: false,
            actual_receipt: true,
        },
        fact: None,
        enrollment: None,
    };
    match i.command {
        w::Command::PrepareEnroll {
            key, payload: p, ..
        } => {
            require(
                p.store == *i.store
                    && p.registration == *i.registration
                    && p.gateway == *i.host
                    && p.namespace.gateway == *i.host
                    && p.namespace.scope == p.scope
                    && key.0 == p.scope
                    && p.store != p.gateway,
                "PREPARATION_BINDING",
            )?;
            require(i.writer_epoch == one(), "WRITER_ANCHOR")?;
            let ep = enrollment(i.registration)?;
            v.absent(&ep)?;
            v.put(ep, State::Preparation(p.clone()))?;
            v.put(
                gateway(i.host)?,
                State::Gateway(Box::new(new_gateway(p.namespace.clone(), i.writer_epoch))),
            )?;
            v.reserve(&d.funding.owner, &[kind.into()], &work)?;
            d.funding.release = true;
            charge(v, &d, &work, kind, 40)?;
            prepaid = true;
            for n in 0..32 {
                let o = owner("close", &n)?;
                v.reserve(&o, work.bundle("finish_gateway")?, &work)?;
            }
            d.fact = Some((w::FactKind::EnrollPreparation, p.gateway.clone()));
        }
        w::Command::Enroll {
            key, payload: p, ..
        } => {
            require(
                p.store == *i.host
                    && p.store == *i.store
                    && p.registration == *i.registration
                    && key.0 == p.scope,
                "ENROLL_BINDING",
            )?;
            v.absent(&enrollment(i.registration)?)?;
            require(
                p.preparations.len() == p.gateways.len(),
                "PREPARATION_COUNT",
            )?;
            let mut intent = serde_json::to_value(p).map_err(|_| fail("ENROLL"))?;
            intent
                .as_object_mut()
                .ok_or_else(|| fail("ENROLL"))?
                .remove("preparations");
            let intent = hash("enrollment", &intent)?;
            require(
                p.gateways
                    .iter()
                    .map(|g| g.gateway.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == p.gateways.len()
                    && p.gateways
                        .iter()
                        .map(|g| g.tag.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == p.gateways.len(),
                "GATEWAY_DUPLICATE",
            )?;
            require(
                p.suppliers
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == p.suppliers.len()
                    && p.pools
                        .iter()
                        .map(|p| p.id.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == p.pools.len(),
                "POOL_DUPLICATE",
            )?;
            for f in &p.families {
                require(
                    f.starts_at.as_str() < f.occurs_before.as_str()
                        && f.received_by.as_str() <= f.accepted_by.as_str()
                        && f.accepted_by.as_str() <= f.correction_by.as_str(),
                    "TERMS_WINDOW",
                )?;
                require(
                    match f.book {
                        w::FamilyTermsBook::Retail => f.supplier_pool.as_str() == "none",
                        w::FamilyTermsBook::Supplier => {
                            p.suppliers.iter().any(|s| s.id == f.supplier_pool)
                        }
                    },
                    "SUPPLIER_FAMILY",
                )?;
            }
            let mut keys = std::collections::BTreeSet::new();
            for (n, f) in p.families.iter().enumerate() {
                require(
                    f.key.0 == p.scope
                        && f.key.3 == p.target
                        && keys.insert(super::family_key(&f.key)?),
                    "FAMILY_TARGET",
                )?;
                require(
                    f.prerequisites
                        .iter()
                        .all(|a| *a < (p.families.len() as u64) && *a != n as u64),
                    "PREREQUISITE",
                )?;
                v.put(
                    Point::family(&f.key)?,
                    State::Family(Box::new(FamilyState {
                        terms: f.clone(),
                        closed: false,
                        unavailable: false,
                        first_closure: None,
                        closed_at: None,
                        entitlement: w::EntitlementHead::Unconsumed {},
                    })),
                )?;
            }
            fn visit(n: usize, seen: &mut Vec<usize>, families: &[w::FamilyTerms]) -> Result<()> {
                require(!seen.contains(&n), "CYCLE")?;
                seen.push(n);
                for j in &families[n].prerequisites {
                    visit(*j as usize, seen, families)?;
                }
                seen.pop();
                Ok(())
            }
            for n in 0..p.families.len() {
                visit(n, &mut vec![], &p.families)?;
            }
            for ns in &p.gateways {
                let matches: Vec<_> = p
                    .preparations
                    .iter()
                    .filter(|a| a.host == ns.gateway)
                    .collect();
                require(matches.len() == 1, "PREPARATION_HOST")?;
                let source =
                    v.source(matches[0], &[w::FactKind::EnrollPreparation], &ns.gateway)?;
                let preparation: w::PrepareEnroll = source.payload()?;
                require(
                    preparation.intent == intent
                        && preparation.namespace == *ns
                        && preparation.store == p.store
                        && preparation.scope == p.scope
                        && preparation.registration == p.registration,
                    "PREPARATION_BINDING",
                )?;
                v.put(
                    gateway(&ns.gateway)?,
                    State::Gateway(Box::new(new_gateway(ns.clone(), Count::ZERO))),
                )?;
            }
            for supplier in &p.suppliers {
                require(
                    supplier
                        .consumed
                        .checked_add(supplier.held)?
                        .checked_add(supplier.released)?
                        == supplier.maximum,
                    "SUPPLIER_CONSERVATION",
                )?;
                v.put(
                    id(PointKind::Supplier, *b"SUPPLIER", supplier.id.as_str())?,
                    State::Supplier(supplier.clone()),
                )?;
            }
            v.reserve(&d.funding.owner, &[kind.into()], &work)?;
            d.funding.release = true;
            charge(
                v,
                &d,
                &work,
                kind,
                4 + p.families.len()
                    + 2 * p.gateways.len()
                    + p.suppliers.len()
                    + p.pools.len()
                    + 32
                    + 1,
            )?;
            prepaid = true;
            for n in 0..32 {
                v.reserve(&owner("close", &n)?, work.bundle("finish_central")?, &work)?;
            }
            v.put(
                enrollment(i.registration)?,
                State::Enrollment(Box::new(EnrollmentState {
                    terms: p.clone(),
                    enrollment: hash("enrollment", p)?,
                    active_round: None,
                    last_round: Count::ZERO,
                })),
            )?;
            d.fact = Some((w::FactKind::Enrollment, p.registration.clone()));
            d.enrollment = Some(p.clone());
        }
        w::Command::LocalGrant { payload: p, .. } => {
            let source = v.source(&p.proof, &[w::FactKind::Enrollment], i.registration)?;
            let enroll: w::Enroll = source.payload()?;
            let g = &p.grant;
            let mut gw = v.gw(i.host)?;
            require(
                g.store == *i.store
                    && g.registration == *i.registration
                    && g.gateway == *i.host
                    && g.namespace == gw.namespace
                    && gw.mode == GatewayMode::Open
                    && g.journal_head == *i.prior_root,
                "GRANT_BINDING",
            )?;
            require(
                enroll.gateways.iter().any(|n| n == &g.namespace),
                "ENROLLMENT_PROOF",
            )?;
            let prefix = format!("gr1.{}.", g.namespace.tag);
            require(
                g.id.as_str()
                    .strip_prefix(&prefix)
                    .is_some_and(|x| !x.is_empty() && x.len() <= 91),
                "GRANT_NAMESPACE",
            )?;
            let mut unsigned = serde_json::to_value(g).map_err(|_| fail("GRANT"))?;
            unsigned
                .as_object_mut()
                .ok_or_else(|| fail("GRANT"))?
                .remove("authentication");
            require(
                hash("grant", &unsigned)? == g.authentication,
                "GRANT_DIGEST",
            )?;
            v.absent(&grant(&g.id, false)?)?;
            d.funding.owner = owner("grant-local", &g.id)?;
            v.reserve(&d.funding.owner, work.bundle("local_grant")?, &work)?;
            let State::Allocation(held) = v.load(&allocation(&d.funding.owner)?)? else {
                return Err(fail("ALLOCATION"));
            };
            require(held.held.fits(&g.resources), "GRANT_ENVELOPE")?;
            let mut counters = w::Counters::zero();
            for slot in &held.slots {
                counters = counters.checked_add(&work.template(slot)?.counters()?)?;
            }
            require(
                counters
                    .dimensions()
                    .into_iter()
                    .zip(g.counters.dimensions())
                    .all(|(a, b)| a <= b),
                "GRANT_COUNTERS",
            )?;
            v.put(
                grant(&g.id, false)?,
                State::Grant(Box::new(GrantState {
                    grant: g.clone(),
                    status: GrantStatus::LocalHeld,
                    token: None,
                    terminal: false,
                })),
            )?;
            let cached = v.enrolled()?;
            require(
                cached.terms == enroll && gw.epoch == i.writer_epoch,
                "ENROLLMENT_PROOF",
            )?;
            gw.epoch = i.writer_epoch;
            v.put(gateway(i.host)?, State::Gateway(Box::new(gw)))?;
            d.fact = Some((w::FactKind::Grant, g.id.clone()));
        }
        w::Command::RegisterGrant { payload: p, .. } => {
            let e = v.enrolled()?;
            require(*i.host == e.terms.store, "CENTRAL_HOST")?;
            let source = v.source(&p.proof, &[w::FactKind::Grant], &p.grant.id)?;
            let source: w::LocalGrant = source.payload()?;
            require(
                source.grant == p.grant
                    && p.proof.host == p.grant.gateway
                    && p.grant.store == *i.store
                    && p.grant.registration == *i.registration,
                "GRANT_PROOF",
            )?;
            v.absent(&grant(&p.grant.id, true)?)?;
            d.funding.owner = owner("grant-central", &p.grant.id)?;
            v.reserve(&d.funding.owner, work.bundle("central_grant")?, &work)?;
            v.put(
                grant(&p.grant.id, true)?,
                State::Grant(Box::new(GrantState {
                    grant: p.grant.clone(),
                    status: GrantStatus::RegisteredUnclaimed,
                    token: None,
                    terminal: false,
                })),
            )?;
        }
        w::Command::Issue { payload: p, .. } => {
            let e = v.enrolled()?;
            let mut g = v.gr(&p.grant, true)?;
            let mut gw = v.gw(&p.token.gateway)?;
            require(
                *i.host == e.terms.store
                    && g.status == GrantStatus::RegisteredUnclaimed
                    && p.token.grant == p.grant
                    && p.token.gateway == g.grant.gateway,
                "GRANT_UNAVAILABLE",
            )?;
            require(
                e.active_round.is_none() || p.token.category == w::TokenCategory::Adjustment,
                "ISSUANCE_FROZEN",
            )?;
            require(
                p.token.allocation == gw.allocation.checked_add(one())?,
                "ALLOCATION_GAP",
            )?;
            require(
                p.token.claim
                    == hash(
                        "claim",
                        &json!([
                            p.token.grant,
                            p.token.id,
                            p.token.gateway,
                            p.token.allocation,
                            p.token.category
                        ]),
                    )?,
                "CLAIM_DIGEST",
            )?;
            v.absent(&token(&p.token.id)?)?;
            d.funding.owner = owner("token", &p.token.id)?;
            v.reserve(&d.funding.owner, work.bundle("central_token")?, &work)?;
            g.status = GrantStatus::Claimed;
            g.token = Some(p.token.id.clone());
            gw.allocation = p.token.allocation;
            let old_owner = owner("grant-central", &p.grant)?;
            let ap = account(i.host)?;
            let State::Resource(mut a) = v.load(&ap)? else {
                return Err(fail("RESOURCE"));
            };
            let State::Allocation(mut o) = v.load(&allocation(&old_owner)?)? else {
                return Err(fail("ALLOCATION"));
            };
            a.terminal_slack(&mut o, &work)?;
            v.put(ap, State::Resource(a))?;
            v.put(allocation(&old_owner)?, State::Allocation(o))?;
            v.put(grant(&p.grant, true)?, State::Grant(Box::new(g)))?;
            v.put(gateway(&p.token.gateway)?, State::Gateway(Box::new(gw)))?;
            v.put(
                Point::position(PointKind::Allocation, &p.token.gateway, p.token.allocation)?,
                State::Position(p.token.id.clone()),
            )?;
            v.put(
                token(&p.token.id)?,
                State::Token(Box::new(new_token(p.token.clone()))),
            )?;
            d.fact = Some((w::FactKind::Claim, p.token.id.clone()));
        }
        w::Command::Activate { payload: p, .. } => {
            v.enrolled()?;
            let source = v.source(&p.proof, &[w::FactKind::Claim], &p.token)?;
            let issue: w::Issue = source.payload()?;
            let mut g = v.gr(&issue.grant, false)?;
            let gw = v.gw(i.host)?;
            require(
                p.gateway == *i.host
                    && issue.token.gateway == *i.host
                    && issue.token.grant == g.grant.id
                    && g.status == GrantStatus::LocalHeld
                    && !g.terminal
                    && gw.mode == GatewayMode::Open,
                "TOKEN_STATE",
            )?;
            v.absent(&token(&p.token)?)?;
            require(issue.token.id == p.token, "TOKEN_BINDING")?;
            let mut t = new_token(issue.token);
            t.status = TokenStatus::Active;
            g.status = GrantStatus::Claimed;
            g.token = Some(p.token.clone());
            d.funding.owner = owner("grant-local", &g.grant.id)?;
            v.put(grant(&g.grant.id, false)?, State::Grant(Box::new(g)))?;
            v.put(
                Point::position(PointKind::Allocation, i.host, t.token.allocation)?,
                State::Position(p.token.clone()),
            )?;
            v.put(token(&p.token)?, State::Token(Box::new(t)))?;
        }
        w::Command::Receive {
            key,
            payload: p,
            authority: a,
        } => {
            let e = v.enrolled()?;
            let mut t = v.tok(&p.token)?;
            let mut gw = v.gw(i.host)?;
            require(
                *key == p.delivery
                    && p.gateway == *i.host
                    && t.token.gateway == *i.host
                    && t.status == TokenStatus::Active
                    && gw.mode == GatewayMode::Open,
                "TOKEN_STATE",
            )?;
            require(
                p.epoch == i.writer_epoch && gw.epoch == i.writer_epoch,
                "WRITER_EPOCH",
            )?;
            require(
                p.submission.occurred_at.as_str() <= p.received_at.as_str()
                    && p.received_at == a.observed_at
                    && p.received_at.as_str() >= gw.clock_floor.as_str(),
                "RECEIPT_TIME",
            )?;
            let prefix = format!("gw1.{}.", gw.namespace.tag);
            require(
                p.delivery.0 == gw.namespace.scope
                    && p.delivery
                        .2
                        .as_str()
                        .strip_prefix(&prefix)
                        .is_some_and(|x| !x.is_empty() && x.len() <= 91),
                "NAMESPACE",
            )?;
            v.absent(&Point::delivery(&p.delivery)?)?;
            let route = hash("route", &p.submission.case)?;
            let mut remainder = 0usize;
            for b in route.as_str().bytes() {
                let n = if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
                remainder = (remainder * 16 + n as usize) % e.terms.gateways.len();
            }
            require(
                e.terms.gateways[remainder].gateway == *i.host,
                "WRONG_OWNER",
            )?;
            let terms = e
                .terms
                .families
                .iter()
                .find(|f| f.key == p.submission.case.0)
                .ok_or_else(|| fail("UNKNOWN_FAMILY"))?;
            require(terms.source == p.submission.case.1, "SOURCE")?;
            for evidence in &p.submission.evidence {
                require(
                    r3::raw_sha256(&r3::proofs::decode_base64(&evidence.body, 4096)?)
                        == evidence.sha256,
                    "EVIDENCE_HASH",
                )?;
            }
            let submission = hash("submission", &p.submission)?;
            let cp = Point::case(&p.submission.case)?;
            let existing = v.read(&cp)?;
            require(
                existing
                    .as_ref()
                    .is_none_or(|s| matches!(s, State::Case(_))),
                "CASE_HEAD",
            )?;
            let receipt = if let Some(State::Case(existing)) = existing {
                require(existing.submission == submission, "CASE_CONFLICT")?;
                t.status = TokenStatus::Alias;
                d.funding.actual_receipt = false;
                existing.receipt
            } else {
                gw.receipt = gw.receipt.checked_add(one())?;
                let receipt = w::Receipt {
                    case: p.submission.case.clone(),
                    delivery: p.delivery.clone(),
                    submission: submission.clone(),
                    token: p.token.clone(),
                    gateway: i.host.clone(),
                    epoch: p.epoch,
                    position: gw.receipt,
                    received_at: p.received_at.clone(),
                    journal_head: i.prior_root.clone(),
                };
                v.put(
                    cp,
                    State::Case(Box::new(CaseState {
                        input: p.submission.clone(),
                        submission: submission.clone(),
                        receipt: receipt.clone(),
                        admission: None,
                        status: CaseStatus::Local,
                        revision: Count::ZERO,
                        signed: Atoms::new(0)?,
                    })),
                )?;
                v.put(
                    Point::position(PointKind::Receipt, i.host, gw.receipt)?,
                    State::Position(p.token.clone()),
                )?;
                t.status = TokenStatus::NewCase;
                receipt
            };
            t.receipt = Some(receipt.clone());
            t.delivery = Some(p.delivery.clone());
            t.submission = Some(p.submission.clone());
            d.funding.owner = owner("grant-local", &t.token.grant)?;
            v.put(
                Point::delivery(&p.delivery)?,
                State::Delivery(Box::new(super::DeliveryState {
                    delivery: p.delivery.clone(),
                    submission,
                    receipt: receipt.clone(),
                    token: p.token.clone(),
                    command: command_digest(i.command)?,
                })),
            )?;
            v.put(gateway(i.host)?, State::Gateway(Box::new(gw)))?;
            v.put(token(&p.token)?, State::Token(Box::new(t)))?;
            d.effects.push(w::Effect::Receipt { body: receipt });
            d.fact = Some((
                if d.funding.actual_receipt {
                    w::FactKind::Receipt
                } else {
                    w::FactKind::Alias
                },
                p.token.clone(),
            ));
        }
        w::Command::Import { payload: p, .. } => {
            let e = v.enrolled()?;
            let mut t = v.tok(&p.token)?;
            require(*i.host == e.terms.store && !t.imported, "TOKEN_STATE")?;
            let source = v.source(
                &p.proof,
                &[w::FactKind::Receipt, w::FactKind::Alias],
                &p.token,
            )?;
            require(p.proof.host == t.token.gateway, "PROOF_HOST")?;
            let receive: w::Receive = source.payload()?;
            let effects = source.effects()?;
            let receipt = match effects.as_slice() {
                [w::Effect::Receipt { body }] => body.clone(),
                _ => return Err(fail("RECEIPT_PROOF")),
            };
            require(
                receive.token == p.token
                    && receive.gateway == t.token.gateway
                    && receipt.case == receive.submission.case
                    && receipt.submission == hash("submission", &receive.submission)?,
                "RECEIPT_BINDING",
            )?;
            let cp = Point::case(&receipt.case)?;
            if source.object.kind == w::FactKind::Alias {
                let Some(State::Case(existing)) = v.read(&cp)? else {
                    return Err(fail("ORIGINAL_NOT_IMPORTED"));
                };
                require(
                    existing.admission.is_some() && existing.receipt == receipt,
                    "ORIGINAL_NOT_IMPORTED",
                )?;
                t.status = TokenStatus::Alias;
            } else {
                v.absent(&cp)?;
                let State::Family(f) = v.load(&Point::family(&receipt.case.0)?)? else {
                    return Err(fail("FAMILY"));
                };
                v.put(
                    cp,
                    State::Case(Box::new(CaseState {
                        input: receive.submission.clone(),
                        submission: receipt.submission.clone(),
                        receipt: receipt.clone(),
                        admission: Some(i.next_ordinal),
                        status: if f.unavailable {
                            CaseStatus::AdjustmentPending
                        } else {
                            CaseStatus::OrdinaryPending
                        },
                        revision: Count::ZERO,
                        signed: Atoms::new(0)?,
                    })),
                )?;
                v.put(
                    Point::position(PointKind::Receipt, &t.token.gateway, receipt.position)?,
                    State::Position(p.token.clone()),
                )?;
                t.status = TokenStatus::NewCase;
            }
            v.absent(&Point::delivery(&receive.delivery)?)?;
            v.put(
                Point::delivery(&receive.delivery)?,
                State::Delivery(Box::new(super::DeliveryState {
                    delivery: receive.delivery.clone(),
                    submission: receipt.submission.clone(),
                    receipt: receipt.clone(),
                    token: p.token.clone(),
                    command: hash(
                        "command",
                        &json!(["RECEIVE", receive.delivery, receive.submission]),
                    )?,
                })),
            )?;
            t.imported = true;
            t.receipt = Some(receipt);
            t.delivery = Some(receive.delivery);
            t.submission = Some(receive.submission);
            d.funding.owner = owner("token", &p.token)?;
            d.funding.actual_receipt = t.status == TokenStatus::NewCase;
            v.put(token(&p.token)?, State::Token(Box::new(t)))?;
        }
        _ => {
            terminal_indexes = Some(terminal::apply(v, &mut d, &work)?);
        }
    }
    let base = match kind {
        "PREPARE_ENROLL" => 40,
        "ENROLL" => {
            let w::Command::Enroll { payload: p, .. } = i.command else {
                unreachable!()
            };
            4 + p.families.len() + 2 * p.gateways.len() + p.suppliers.len() + p.pools.len() + 32 + 1
        }
        "LOCAL_GRANT" | "REGISTER_GRANT" | "ACTIVATE" => 6,
        "ISSUE" => 8,
        "RECEIVE" => {
            if d.funding.actual_receipt {
                7
            } else {
                5
            }
        }
        "IMPORT" => {
            if d.funding.actual_receipt {
                7
            } else {
                6
            }
        }
        _ => terminal_indexes.ok_or_else(|| fail("INDEX_TEMPLATE"))?,
    };
    if !prepaid {
        charge(v, &d, &work, kind, base)?;
    }
    Ok(d)
}

fn charge(v: &mut View<'_>, d: &Delta, work: &Worksheet, kind: &str, base: usize) -> Result<()> {
    let i = v.input;
    let ap = account(i.host)?;
    let op = allocation(&d.funding.owner)?;
    let State::Resource(mut resources) = v.load(&ap)? else {
        return Err(fail("RESOURCE_HOST"));
    };
    let State::Allocation(mut owned) = v.load(&op)? else {
        return Err(fail("UNFUNDED"));
    };
    let mut actual = work.template(kind)?.counters()?;
    if kind == "RECEIVE" && !d.funding.actual_receipt {
        actual.receipt = Count::ZERO;
    }
    let cached = usize::from(
        matches!(kind, "SEAL_BEGIN" | "INSTALL")
            && i.observations
                .iter()
                .any(|o| matches!(o.state, Some(State::Preparation(_))))
            && v.pending
                .values()
                .any(|s| matches!(s, State::Enrollment(_))),
    );
    actual.index_cardinality = Count::new((base + i.introduced_objects + cached) as u128)?;
    resources.spend(&mut owned, kind, &actual, work)?;
    if d.funding.release {
        resources.terminal_slack(&mut owned, work)?;
    }
    v.put(ap, State::Resource(resources))?;
    v.put(op, State::Allocation(owned))?;
    Ok(())
}

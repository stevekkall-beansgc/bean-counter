//! Decisions use a bounded family topology and point-local signed capacity.
//! Corrections retain entitlement and pool usage and emit exact inverse/replacement.
use super::*;
fn family(v: &mut impl EconomicPoints, key: &w::Family) -> Result<FamilyState> {
    match v.load(&Point::family(key)?)? {
        State::Family(f) => Ok(*f),
        _ => Err(fail("FAMILY")),
    }
}
fn consume(f: &mut FamilyState, c: &CaseState) -> Result<()> {
    f.entitlement = w::EntitlementHead::Consumed {
        case: Box::new(c.input.case.clone()),
        revision: c.revision,
        head: hash(
            "result",
            &json!({"case":c.input.case,"state":"FINAL_ALLOW","revision":c.revision,"signed":c.signed,"receipt":hash("receipt",&c.receipt)?}),
        )?,
    };
    Ok(())
}
fn action(
    c: &CaseState,
    f: &FamilyState,
    kind: w::ActionKind,
    n: Atoms,
    roles: &w::Roles,
    assent: &Digest,
) -> Option<w::Effect> {
    if n.value() == 0 {
        return None;
    }
    Some(w::Effect::Action {
        body: w::Action {
            book: if f.terms.book == w::FamilyTermsBook::Retail {
                w::ActionBook::Retail
            } else {
                w::ActionBook::Supplier
            },
            case: c.input.case.clone(),
            revision: c.revision,
            kind,
            signed_atoms: n,
            magnitude: Count::new(n.value().unsigned_abs()).expect("bounded atoms"),
            roles: roles.clone(),
            assent: assent.clone(),
        },
    })
}
trait EconomicPoints {
    fn enrolled(&mut self) -> Result<EnrollmentState>;
    fn load(&mut self, point: &Point) -> Result<State>;
    fn put(&mut self, point: Point, state: State) -> Result<()>;
}
impl EconomicPoints for View<'_> {
    fn enrolled(&mut self) -> Result<EnrollmentState> {
        View::enrolled(self)
    }
    fn load(&mut self, point: &Point) -> Result<State> {
        View::load(self, point)
    }
    fn put(&mut self, point: Point, state: State) -> Result<()> {
        View::put(self, point, state)
    }
}
pub(super) fn apply(v: &mut View<'_>, d: &mut Delta, work: &Worksheet) -> Result<usize> {
    let command = v.input.command;
    let host = v.input.host;
    let indices = business(v, command, host, &mut d.effects)?;
    v.reserve(
        &d.funding.owner,
        std::slice::from_ref(&d.funding.slot),
        work,
    )?;
    d.funding.release = true;
    Ok(indices)
}
fn business(
    v: &mut impl EconomicPoints,
    command: &w::Command,
    host: &Id,
    effects: &mut Vec<w::Effect>,
) -> Result<usize> {
    let enrollment = v.enrolled()?;
    require(enrollment.terms.store == *host, "CENTRAL_HOST")?;
    let (key, authority) = match command {
        w::Command::Decide {
            payload, authority, ..
        } => (&payload.case, authority),
        w::Command::Correct {
            payload, authority, ..
        } => (&payload.case, authority),
        _ => return Err(fail("ECONOMIC_COMMAND")),
    };
    let mut c = match v.load(&Point::case(key)?)? {
        State::Case(c) => *c,
        _ => return Err(fail("CASE")),
    };
    let mut f = family(v, &key.0)?;
    let now = &authority.observed_at;
    match command {
        w::Command::Decide { payload: p, .. } => {
            let status = c.effective_status(&f);
            require(
                matches!(
                    status,
                    CaseStatus::OrdinaryPending | CaseStatus::AdjustmentPending
                ),
                "CASE_FINAL",
            )?;
            require(
                c.receipt.received_at.as_str() <= now.as_str(),
                "DECISION_TIME",
            )?;
            if p.verdict == w::DecideVerdict::Deny {
                c.status = CaseStatus::FinalDeny;
            } else {
                require(
                    matches!(f.entitlement, w::EntitlementHead::Unconsumed {}),
                    "ENTITLEMENT",
                )?;
                if p.path == w::DecidePath::Ordinary {
                    require(
                        status == CaseStatus::OrdinaryPending && !f.unavailable,
                        "ORDINARY_CLOSED",
                    )?;
                    require(
                        p.signed_atoms == f.terms.ordinary_atoms
                            && p.roles == f.terms.roles
                            && p.assent == f.terms.assent,
                        "ORIGINAL_TERMS",
                    )?;
                    let mut ordinary = Count::ZERO;
                    // Original topology is <=32. No case/action history is traversed.
                    for (index, terms) in enrollment.terms.families.iter().enumerate() {
                        let other = family(v, &terms.key)?;
                        if f.terms.prerequisites.contains(&(index as u64)) {
                            require(
                                matches!(other.entitlement, w::EntitlementHead::Consumed { .. }),
                                "PREREQUISITE",
                            )?;
                        }
                        ordinary = ordinary.checked_add(other.ordinary_positive)?;
                    }
                    require(
                        f.terms.starts_at.as_str() <= c.input.occurred_at.as_str()
                            && c.input.occurred_at.as_str() < f.terms.occurs_before.as_str()
                            && c.receipt.received_at.as_str() <= f.terms.received_by.as_str()
                            && now.as_str() <= f.terms.accepted_by.as_str(),
                        "WINDOW",
                    )?;
                    if f.terms.book == w::FamilyTermsBook::Retail {
                        let positive = Count::new(p.signed_atoms.value().max(0) as u128)?;
                        require(
                            ordinary.checked_add(positive)? <= enrollment.terms.premium_cap,
                            "PREMIUM_CAP",
                        )?;
                        f.ordinary_positive = positive;
                    } else {
                        require(p.signed_atoms.value() >= 0, "SUPPLIER_CAPACITY")?;
                        let point = id(
                            PointKind::Supplier,
                            *b"SUPPLIER",
                            f.terms.supplier_pool.as_str(),
                        )?;
                        let State::Supplier(mut supplier) = v.load(&point)? else {
                            return Err(fail("SUPPLIER"));
                        };
                        let amount = Count::new(p.signed_atoms.value() as u128)?;
                        require(amount <= supplier.held, "SUPPLIER_CAPACITY")?;
                        supplier.held = supplier.held.checked_sub(amount)?;
                        supplier.consumed = supplier.consumed.checked_add(amount)?;
                        v.put(point, State::Supplier(supplier))?;
                    }
                } else {
                    require(
                        status == CaseStatus::AdjustmentPending && f.unavailable,
                        "ADJUSTMENT_PATH",
                    )?;
                    let point = id(PointKind::Adjustment, *b"ADJPOOL_", p.pool.as_str())?;
                    let State::Adjustment(mut pool) = v.load(&point)? else {
                        return Err(fail("ADJUSTMENT_POOL"));
                    };
                    let n = p.signed_atoms.value();
                    let direction = if n > 0 {
                        w::PoolAuthorizationDirection::Positive
                    } else if n < 0 {
                        w::PoolAuthorizationDirection::Negative
                    } else {
                        w::PoolAuthorizationDirection::Zero
                    };
                    require(
                        pool.terms.authorizations.iter().any(|a| {
                            a.direction == direction && a.roles == p.roles && a.assent == p.assent
                        }),
                        "ADJUSTMENT_AUTH",
                    )?;
                    let mag = Count::new(n.unsigned_abs())?;
                    pool.funding_used = pool.funding_used.checked_add(mag)?;
                    pool.gross_used = pool.gross_used.checked_add(mag)?;
                    pool.positive_used = pool
                        .positive_used
                        .checked_add(Count::new(n.max(0) as u128)?)?;
                    pool.negative_used = pool
                        .negative_used
                        .checked_add(Count::new((-n).max(0) as u128)?)?;
                    require(
                        pool.funding_used <= pool.terms.funding
                            && pool.gross_used <= pool.terms.gross
                            && pool.positive_used <= pool.terms.positive
                            && pool.negative_used <= pool.terms.negative,
                        "ADJUSTMENT_CAPACITY",
                    )?;
                    v.put(point, State::Adjustment(pool))?;
                }
                c.status = CaseStatus::FinalAllow;
                c.revision = one();
                c.signed = p.signed_atoms;
                c.admitted_roles = Some(p.roles.clone());
                consume(&mut f, &c)?;
                if let Some(a) = action(
                    &c,
                    &f,
                    if p.path == w::DecidePath::Ordinary {
                        w::ActionKind::Ordinary
                    } else {
                        w::ActionKind::Adjustment
                    },
                    p.signed_atoms,
                    &p.roles,
                    &p.assent,
                ) {
                    effects.push(a);
                }
                v.put(Point::family(&key.0)?, State::Family(Box::new(f)))?;
            }
        }
        w::Command::Correct { payload: p, .. } => {
            require(
                c.status == CaseStatus::FinalAllow && c.revision == p.expected_revision,
                "REVISION",
            )?;
            require(
                f.terms.correction_atoms.contains(&p.replacement)
                    && now.as_str() <= f.terms.correction_by.as_str(),
                "CORRECTION_TERMS",
            )?;
            require(
                c.admitted_roles.as_ref() == Some(&p.roles) && p.assent == f.terms.assent,
                "CORRECTION_AUTH",
            )?;
            require(
                matches!(&f.entitlement,w::EntitlementHead::Consumed{case,..} if **case==c.input.case),
                "ENTITLEMENT",
            )?;
            let old = c.signed;
            c.revision = c.revision.checked_add(one())?;
            c.signed = p.replacement;
            if let Some(a) = action(
                &c,
                &f,
                w::ActionKind::Inverse,
                Atoms::new(-old.value())?,
                &p.roles,
                &p.assent,
            ) {
                effects.push(a);
            }
            if let Some(a) = action(
                &c,
                &f,
                w::ActionKind::Replacement,
                p.replacement,
                &p.roles,
                &p.assent,
            ) {
                effects.push(a);
            }
            consume(&mut f, &c)?;
            v.put(Point::family(&key.0)?, State::Family(Box::new(f)))?;
        }
        _ => unreachable!(),
    }
    v.put(Point::case(key)?, State::Case(Box::new(c)))?;
    Ok(match command {
        w::Command::Decide { payload, .. } if payload.verdict == w::DecideVerdict::Deny => 6,
        w::Command::Decide { .. } => 9 + if effects.is_empty() { 0 } else { 2 },
        _ => 7 + effects.len(),
    })
}

/// Pure detached semantic evaluation for a SELECT-only historical reader. This
/// carries no resource reservation, commit capability or authoritative plan.
#[derive(Clone, Debug)]
pub enum EconomicReplay {
    Need(Vec<Point>),
    Checked {
        states: Vec<(Point, State)>,
        effects: Vec<w::Effect>,
    },
}
pub fn replay_economics(
    command: &w::Command,
    host: &Id,
    enrollment: &EnrollmentState,
    observations: &[(Point, State)],
) -> Result<EconomicReplay> {
    command.validate()?;
    require(observations.len() <= 256, "RESOLUTION_BOUND")?;
    struct Detached<'a> {
        enrollment: &'a EnrollmentState,
        observations: &'a [(Point, State)],
        pending: BTreeMap<Point, State>,
        missing: Vec<Point>,
    }
    impl EconomicPoints for Detached<'_> {
        fn enrolled(&mut self) -> Result<EnrollmentState> {
            Ok(self.enrollment.clone())
        }
        fn load(&mut self, point: &Point) -> Result<State> {
            if let Some(value) = self.pending.get(point) {
                return Ok(value.clone());
            }
            if let Some((_, value)) = self.observations.iter().find(|(p, _)| p == point) {
                return Ok(value.clone());
            }
            self.missing.push(point.clone());
            Err(fail("NEED_POINT"))
        }
        fn put(&mut self, point: Point, state: State) -> Result<()> {
            self.load(&point)?;
            self.pending.insert(point, state);
            Ok(())
        }
    }
    let mut view = Detached {
        enrollment,
        observations,
        pending: BTreeMap::new(),
        missing: vec![],
    };
    let mut effects = vec![];
    match business(&mut view, command, host, &mut effects) {
        Ok(_) => Ok(EconomicReplay::Checked {
            states: view.pending.into_iter().collect(),
            effects,
        }),
        Err(e) if e.code == "NEED_POINT" => Ok(EconomicReplay::Need(view.missing)),
        Err(e) => Err(e),
    }
}

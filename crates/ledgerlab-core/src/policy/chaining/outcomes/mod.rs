//! Approved v0 outcome adjustments. Typed economic values only: no canonical
//! encoding, receipt, persistence, authentication or assent implementation.
//!
//! Supply complete committed history under coordinator locks. Freeze a Target
//! in the SAME acceptance as its final base rating; never attach terms later.
use super::compile::require;
use super::evaluate::is_reversed;
use super::{Binding, Book, Evaluation, Operation};
use crate::domain::{prefixed, slug, text, validate_source, Revision, Scope, Timestamp};
use crate::money::{add_atoms, ExactRatio, Money};
use crate::wire::Completion;
use crate::{Error, Result};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Amount {
    Fixed(Money),
    /// Signed exact percentage (25/1 means 25 percent), never binary float.
    Percent(ExactRatio),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub code: String,
    pub amount: Amount,
}
/// All endpoints are frozen at target acceptance. Occurrence is half-open;
/// receipt/acceptance deadlines are inclusive. Corrections use their own window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub starts_at: Timestamp,
    pub occurs_before: Timestamp,
    pub received_by: Timestamp,
    pub accepted_by: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub family: String,
    pub binding_id: String,
    pub source: String,
    pub correction_source: String,
    pub evidence_required: bool,
    pub ordinary: Window,
    pub corrections: Window,
    pub codes: Vec<Code>,
    pub replacement_codes: Vec<String>,
    pub allow_reversal: bool,
}
/// Separate finite premium ceilings per obligation; every used binding needs one,
/// including discount-only and zero policies. Discount capacity is the base net
/// in that binding. Retail and supplier capacities never offset one another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limit {
    pub binding_id: String,
    pub premium: Money,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub version: String,
    pub document: String,
    pub families: Vec<Family>,
    pub limits: Vec<Limit>,
}
/// Narrow coordinator observations, NOT evidence verification or assent tokens.
/// The coordinator must retain and verify referenced documents and freeze these
/// exact terms as part of accepting the base. Public ingress must not set flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetVerification {
    pub rated_final: bool,
    pub accepted_at: Timestamp,
    pub policy_document: String,
    pub verified_assents: Vec<String>,
    pub verified_offers: Vec<String>,
    pub verified_delegations: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Target {
    base: Evaluation,
    policy: Policy,
    verification: TargetVerification,
    retail_basis: Money,
}
impl Target {
    pub fn freeze(
        base: &Evaluation,
        policy: Policy,
        verification: TargetVerification,
    ) -> Result<Self> {
        require(
            base.event.dto().kind.is_work()
                && base.event.dto().status == Some(Completion::Succeeded),
            "TARGET_INELIGIBLE",
            "explicit successful base work required",
        )?;
        require(
            verification.rated_final,
            "TARGET_NOT_FINAL",
            "final rating required",
        )?;
        require(
            !base
                .bundle
                .policies
                .iter()
                .flat_map(|p| &p.rules)
                .any(|r| matches!(r.operation, Operation::Cap { .. })),
            "OUTCOME_CAP_COMPOSITION",
            "capped pricing and outcome adjustments cannot coexist",
        )?;
        require(
            base.received_at
                .as_ref()
                .is_some_and(|r| r.micros() <= verification.accepted_at.micros()),
            "INVALID_ACCEPTED_ORDER",
            "base receipt precedes acceptance",
        )?;
        let occurred = base
            .event
            .dto()
            .occurred_at
            .as_ref()
            .ok_or_else(|| Error::new("TARGET_INELIGIBLE", "base occurrence required"))?;
        require(
            occurred.micros() <= base.received_at.as_ref().expect("checked").micros(),
            "INVALID_RECEIVED_ORDER",
            "base occurrence precedes receipt",
        )?;
        text(&policy.version, 128)?;
        prefixed(&policy.document, "doc_")?;
        require(
            policy.document == verification.policy_document,
            "TERMS_NOT_VERIFIED",
            "verified frozen policy required",
        )?;
        require(
            !policy.families.is_empty() && policy.families.len() <= 32 && policy.limits.len() <= 16,
            "POLICY_LIMIT",
            "outcome families and limits",
        )?;
        for documents in [
            &verification.verified_assents,
            &verification.verified_offers,
            &verification.verified_delegations,
        ] {
            require(documents.len() <= 16, "LIMIT", "verified terms references")?;
            let mut unique = BTreeSet::new();
            for document in documents {
                prefixed(document, "doc_")?;
                require(
                    unique.insert(document),
                    "TERMS_NOT_VERIFIED",
                    "unique verified references",
                )?;
            }
        }
        let mut keys = BTreeSet::new();
        for family in &policy.families {
            slug(&family.family)?;
            validate_source(&family.source)?;
            validate_source(&family.correction_source)?;
            let binding = find_binding(base, &family.binding_id)?;
            require(
                keys.insert((&binding.agreement, &family.family)),
                "POLICY_AMBIGUOUS_MATCH",
                "stable agreement/family",
            )?;
            require(
                verification.verified_assents.contains(&binding.assent),
                "TERMS_NOT_VERIFIED",
                "binding assent must be verified",
            )?;
            require(
                binding.sources.contains(&family.source)
                    && binding
                        .correction_sources
                        .contains(&family.correction_source),
                "OUTCOME_AUTHORITY",
                "family sources inside frozen binding authority",
            )?;
            if let Some(delegation) = binding.roles.payer_delegation() {
                require(
                    verification
                        .verified_delegations
                        .iter()
                        .any(|d| d == delegation),
                    "TERMS_NOT_VERIFIED",
                    "payer delegation verified",
                )?;
            }
            if let Some(offer) = &binding.offer {
                require(
                    verification.verified_offers.contains(offer),
                    "TERMS_NOT_VERIFIED",
                    "supplier offer verified",
                )?;
            }
            require(
                binding.book == Book::Retail || binding.book == Book::Supplier,
                "POLICY_UNSUPPORTED_OP",
                "separate payable binding required",
            )?;
            // Supplier adjustments can target only the supplier's actual authorized
            // completion, never an unrelated invocation found elsewhere in a chain.
            if binding.book == Book::Supplier {
                require(
                    base.invocations.iter().any(|i| {
                        i.binding_id == binding.id
                            && i.operation_id == base.event.candidate().operation_id()
                            && i.source == base.event.source()
                            && base.event.dto().invocation_id.as_deref() == Some(&i.id)
                            && base.event.dto().binding_id.as_deref() == Some(&binding.id)
                    }),
                    "SUPPLIER_TARGET_PATH",
                    "target is the nominated supplier completion",
                )?;
            }
            require(
                base.bundle.policies.iter().any(|p| {
                    p.binding.id == binding.id
                        && p.rules.iter().any(|r| r.on == base.event.dto().kind)
                }),
                "TARGET_INELIGIBLE",
                "binding rated this base",
            )?;
            for window in [&family.ordinary, &family.corrections] {
                require(
                    window.starts_at.micros() >= occurred.micros()
                        && window.starts_at.micros() < window.occurs_before.micros()
                        && window.occurs_before.micros() <= window.received_by.micros()
                        && window.received_by.micros() <= window.accepted_by.micros(),
                    "OUTCOME_WINDOW",
                    "ordered frozen deadlines",
                )?;
            }
            require(
                !family.codes.is_empty()
                    && family.codes.len() <= 32
                    && family.replacement_codes.len() <= 32,
                "POLICY_LIMIT",
                "outcome codes",
            )?;
            let mut codes = BTreeSet::new();
            for code in &family.codes {
                slug(&code.code)?;
                require(
                    codes.insert(&code.code),
                    "POLICY_AMBIGUOUS_MATCH",
                    "outcome code",
                )?;
                if let Amount::Fixed(m) = &code.amount {
                    same_currency(base, m)?;
                }
            }
            let mut replacements = BTreeSet::new();
            for code in &family.replacement_codes {
                require(
                    codes.contains(code) && replacements.insert(code),
                    "POLICY_OUTCOME_CODE",
                    "permitted replacement code",
                )?;
            }
        }
        let mut limits = BTreeSet::new();
        for limit in &policy.limits {
            same_currency(base, &limit.premium)?;
            let binding = find_binding(base, &limit.binding_id)?;
            if binding.book == Book::Supplier {
                let net = booked_net(base, &binding.id)?;
                require(
                    base.invocations
                        .iter()
                        .filter(|i| i.binding_id == binding.id)
                        .all(|i| {
                            i.outcome_deadline.is_some()
                                && limit.premium.atoms() <= i.held.atoms() - net
                        }),
                    "EXPOSURE_EXCEEDED",
                    "frozen supplier premium fits contingent held capacity",
                )?;
                require(
                    add_atoms(net, limit.premium.atoms())?
                        <= binding.maximum_exposure.as_ref().expect("compiled").atoms(),
                    "EXPOSURE_EXCEEDED",
                    "supplier binding exposure",
                )?;
            }
            require(
                limit.premium.atoms() >= 0 && limits.insert(&limit.binding_id),
                "POLICY_LIMIT",
                "unique finite nonnegative premium ceiling",
            )?;
        }
        require(
            policy
                .families
                .iter()
                .all(|f| limits.contains(&f.binding_id)),
            "PREMIUM_BOUND_REQUIRED",
            "every binding needs a finite ceiling",
        )?;
        let retail = base
            .bundle
            .policies
            .iter()
            .find(|p| p.binding.book == Book::Retail)
            .expect("compiled");
        let net = booked_net(base, &retail.binding.id)?;
        Ok(Self {
            retail_basis: Money::new(&base.bundle.currency, base.bundle.scale, net)?,
            base: base.clone(),
            policy,
            verification,
        })
    }
    pub fn base(&self) -> &Evaluation {
        &self.base
    }
    pub fn policy(&self) -> &Policy {
        &self.policy
    }
    pub fn verification(&self) -> &TargetVerification {
        &self.verification
    }
    pub fn retail_basis(&self) -> &Money {
        &self.retail_basis
    }
}
fn find_binding<'a>(base: &'a Evaluation, id: &str) -> Result<&'a Binding> {
    base.bundle
        .policies
        .iter()
        .map(|p| &p.binding)
        .find(|b| b.id == id)
        .ok_or_else(|| Error::new("TERMS_NOT_ACCEPTED", "target binding"))
}
fn same_currency(base: &Evaluation, money: &Money) -> Result<()> {
    require(
        money.currency() == base.bundle.currency && money.scale() == base.bundle.scale,
        "POLICY_CURRENCY",
        "target money",
    )
}
fn booked_net(base: &Evaluation, binding: &str) -> Result<i128> {
    let net = base
        .actions
        .iter()
        .filter(|a| a.binding.id == binding)
        .try_fold(0, |n, a| add_atoms(n, a.amount.atoms()))?;
    require(net >= 0, "NEGATIVE_BASIS", "booked obligation net")?;
    Ok(net)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Claim {
        code: String,
    },
    /// None means full reversal; Some is a policy-permitted replacement.
    Correct {
        expected_revision: u64,
        replacement: Option<String>,
    },
}
/// Deliberately contains no operative amount, percentage, basis or policy version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub scope: Scope,
    pub id: String,
    pub target: String,
    pub agreement: String,
    pub family: String,
    pub source: String,
    pub occurred_at: Timestamp,
    pub evidence: Vec<String>,
    pub change: Change,
}
/// Authenticated, locked and verified observations supplied by the coordinator.
/// The exact scope/target/agreement/family binding also covers explicit link rights.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    pub scope: Scope,
    pub target: String,
    pub agreement: String,
    pub family: String,
    pub source: String,
    pub principal: String,
    pub grant: String,
    pub grant_revision: Revision,
    pub active: bool,
    pub may_read: bool,
    pub may_submit: bool,
    pub may_correct: bool,
    pub verified_evidence: Vec<String>,
    pub received_at: Timestamp,
    pub accepted_at: Timestamp,
}
/// Structured semantic identity; canonical encoding is a separate integration gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimKey {
    pub scope: Scope,
    pub agreement: String,
    pub family: String,
    pub target: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Posting {
    pub amount: Money,
    /// An inverse refers to the previous result revision, never a recomputed price.
    pub reverses_revision: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Explanation {
    pub code: &'static str,
    pub basis: Money,
    pub exact: ExactRatio,
    pub rounded: Money,
}
#[derive(Clone, Debug)]
pub struct Decision {
    target: Target,
    request: Request,
    verified: Verified,
    key: ClaimKey,
    revision: u64,
    current_code: Option<String>,
    current: Money,
    binding: Binding,
    postings: Vec<Posting>,
    explanations: Vec<Explanation>,
}
impl Decision {
    pub fn target(&self) -> &Target {
        &self.target
    }
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn verified(&self) -> &Verified {
        &self.verified
    }
    pub fn key(&self) -> &ClaimKey {
        &self.key
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn current_code(&self) -> Option<&str> {
        self.current_code.as_deref()
    }
    pub fn current(&self) -> &Money {
        &self.current
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn postings(&self) -> &[Posting] {
        &self.postings
    }
    pub fn explanations(&self) -> &[Explanation] {
        &self.explanations
    }
}
#[derive(Clone, Debug)]
pub enum Submission {
    Accepted(Box<Decision>),
    /// Index of the original immutable decision, even after subsequent correction.
    Duplicate(usize),
}
fn key(request: &Request) -> ClaimKey {
    ClaimKey {
        scope: request.scope.clone(),
        agreement: request.agreement.clone(),
        family: request.family.clone(),
        target: request.target.clone(),
    }
}
fn window(w: &Window, request: &Request, v: &Verified) -> Result<()> {
    require(
        request.occurred_at.micros() <= v.received_at.micros(),
        "INVALID_RECEIVED_ORDER",
        "occurrence precedes receipt",
    )?;
    require(
        v.received_at.micros() <= v.accepted_at.micros(),
        "INVALID_ACCEPTED_ORDER",
        "receipt precedes acceptance",
    )?;
    require(
        w.starts_at.micros() <= request.occurred_at.micros()
            && request.occurred_at.micros() < w.occurs_before.micros(),
        "OUTCOME_WINDOW",
        "half-open occurrence window",
    )?;
    require(
        v.received_at.micros() <= w.received_by.micros()
            && v.accepted_at.micros() <= w.accepted_by.micros(),
        "OUTCOME_DEADLINE",
        "frozen receipt and acceptance deadlines",
    )
}
/// Pure all-or-nothing proposal. Append Accepted only after durable commit.
/// `bases` is the complete locked base/reversal history; `history` is the complete
/// ordered outcome history. Never reconstruct either from totals or today's prices.
pub fn evaluate(
    request: &Request,
    verified: &Verified,
    targets: &[Target],
    bases: &[Evaluation],
    history: &[Decision],
) -> Result<Submission> {
    require(
        history.len() < 1000 && bases.len() <= 1000 && targets.len() <= 1000,
        "LIMIT",
        "bounded history",
    )?;
    validate_outcome_history(history)?;
    text(&request.id, 128)?;
    prefixed(&request.target, "ev_")?;
    text(&request.agreement, 128)?;
    slug(&request.family)?;
    validate_source(&request.source)?;
    text(&verified.principal, 128)?;
    prefixed(&verified.grant, "doc_")?;
    require(
        verified.active
            && verified.may_read
            && verified.scope == request.scope
            && verified.target == request.target
            && verified.agreement == request.agreement
            && verified.family == request.family
            && verified.source == request.source,
        "OUTCOME_AUTHORITY",
        "verified scoped principal, target and read rights required",
    )?;
    let key = key(request);
    // Identity retries precede current deadlines, finality and write permissions.
    if let Some((i, old)) = history.iter().enumerate().find(|(_, d)| {
        d.request.scope == request.scope
            && d.request.source == request.source
            && d.request.id == request.id
    }) {
        require(
            old.request == *request,
            "IDENTITY_CONFLICT",
            "same delivery different content",
        )?;
        return Ok(Submission::Duplicate(i));
    }
    let lineage: Vec<_> = history
        .iter()
        .enumerate()
        .filter(|(_, d)| d.key == key)
        .collect();
    if let Change::Claim { .. } = request.change {
        if let Some((i, original)) = lineage.first() {
            let mut comparable = request.clone();
            comparable.id = original.request.id.clone();
            require(
                comparable == original.request,
                "CLAIM_CONFLICT",
                "permanent family claim",
            )?;
            return Ok(Submission::Duplicate(*i));
        }
    }
    // Once any family exists, its original target snapshot governs ALL families.
    let frozen = history
        .iter()
        .find(|d| d.key.scope == key.scope && d.key.target == key.target)
        .map(|d| &d.target);
    let candidates: Vec<_> = targets
        .iter()
        .filter(|t| t.base.event.scope() == &request.scope && t.base.event.id() == request.target)
        .collect();
    require(
        candidates.len() <= 1,
        "TARGET_CONFLICT",
        "unique frozen target",
    )?;
    let target = frozen
        .or_else(|| candidates.first().copied())
        .ok_or_else(|| Error::new("WAITING_DEPENDENCIES", "eligible final base target"))?;
    let base = &target.base;
    require(
        bases.iter().any(|d| {
            d.event.id() == request.target
                && d.event.scope() == &request.scope
                && d.event.bytes() == base.event.bytes()
                && d.actions == base.actions
        }),
        "WAITING_DEPENDENCIES",
        "committed base history",
    )?;
    require(
        !bases.iter().any(|d| {
            d.event.scope() == &request.scope
                && d.event
                    .dto()
                    .targets
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&request.target))
        }) && !base.actions.iter().any(|a| is_reversed(&a.id, bases)),
        "TARGET_REVERSED",
        "active successful work required",
    )?;
    let family = target
        .policy
        .families
        .iter()
        .find(|f| {
            f.family == request.family
                && find_binding(base, &f.binding_id).is_ok_and(|b| b.agreement == request.agreement)
        })
        .ok_or_else(|| Error::new("POLICY_OUTCOME_FAMILY", "predeclared stable family"))?;
    let binding = find_binding(base, &family.binding_id)?;
    require(
        request.evidence.len() <= 16 && verified.verified_evidence.len() <= 16,
        "LIMIT",
        "retained evidence",
    )?;
    let mut evidence = BTreeSet::new();
    for doc in &request.evidence {
        prefixed(doc, "doc_")?;
        require(
            evidence.insert(doc) && verified.verified_evidence.contains(doc),
            "OUTCOME_EVIDENCE_REQUIRED",
            "each retained reference must be verified",
        )?;
    }
    require(
        !family.evidence_required || !request.evidence.is_empty(),
        "OUTCOME_EVIDENCE_REQUIRED",
        "frozen evidence requirement",
    )?;
    require(
        verified.accepted_at.micros() >= target.verification.accepted_at.micros(),
        "INVALID_ACCEPTED_ORDER",
        "base accepted first",
    )?;
    let previous = lineage.last().map(|(_, d)| *d);
    let code = match &request.change {
        Change::Claim { code } => {
            require(
                verified.may_submit && request.source == family.source,
                "OUTCOME_AUTHORITY",
                "nominated ordinary source",
            )?;
            window(&family.ordinary, request, verified)?;
            Some(code.clone())
        }
        Change::Correct {
            expected_revision,
            replacement,
        } => {
            require(
                verified.may_correct && request.source == family.correction_source,
                "CORRECTION_UNAUTHORIZED",
                "separate correction authorization",
            )?;
            let prior =
                previous.ok_or_else(|| Error::new("CLAIM_MISSING", "correction lineage"))?;
            require(
                *expected_revision == prior.revision,
                "STALE_CORRECTION",
                "current revision required",
            )?;
            require(
                verified.accepted_at.micros() >= prior.verified.accepted_at.micros(),
                "INVALID_ACCEPTED_ORDER",
                "correction follows current revision",
            )?;
            window(&family.corrections, request, verified)?;
            require(
                replacement.as_ref().map_or(family.allow_reversal, |c| {
                    family.replacement_codes.contains(c)
                }),
                "CORRECTION_NOT_PERMITTED",
                "frozen replacement/reversal permission",
            )?;
            replacement.clone()
        }
    };
    let exact = match &code {
        None => ExactRatio::integer(0),
        Some(code) => match &family
            .codes
            .iter()
            .find(|c| c.code == *code)
            .ok_or_else(|| Error::new("POLICY_OUTCOME_CODE", "predeclared outcome code"))?
            .amount
        {
            Amount::Fixed(m) => ExactRatio::integer(m.atoms()),
            Amount::Percent(p) => ExactRatio::integer(target.retail_basis.atoms())
                .mul(p)?
                .div(&ExactRatio::integer(100))?,
        },
    };
    let current = Money::new(
        &base.bundle.currency,
        base.bundle.scale,
        exact.round_atoms()?,
    )?;
    // Evaluate the prospective ACTIVE results, separately summing discounts and
    // premiums. A positive fee never buys more discount capacity, or vice versa.
    let mut discount = current.atoms().min(0);
    let mut premium = current.atoms().max(0);
    for (i, d) in history.iter().enumerate() {
        if d.key.scope != key.scope
            || d.key.target != key.target
            || d.key == key
            || d.binding.id != binding.id
            || history[i + 1..].iter().any(|later| later.key == d.key)
        {
            continue;
        }
        discount = add_atoms(discount, d.current.atoms().min(0))?;
        premium = add_atoms(premium, d.current.atoms().max(0))?;
    }
    require(
        -discount <= booked_net(base, &binding.id)?,
        "DISCOUNT_EXCEEDS_BASIS",
        "aggregate active discounts",
    )?;
    let limit = target
        .policy
        .limits
        .iter()
        .find(|l| l.binding_id == binding.id)
        .expect("frozen limit");
    require(
        premium <= limit.premium.atoms(),
        "PREMIUM_LIMIT",
        "aggregate active premiums",
    )?;
    add_atoms(
        add_atoms(booked_net(base, &binding.id)?, discount)?,
        premium,
    )?;
    let mut postings = Vec::new();
    let mut explanations = Vec::new();
    if let Some(previous) = previous {
        let inverse = Money::new(
            &base.bundle.currency,
            base.bundle.scale,
            -previous.current.atoms(),
        )?;
        if inverse.atoms() != 0 {
            postings.push(Posting {
                amount: inverse.clone(),
                reverses_revision: Some(previous.revision),
            });
        }
        explanations.push(Explanation {
            code: "EXACT_REVERSAL",
            basis: target.retail_basis.clone(),
            exact: ExactRatio::integer(inverse.atoms()),
            rounded: inverse,
        });
    }
    if current.atoms() != 0 {
        postings.push(Posting {
            amount: current.clone(),
            reverses_revision: None,
        });
    }
    explanations.push(Explanation {
        code: if code.is_none() {
            "CLAIM_REVERSED"
        } else if current.atoms() == 0 {
            "ZERO_ROUNDED"
        } else {
            "OUTCOME_APPLIED"
        },
        basis: target.retail_basis.clone(),
        exact,
        rounded: current.clone(),
    });
    // Bound atomic delta as well as the active result.
    postings
        .iter()
        .try_fold(0, |n, p| add_atoms(n, p.amount.atoms()))?;
    Ok(Submission::Accepted(Box::new(Decision {
        target: target.clone(),
        request: request.clone(),
        verified: verified.clone(),
        key,
        revision: previous.map_or(1, |p| p.revision + 1),
        current_code: code,
        current,
        binding: binding.clone(),
        postings,
        explanations,
    })))
}

fn validate_outcome_history(history: &[Decision]) -> Result<()> {
    require(history.len() < 1000, "LIMIT", "bounded outcome history")?;
    for (i, decision) in history.iter().enumerate() {
        let previous = history[..i].iter().rev().find(|d| d.key == decision.key);
        require(
            decision.revision == previous.map_or(1, |d| d.revision + 1)
                && !history[..i].iter().any(|d| {
                    d.request.scope == decision.request.scope
                        && d.request.source == decision.request.source
                        && d.request.id == decision.request.id
                }),
            "HISTORY_CONFLICT",
            "ordered unique committed claim revisions required",
        )?;
    }
    Ok(())
}

/// Guard the base reversal boundary when outcomes are enabled. Every active
/// monetary result must first be reversed through its own authorized lineage.
/// The coordinator must lock both histories and commit their proposals atomically.
pub fn reverse_base(
    event: &crate::domain::Event,
    bases: &[Evaluation],
    outcomes: &[Decision],
    authority: &super::SourceAuthority,
) -> Result<Evaluation> {
    validate_outcome_history(outcomes)?;
    if let Some(targets) = &event.dto().targets {
        for (i, d) in outcomes.iter().enumerate() {
            if d.key.scope == *event.scope()
                && targets.contains(&d.key.target)
                && !outcomes[i + 1..].iter().any(|later| later.key == d.key)
            {
                require(
                    d.current.atoms() == 0,
                    "REVERSAL_DEPENDENTS_REQUIRED",
                    "reverse active outcome results before reversing their base",
                )?;
            }
        }
    }
    super::reverse(event, bases, authority)
}

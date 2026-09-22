//! Internal, nonposting Phase 4 arithmetic. The facade must first authenticate
//! scoped reads and verify the complete retained history and terminal snapshot.
//! This module cannot establish that trust. No candidate becomes a Target,
//! Decision, canonical record, authority observation, or acceptance command.
use super::{compile::require, outcomes as o, Book};
use crate::domain::{text, Roles, Scope};
use crate::money::{add_atoms, ExactRatio, Money};
use crate::{Error, Result};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub use super::comparison_provenance::{provenance_digest, ALGORITHM};
pub const ASSUMPTION: &str =
    "Amounts substituted for comparison; no assent, eligibility change or posting authorized.";
const MAX_STEPS: usize = 999;
const MAX_CANDIDATE_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComparisonProvenance {
    pub snapshot: String,
    pub activity: String,
    pub semantics: String,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FamilyKey {
    pub agreement: String,
    pub family: String,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AmountKey {
    pub agreement: String,
    pub family: String,
    pub code: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateAmount {
    pub key: AmountKey,
    pub amount: o::Amount,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmountCandidate {
    pub key: String,
    pub amounts: Vec<CandidateAmount>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityEntry {
    Outcome { decision_index: usize },
    Close { binding_id: String },
}
/// Original registration checkpoint, before any outcome or closure. The facade
/// verifies its retained provenance; numeric conservation is rechecked here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReservationBasis {
    pub binding_id: String,
    pub maximum: Money,
    pub consumed: Money,
    pub held: Money,
    pub released: Money,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingBasis {
    pub binding_id: String,
    pub agreement: String,
    pub book: Book,
    pub roles: Roles,
    pub booked_net: Money,
    pub premium_limit: Money,
}
#[derive(Clone, Debug)]
struct Family {
    key: FamilyKey,
    binding: usize,
    codes: Vec<o::Code>,
    replacements: Vec<String>,
    allow_reversal: bool,
}
#[derive(Clone, Debug)]
struct EconomicStep {
    family: usize,
    change: o::Change,
    revision: u64,
    booked: i128,
    booked_delta: i128,
}
/// Detached immutable numeric projection. No raw candidate can construct this
/// through public fields. Retained bytes/evidence remain in the facade workspace.
#[derive(Clone, Debug)]
pub struct ComparisonActivity {
    provenance: ComparisonProvenance,
    scope: Scope,
    target: String,
    historical_policy: String,
    historical_version: String,
    retail_basis: Money,
    bindings: Vec<BindingBasis>,
    families: Vec<Family>,
    economic: Vec<EconomicStep>,
    chronology: Vec<ActivityEntry>,
    reservations: Vec<ReservationBasis>,
}
impl ComparisonActivity {
    /// Input must be the original successfully verified retained material. This
    /// checks numeric coherence, not authentication, evidence, or completeness.
    pub fn from_history(
        target: &o::Target,
        decisions: &[o::Decision],
        chronology: &[ActivityEntry],
        reservations: &[ReservationBasis],
        provenance: ComparisonProvenance,
    ) -> Result<Self> {
        require(
            decisions.len() <= MAX_STEPS
                && chronology.len() <= MAX_STEPS
                && reservations.len() <= 16,
            "COMPARISON_LIMIT",
            "bounded complete chronology",
        )?;
        for value in [
            &provenance.snapshot,
            &provenance.activity,
            &provenance.semantics,
        ] {
            text(value, 256)?;
        }
        let base = target.base();
        require(
            base.event().dto().links.as_ref().is_none_or(Vec::is_empty)
                && base.event().dto().corrects.is_none(),
            "COMPARISON_UNSUPPORTED_HISTORY",
            "one final base without predecessor history",
        )?;
        let mut bindings = Vec::new();
        for policy in &base.bundle.policies {
            let b = &policy.binding;
            if !matches!(b.book, Book::Retail | Book::Supplier) {
                continue;
            }
            let net = base
                .actions()
                .iter()
                .filter(|a| a.binding().id == b.id)
                .try_fold(0, |n, a| add_atoms(n, a.amount().atoms()))?;
            require(net >= 0, "NEGATIVE_BASIS", "original booked net")?;
            bindings.push(BindingBasis {
                binding_id: b.id.clone(),
                agreement: b.agreement.clone(),
                book: b.book,
                roles: b.roles.clone(),
                booked_net: money(target.retail_basis(), net)?,
                premium_limit: target
                    .policy()
                    .limits
                    .iter()
                    .find(|l| l.binding_id == b.id)
                    .map(|l| l.premium.clone())
                    .unwrap_or(money(target.retail_basis(), 0)?),
            });
        }
        require(
            !bindings.is_empty() && bindings.len() <= 16,
            "COMPARISON_LIMIT",
            "binding count",
        )?;
        let mut families = Vec::new();
        for f in &target.policy().families {
            let binding = bindings
                .iter()
                .position(|b| b.binding_id == f.binding_id)
                .ok_or_else(|| invalid("frozen family binding"))?;
            families.push(Family {
                key: FamilyKey {
                    agreement: bindings[binding].agreement.clone(),
                    family: f.family.clone(),
                },
                binding,
                codes: f.codes.clone(),
                replacements: f.replacement_codes.clone(),
                allow_reversal: f.allow_reversal,
            });
        }
        let mut economic = Vec::new();
        for d in decisions {
            require(
                d.target().policy() == target.policy()
                    && d.key().scope == *base.event().scope()
                    && d.key().target == base.event().id()
                    && d.target().retail_basis() == target.retail_basis()
                    && d.target().base().actions() == base.actions(),
                "COMPARISON_HISTORY",
                "one original frozen target",
            )?;
            let family = families
                .iter()
                .position(|f| {
                    f.key.agreement == d.key().agreement && f.key.family == d.key().family
                })
                .ok_or_else(|| invalid("unknown original family"))?;
            require(
                bindings[families[family].binding].binding_id == d.binding().id,
                "COMPARISON_HISTORY",
                "original binding",
            )?;
            economic.push(EconomicStep {
                family,
                change: d.request().change.clone(),
                revision: d.revision(),
                booked: d.current().atoms(),
                booked_delta: d
                    .postings()
                    .iter()
                    .try_fold(0, |n, p| add_atoms(n, p.amount.atoms()))?,
            });
        }
        for r in reservations {
            let invocations: Vec<_> = base
                .invocations()
                .iter()
                .filter(|i| i.binding_id == r.binding_id)
                .collect();
            require(
                invocations.len() == 1,
                "COMPARISON_HISTORY",
                "one registered supplier invocation",
            )?;
            let i = invocations[0];
            let consumptions: Vec<_> = base
                .consumptions()
                .iter()
                .filter(|c| c.invocation_id == i.id)
                .collect();
            require(
                consumptions.len() == 1
                    && r.maximum == i.maximum_exposure
                    && r.consumed == consumptions[0].consume
                    && r.released == consumptions[0].release,
                "COMPARISON_HISTORY",
                "checkpoint matches original base consumption",
            )?;
        }
        let activity = Self {
            provenance,
            scope: base.event().scope().clone(),
            target: base.event().id().into(),
            historical_policy: target.policy().document.clone(),
            historical_version: target.policy().version.clone(),
            retail_basis: target.retail_basis().clone(),
            bindings,
            families,
            economic,
            chronology: chronology.to_vec(),
            reservations: reservations.to_vec(),
        };
        activity.validate()?;
        // The resumable driver verifies original numeric parity first, before
        // exposing a completed comparison. Projection does not price candidates.
        Ok(activity)
    }
    pub fn provenance(&self) -> &ComparisonProvenance {
        &self.provenance
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn historical_policy(&self) -> &str {
        &self.historical_policy
    }
    pub fn historical_version(&self) -> &str {
        &self.historical_version
    }
    pub fn retail_basis(&self) -> &Money {
        &self.retail_basis
    }
    pub fn bindings(&self) -> &[BindingBasis] {
        &self.bindings
    }
    pub fn original_amounts(&self) -> Vec<CandidateAmount> {
        self.families
            .iter()
            .flat_map(|f| {
                f.codes.iter().map(|c| CandidateAmount {
                    key: AmountKey {
                        agreement: f.key.agreement.clone(),
                        family: f.key.family.clone(),
                        code: c.code.clone(),
                    },
                    amount: c.amount.clone(),
                })
            })
            .collect()
    }
    fn validate(&self) -> Result<()> {
        require(
            !self.families.is_empty() && self.families.len() <= 32 && self.reservations.len() <= 16,
            "COMPARISON_LIMIT",
            "family/reservation count",
        )?;
        let mut seen = BTreeSet::new();
        for r in &self.reservations {
            require(
                seen.insert(&r.binding_id),
                "COMPARISON_HISTORY",
                "unique reservation",
            )?;
            let b = self
                .bindings
                .iter()
                .find(|b| b.binding_id == r.binding_id)
                .ok_or_else(|| invalid("reservation binding"))?;
            require(
                b.book == Book::Supplier,
                "COMPARISON_HISTORY",
                "supplier reservation",
            )?;
            for m in [&r.maximum, &r.consumed, &r.held, &r.released] {
                same_unit(&self.retail_basis, m)?;
                require(
                    m.atoms() >= 0,
                    "COMPARISON_HISTORY",
                    "nonnegative checkpoint",
                )?;
            }
            require(
                add_atoms(
                    add_atoms(r.consumed.atoms(), r.held.atoms())?,
                    r.released.atoms(),
                )? == r.maximum.atoms()
                    && b.premium_limit.atoms() <= r.held.atoms(),
                "COMPARISON_HISTORY",
                "checkpoint conservation and frozen ceiling",
            )?;
        }
        for b in &self.bindings {
            if b.book == Book::Supplier
                && self
                    .families
                    .iter()
                    .any(|f| self.bindings[f.binding].binding_id == b.binding_id)
            {
                require(
                    seen.contains(&b.binding_id),
                    "COMPARISON_HISTORY",
                    "supplier checkpoint required",
                )?;
            }
        }
        let mut next = 0;
        for entry in &self.chronology {
            match entry {
                ActivityEntry::Outcome { decision_index } => {
                    require(
                        *decision_index == next && next < self.economic.len(),
                        "COMPARISON_HISTORY",
                        "complete original decision order",
                    )?;
                    next += 1;
                }
                ActivityEntry::Close { binding_id } => {
                    require(
                        seen.contains(binding_id),
                        "COMPARISON_HISTORY",
                        "closure reservation",
                    )?;
                }
            }
        }
        require(
            next == self.economic.len(),
            "COMPARISON_HISTORY",
            "no omitted outcome",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingEstimate {
    pub binding_id: String,
    pub premium: i128,
    /// Signed negative aggregate, separate from premiums.
    pub discount: i128,
    pub adjustment: i128,
    pub final_net: i128,
    pub difference_from_booked: i128,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyEstimate {
    pub key: FamilyKey,
    /// None means not observed; Some(0) is a claimed zero or reversed result.
    pub revision: Option<u64>,
    pub current_code: Option<String>,
    pub amount: Option<i128>,
    pub ordinary_closed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReservationEstimate {
    pub binding_id: String,
    pub maximum: i128,
    pub consumed: i128,
    pub held: i128,
    pub released: i128,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepEstimate {
    pub ordinal: usize,
    pub entry: ActivityEntry,
    pub family: Option<FamilyKey>,
    pub revision: Option<u64>,
    pub exact: Option<ExactRatio>,
    pub inverse: Option<i128>,
    pub replacement: Option<i128>,
    pub delta: i128,
    pub difference_from_booked_delta: i128,
    pub reason: &'static str,
    pub binding: BindingEstimate,
    pub reservation: Option<ReservationEstimate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatestEstimate {
    pub bindings: Vec<BindingEstimate>,
    pub families: Vec<FamilyEstimate>,
    pub reservations: Vec<ReservationEstimate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateFailure {
    pub ordinal: Option<usize>,
    pub reason: Error,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateResult {
    Complete {
        steps: Vec<StepEstimate>,
        latest: LatestEstimate,
    },
    /// No prefix total is presented as a comparable full-history result.
    Infeasible(CandidateFailure),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateComparison {
    pub key: String,
    pub fingerprint: String,
    pub provenance: ComparisonProvenance,
    pub amounts: Vec<CandidateAmount>,
    pub result: CandidateResult,
}
/// Estimates have no conversion to authoritative outcome decisions.
/// ```compile_fail
/// use ledgerlab_core::policy::chaining::{comparison::ComparisonMatrix, outcomes::Decision};
/// fn promote(matrix: ComparisonMatrix) -> Decision { matrix.into() }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComparisonMatrix {
    pub original: CandidateComparison,
    pub candidates: Vec<CandidateComparison>,
}
impl ComparisonMatrix {
    pub const fn committed(&self) -> bool {
        false
    }
    pub const fn assumption(&self) -> &'static str {
        ASSUMPTION
    }
}

/// Two through eight drafts, plus an automatic original-policy control. Candidate
/// failures are isolated. Candidate order and labels do not affect economics.
pub fn compare_amount_candidates(
    activity: &ComparisonActivity,
    candidates: &[AmountCandidate],
) -> Result<ComparisonMatrix> {
    let mut driver = ComparisonDriver::new(activity, candidates)?;
    while !driver.advance()? {}
    driver.finish()
}
fn validate_candidates(candidates: &[AmountCandidate]) -> Result<()> {
    require(
        (2..=8).contains(&candidates.len()),
        "COMPARISON_LIMIT",
        "two through eight candidates",
    )?;
    let mut bytes = 0usize;
    for c in candidates {
        text(&c.key, 128)?;
        require(
            c.amounts.len() <= 1024,
            "COMPARISON_LIMIT",
            "candidate amount count",
        )?;
        for a in &c.amounts {
            for s in [&a.key.agreement, &a.key.family, &a.key.code] {
                text(s, 128)?;
            }
        }
        bytes += descriptor(&c.amounts)?.len() + c.key.len();
        require(
            bytes <= MAX_CANDIDATE_BYTES,
            "COMPARISON_LIMIT",
            "collective candidate bytes",
        )?;
    }
    Ok(())
}

fn descriptor(amounts: &[CandidateAmount]) -> Result<Vec<u8>> {
    let mut sorted: Vec<_> = amounts.iter().collect();
    sorted.sort_by(|a, b| a.key.cmp(&b.key));
    let rows: Vec<_> = sorted
        .into_iter()
        .map(|a| {
            let amount = match &a.amount {
                o::Amount::Fixed(m) => json!({"fixed":m}),
                o::Amount::Percent(p) => json!({"percent":p}),
            };
            json!([a.key.agreement, a.key.family, a.key.code, amount])
        })
        .collect();
    serde_json::to_vec(&rows).map_err(|_| invalid("amount descriptor"))
}
fn money(unit: &Money, atoms: i128) -> Result<Money> {
    Money::new(unit.currency(), unit.scale(), atoms)
}
fn same_unit(a: &Money, b: &Money) -> Result<()> {
    require(
        a.currency() == b.currency() && a.scale() == b.scale(),
        "POLICY_CURRENCY",
        "original currency and scale",
    )
}
fn invalid(detail: &str) -> Error {
    Error::new("COMPARISON_HISTORY", detail)
}

#[derive(Clone, Debug, Default)]
struct Head {
    revision: Option<u64>,
    code: Option<String>,
    amount: i128,
    closed: bool,
}
impl ComparisonActivity {
    fn comparison(
        &self,
        key: &str,
        amounts: &[CandidateAmount],
        result: CandidateResult,
    ) -> Result<CandidateComparison> {
        let mut input = serde_json::to_vec(&json!([
            self.provenance.snapshot,
            self.provenance.activity,
            self.provenance.semantics
        ]))
        .map_err(|_| invalid("provenance descriptor"))?;
        input.extend(descriptor(amounts)?);
        Ok(CandidateComparison {
            key: key.into(),
            fingerprint: provenance_digest("candidate", &input)?,
            provenance: self.provenance.clone(),
            amounts: amounts.to_vec(),
            result,
        })
    }
    fn amount_table(&self, amounts: &[CandidateAmount]) -> Result<BTreeMap<AmountKey, o::Amount>> {
        let expected: BTreeSet<_> = self.original_amounts().into_iter().map(|a| a.key).collect();
        require(
            amounts.len() == expected.len(),
            "COMPARISON_CANDIDATE",
            "complete existing code table",
        )?;
        let originals: BTreeMap<_, _> = self
            .original_amounts()
            .into_iter()
            .map(|a| (a.key, a.amount))
            .collect();
        let mut table = BTreeMap::new();
        for a in amounts {
            require(
                expected.contains(&a.key)
                    && table.insert(a.key.clone(), a.amount.clone()).is_none(),
                "COMPARISON_CANDIDATE",
                "no extra, missing or duplicate keys",
            )?;
            let family = self
                .families
                .iter()
                .find(|f| f.key.agreement == a.key.agreement && f.key.family == a.key.family)
                .ok_or_else(|| invalid("candidate family"))?;
            require(
                self.bindings[family.binding].book != Book::Supplier
                    || originals.get(&a.key) == Some(&a.amount),
                "COMPARISON_SUPPLIER_TERMS",
                "supplier terms remain unchanged",
            )?;
            if let o::Amount::Fixed(m) = &a.amount {
                same_unit(&self.retail_basis, m)?;
            }
        }
        Ok(table)
    }
    fn binding_estimate(
        &self,
        index: usize,
        heads: &[Head],
        booked: &[i128],
    ) -> Result<BindingEstimate> {
        let mut discount = 0;
        let mut premium = 0;
        let mut original_discount = 0;
        let mut original_premium = 0;
        for (i, _) in self
            .families
            .iter()
            .enumerate()
            .filter(|(_, f)| f.binding == index)
        {
            discount = add_atoms(discount, heads[i].amount.min(0))?;
            premium = add_atoms(premium, heads[i].amount.max(0))?;
            original_discount = add_atoms(original_discount, booked[i].min(0))?;
            original_premium = add_atoms(original_premium, booked[i].max(0))?;
        }
        let b = &self.bindings[index];
        let net = o::active_net(
            b.booked_net.atoms(),
            discount,
            premium,
            b.premium_limit.atoms(),
        )?;
        let original = o::active_net(
            b.booked_net.atoms(),
            original_discount,
            original_premium,
            b.premium_limit.atoms(),
        )?;
        Ok(BindingEstimate {
            binding_id: b.binding_id.clone(),
            discount,
            premium,
            adjustment: add_atoms(discount, premium)?,
            final_net: net,
            difference_from_booked: add_atoms(net, -original)?,
        })
    }
    fn initial_state(&self) -> NumericState {
        NumericState {
            heads: vec![Head::default(); self.families.len()],
            booked: vec![0; self.families.len()],
            reservations: self
                .reservations
                .iter()
                .map(|r| ReservationEstimate {
                    binding_id: r.binding_id.clone(),
                    maximum: r.maximum.atoms(),
                    consumed: r.consumed.atoms(),
                    held: r.held.atoms(),
                    released: r.released.atoms(),
                })
                .collect(),
            steps: Vec::new(),
        }
    }
    fn step(
        &self,
        table: &BTreeMap<AmountKey, o::Amount>,
        is_original: bool,
        state: &mut NumericState,
        ordinal: usize,
    ) -> std::result::Result<(), CandidateFailure> {
        let NumericState {
            heads,
            booked,
            reservations,
            steps,
        } = state;
        let entry = &self.chronology[ordinal];
        let step = (|| -> Result<StepEstimate> {
            let (
                binding_index,
                family,
                revision,
                exact,
                inverse,
                replacement,
                delta,
                difference,
                reason,
            ) = match entry {
                ActivityEntry::Outcome { decision_index } => {
                    let d = &self.economic[*decision_index];
                    let f = &self.families[d.family];
                    let h = &mut heads[d.family];
                    let (code, inverse, ordinary) = match &d.change {
                        o::Change::Claim { code } => {
                            require(
                                h.revision.is_none() && !h.closed && d.revision == 1,
                                "COMPARISON_HISTORY",
                                "ordinary once before closure, including zero",
                            )?;
                            (Some(code.clone()), None, true)
                        }
                        o::Change::Correct {
                            expected_revision,
                            replacement,
                        } => {
                            require(
                                h.revision == Some(*expected_revision)
                                    && d.revision == expected_revision + 1,
                                "COMPARISON_HISTORY",
                                "ordered correction revision",
                            )?;
                            require(
                                replacement
                                    .as_ref()
                                    .map_or(f.allow_reversal, |c| f.replacements.contains(c)),
                                "COMPARISON_HISTORY",
                                "frozen replacement permission",
                            )?;
                            (replacement.clone(), Some(-h.amount), false)
                        }
                    };
                    let amount = code
                        .as_ref()
                        .map(|code| {
                            table
                                .get(&AmountKey {
                                    agreement: f.key.agreement.clone(),
                                    family: f.key.family.clone(),
                                    code: code.clone(),
                                })
                                .ok_or_else(|| invalid("unknown activity code"))
                        })
                        .transpose()?;
                    let exact = o::exact_amount(amount, &self.retail_basis)?;
                    let current = exact.round_atoms()?;
                    let delta = add_atoms(inverse.unwrap_or(0), current)?;
                    if is_original {
                        require(
                            current == d.booked && delta == d.booked_delta,
                            "COMPARISON_HISTORY",
                            "original control parity",
                        )?;
                    }
                    h.amount = current;
                    h.revision = Some(d.revision);
                    h.code = code.clone();
                    // Ordinary closes only its family; corrections never
                    // replenish or consume the reservation, even after close.
                    h.closed = true;
                    if ordinary {
                        if let Some(r) = reservations
                            .iter_mut()
                            .find(|r| r.binding_id == self.bindings[f.binding].binding_id)
                        {
                            let consume = current.max(0);
                            require(
                                consume <= r.held,
                                "COMPARISON_RESERVATION_CAPACITY",
                                "ordinary held capacity",
                            )?;
                            r.consumed = add_atoms(r.consumed, consume)?;
                            r.held -= consume;
                        }
                    }
                    booked[d.family] = d.booked;
                    (
                        f.binding,
                        Some(f.key.clone()),
                        Some(d.revision),
                        Some(exact),
                        inverse,
                        Some(current),
                        delta,
                        add_atoms(delta, -d.booked_delta)?,
                        if code.is_none() {
                            "CLAIM_REVERSED"
                        } else if current == 0 {
                            "ZERO_ROUNDED"
                        } else {
                            "OUTCOME_APPLIED"
                        },
                    )
                }
                ActivityEntry::Close { binding_id } => {
                    let index = self
                        .bindings
                        .iter()
                        .position(|b| &b.binding_id == binding_id)
                        .ok_or_else(|| invalid("closure binding"))?;
                    let r = reservations
                        .iter_mut()
                        .find(|r| &r.binding_id == binding_id)
                        .ok_or_else(|| invalid("closure checkpoint"))?;
                    r.released = add_atoms(r.released, r.held)?;
                    r.held = 0;
                    for (i, _) in self
                        .families
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.binding == index)
                    {
                        heads[i].closed = true;
                    }
                    (
                        index,
                        None,
                        None,
                        None,
                        None,
                        None,
                        0,
                        0,
                        "RESERVATION_CLOSED",
                    )
                }
            };
            let binding = self.binding_estimate(binding_index, heads, booked)?;
            let reservation = reservations
                .iter()
                .find(|r| r.binding_id == binding.binding_id)
                .cloned();
            if let Some(r) = &reservation {
                require(
                    add_atoms(add_atoms(r.consumed, r.held)?, r.released)? == r.maximum,
                    "COMPARISON_HISTORY",
                    "reservation conservation",
                )?;
            }
            Ok(StepEstimate {
                ordinal,
                entry: entry.clone(),
                family,
                revision,
                exact,
                inverse,
                replacement,
                delta,
                difference_from_booked_delta: difference,
                reason,
                binding,
                reservation,
            })
        })()
        .map_err(|reason| CandidateFailure {
            ordinal: Some(ordinal),
            reason,
        })?;
        steps.push(step);
        Ok(())
    }
    fn latest(
        &self,
        state: NumericState,
    ) -> std::result::Result<CandidateResult, CandidateFailure> {
        let NumericState {
            heads,
            booked,
            reservations,
            steps,
        } = state;
        let bindings = (0..self.bindings.len())
            .map(|i| self.binding_estimate(i, &heads, &booked))
            .collect::<Result<_>>()
            .map_err(|reason| CandidateFailure {
                ordinal: None,
                reason,
            })?;
        let families = self
            .families
            .iter()
            .zip(heads)
            .map(|(f, h)| FamilyEstimate {
                key: f.key.clone(),
                revision: h.revision,
                current_code: h.code,
                amount: h.revision.map(|_| h.amount),
                ordinary_closed: h.closed,
            })
            .collect();
        Ok(CandidateResult::Complete {
            steps,
            latest: LatestEstimate {
                bindings,
                families,
                reservations,
            },
        })
    }
    #[cfg(test)]
    fn run(
        &self,
        amounts: &[CandidateAmount],
    ) -> std::result::Result<(Vec<StepEstimate>, LatestEstimate), CandidateFailure> {
        let table = self
            .amount_table(amounts)
            .map_err(|reason| CandidateFailure {
                ordinal: None,
                reason,
            })?;
        let original = self
            .original_amounts()
            .iter()
            .all(|a| table.get(&a.key) == Some(&a.amount));
        let mut state = self.initial_state();
        for ordinal in 0..self.chronology.len() {
            self.step(&table, original, &mut state, ordinal)?;
        }
        match self.latest(state)? {
            CandidateResult::Complete { steps, latest } => Ok((steps, latest)),
            CandidateResult::Infeasible(f) => Err(f),
        }
    }
}

struct NumericState {
    heads: Vec<Head>,
    booked: Vec<i128>,
    reservations: Vec<ReservationEstimate>,
    steps: Vec<StepEstimate>,
}
/// Resumable pure computation. Callers may check cancellation and yield before
/// and after each advance, then discard this value without producing a report.
/// Each advance performs at most one retained economic/closure step. Table
/// validation is bounded by 1024 entries, finalization by 32 families/16 bindings.
/// Numeric work is O(candidates * steps * families); no full-prefix rescans.
pub struct ComparisonDriver<'a> {
    activity: &'a ComparisonActivity,
    inputs: Vec<AmountCandidate>,
    current: usize,
    ordinal: usize,
    table: Option<BTreeMap<AmountKey, o::Amount>>,
    state: Option<NumericState>,
    completed: Vec<CandidateComparison>,
    error: Option<Error>,
}
impl<'a> ComparisonDriver<'a> {
    pub fn new(activity: &'a ComparisonActivity, candidates: &[AmountCandidate]) -> Result<Self> {
        validate_candidates(candidates)?;
        activity.validate()?;
        let mut inputs = vec![AmountCandidate {
            key: "original".into(),
            amounts: activity.original_amounts(),
        }];
        inputs.extend_from_slice(candidates);
        Ok(Self {
            activity,
            inputs,
            current: 0,
            ordinal: 0,
            table: None,
            state: None,
            completed: Vec::new(),
            error: None,
        })
    }
    /// True means every scenario completed (possibly with candidate failures).
    /// An original-control failure is terminal and never yields a matrix.
    pub fn advance(&mut self) -> Result<bool> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.current == self.inputs.len() {
            return Ok(true);
        }
        let result = self.advance_inner();
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }
    fn advance_inner(&mut self) -> Result<bool> {
        if self.table.is_none() {
            match self
                .activity
                .amount_table(&self.inputs[self.current].amounts)
            {
                Ok(table) => {
                    self.table = Some(table);
                    self.state = Some(self.activity.initial_state());
                }
                Err(reason) => {
                    return self.complete(CandidateResult::Infeasible(CandidateFailure {
                        ordinal: None,
                        reason,
                    }))
                }
            }
        }
        if self.ordinal < self.activity.chronology.len() {
            if let Err(failure) = self.activity.step(
                self.table.as_ref().expect("initialized"),
                self.current == 0,
                self.state.as_mut().expect("initialized"),
                self.ordinal,
            ) {
                return self.complete(CandidateResult::Infeasible(failure));
            }
            self.ordinal += 1;
        }
        if self.ordinal == self.activity.chronology.len() {
            let result = self
                .activity
                .latest(self.state.take().expect("initialized"))
                .unwrap_or_else(CandidateResult::Infeasible);
            return self.complete(result);
        }
        Ok(false)
    }
    fn complete(&mut self, result: CandidateResult) -> Result<bool> {
        if self.current == 0 {
            if let CandidateResult::Infeasible(f) = result {
                return Err(f.reason);
            }
        }
        let candidate = &self.inputs[self.current];
        self.completed.push(self.activity.comparison(
            &candidate.key,
            &candidate.amounts,
            result,
        )?);
        self.current += 1;
        self.ordinal = 0;
        self.table = None;
        self.state = None;
        Ok(self.current == self.inputs.len())
    }
    pub fn finish(mut self) -> Result<ComparisonMatrix> {
        if let Some(error) = self.error {
            return Err(error);
        }
        require(
            self.current == self.inputs.len(),
            "COMPARISON_INCOMPLETE",
            "all steps and original control must complete",
        )?;
        let original = self.completed.remove(0);
        Ok(ComparisonMatrix {
            original,
            candidates: self.completed,
        })
    }
}

#[cfg(test)]
mod tests;

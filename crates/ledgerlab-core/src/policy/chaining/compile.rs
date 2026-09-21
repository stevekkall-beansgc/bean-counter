use super::model::*;
use crate::domain::{prefixed, slug, text, validate_source};
use crate::money::validate_currency;
use crate::wire::{EventKind, Relation};
use crate::{Error, Result};
use std::collections::BTreeSet;

pub(super) fn require(ok: bool, code: &'static str, detail: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::new(code, detail))
    }
}
pub(super) fn phase(op: &Operation) -> u8 {
    match op {
        Operation::Base(_) | Operation::ObserveCost => 0,
        Operation::Premium(_) => 1,
        Operation::Discount { .. } | Operation::LinkedDiscount { .. } => 2,
        Operation::Cap { .. } => 3,
        Operation::Share { .. } => 4,
    }
}
impl Book {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::Supplier => "supplier",
            Self::CostObservation => "cost_observation",
            Self::Allocation => "allocation",
        }
    }
}
impl Bundle {
    /// Compile a complete pinned bundle. Input rule position is preserved within
    /// each phase; binding order is retail, supplier, observation, then agreement.
    pub fn compile(currency: &str, scale: u8, mut policies: Vec<Policy>) -> Result<Self> {
        validate_currency(currency, scale)?;
        require(
            !policies.is_empty() && policies.len() <= 16,
            "POLICY_LIMIT",
            "bindings",
        )?;
        require(
            policies
                .iter()
                .filter(|p| p.binding.book == Book::Retail)
                .count()
                == 1,
            "TERMS_NOT_ACCEPTED",
            "exactly one retail binding",
        )?;
        policies.sort_by(|a, b| {
            (a.binding.book, &a.binding.agreement).cmp(&(b.binding.book, &b.binding.agreement))
        });
        let mut bindings = BTreeSet::new();
        let mut agreements = BTreeSet::new();
        let mut nodes = 1;
        let mut order = Vec::new();
        let mut cap_count = 0;
        let mut share_count = 0;
        for (pi, policy) in policies.iter().enumerate() {
            let b = &policy.binding;
            text(&b.id, 128)?;
            text(&b.agreement, 128)?;
            b.roles.validate()?;
            prefixed(&b.assent, "doc_")?;
            slug(&b.unit)?;
            require(
                bindings.insert(&b.id) && agreements.insert(&b.agreement),
                "POLICY_AMBIGUOUS_MATCH",
                "binding/agreement",
            )?;
            require(
                b.book != Book::Allocation,
                "POLICY_UNSUPPORTED_OP",
                "allocation is internal",
            )?;
            require(!b.maximum_quantity.is_zero(), "QUANTITY", "binding ceiling")?;
            require(
                b.roles.bearer() == b.roles.payer() || b.roles.payer_delegation().is_some(),
                "PAYER_DELEGATION_REQUIRED",
                "different payer",
            )?;
            require(
                !b.sources.is_empty()
                    && b.sources.len() <= 16
                    && !b.event_types.is_empty()
                    && b.event_types.len() <= 5
                    && b.correction_sources.len() <= 16
                    && b.allowed_modifiers.len() <= 64,
                "POLICY_LIMIT",
                "binding permissions",
            )?;
            let mut sources = BTreeSet::new();
            for source in &b.sources {
                validate_source(source)?;
                require(
                    sources.insert(source),
                    "POLICY_AMBIGUOUS_MATCH",
                    "duplicate source",
                )?;
            }
            for source in &b.correction_sources {
                validate_source(source)?;
            }
            for component in &b.allowed_modifiers {
                slug(component)?;
            }
            for kind in &b.event_types {
                require(
                    kind.is_work() || *kind == EventKind::Acquired,
                    "POLICY_UNSUPPORTED_OP",
                    "event type",
                )?;
            }
            if let Some(offer) = &b.offer {
                prefixed(offer, "doc_")?;
            }
            if let Some(limit) = &b.maximum_exposure {
                require(
                    limit.currency() == currency && limit.scale() == scale && limit.atoms() >= 0,
                    "POLICY_CURRENCY",
                    "binding exposure",
                )?;
            }
            if b.book == Book::Supplier {
                require(
                    b.offer.is_some() && b.maximum_exposure.is_some(),
                    "TERMS_NOT_ACCEPTED",
                    "supplier offer and exposure",
                )?;
            }
            if let Some(o) = &b.outcome {
                validate_source(&o.source)?;
                slug(&o.claim_namespace)?;
                require(
                    o.window_us > 0
                        && o.window_us <= 90 * 86_400_000_000
                        && o.report_grace_us <= 7 * 86_400_000_000,
                    "OUTCOME_WINDOW",
                    "bounded half-open outcome window",
                )?;
            }
            require(!policy.rules.is_empty(), "POLICY_LIMIT", "empty policy")?;
            let mut ids = BTreeSet::new();
            let mut components = BTreeSet::new();
            for (ri, rule) in policy.rules.iter().enumerate() {
                slug(&rule.id)?;
                slug(&rule.component)?;
                require(
                    ids.insert(rule.id.clone()) && components.insert(rule.component.clone()),
                    "POLICY_AMBIGUOUS_MATCH",
                    "rule/component",
                )?;
                require(
                    b.event_types.contains(&rule.on),
                    "POLICY_UNAUTHORIZED_MODIFIER",
                    "event outside binding",
                )?;
                require(rule.when.len() <= 8, "POLICY_LIMIT", "predicates")?;
                nodes += 2 + rule.when.len();
                for predicate in &rule.when {
                    match predicate {
                        Predicate::Tier(t) => slug(t)?,
                        Predicate::Source(s) => validate_source(s)?,
                        _ => {}
                    }
                }
                if let Some(m) = rule.matcher {
                    validate_matcher(rule.on, m)?;
                }
                if rule.on == EventKind::Acquired {
                    require(
                        b.outcome.is_some(),
                        "OUTCOME_AUTHORITY",
                        "outcome terms required",
                    )?;
                }
                let local_basis = |component: &str, consumer_phase: u8| -> Result<()> {
                    slug(component)?;
                    let found = policy.rules.iter().enumerate().any(|(i, r)| {
                        r.component == component
                            && r.on == rule.on
                            && matches!(r.operation, Operation::Base(_) | Operation::Premium(_))
                            && (phase(&r.operation) < consumer_phase
                                || (phase(&r.operation) == consumer_phase && i < ri))
                    });
                    require(
                        found,
                        "POLICY_MISSING_BASIS",
                        "earlier booked base/premium on the same event",
                    )
                };
                match &rule.operation {
                    Operation::Base(price) | Operation::Premium(price) => {
                        require(
                            matches!(b.book, Book::Retail | Book::Supplier),
                            "POLICY_UNSUPPORTED_OP",
                            "priced book",
                        )?;
                        if matches!(rule.operation, Operation::Base(_)) {
                            require(
                                rule.on.is_work(),
                                "POLICY_UNSUPPORTED_OP",
                                "base needs work",
                            )?;
                        }
                        validate_price(price, scale)?;
                        if let Price::Percent { component, .. } = price {
                            local_basis(component, phase(&rule.operation))?;
                        }
                        if let Price::Unit { unit, .. } = price {
                            require(
                                rule.on.is_work() && unit == &b.unit,
                                "POLICY_UNIT",
                                "unit price requires matching work unit",
                            )?;
                        }
                    }
                    Operation::Discount {
                        amount, component, ..
                    } => {
                        validate_discount(amount, scale)?;
                        local_basis(component, 2)?;
                        modifier(b, &rule.component)?;
                    }
                    Operation::LinkedDiscount { .. } => {
                        return Err(Error::new(
                            "OUTCOME_TARGET_REQUIRED",
                            "superseded proposal: use frozen-target outcome families",
                        ));
                    }
                    Operation::Cap {
                        ceiling,
                        stage,
                        component,
                    } => {
                        cap_count += 1;
                        slug(stage)?;
                        ceiling.atoms_exact(scale)?;
                        local_basis(component, 3)?;
                        require(
                            b.book == Book::Retail
                                && rule.on == EventKind::Acquired
                                && rule.when.is_empty()
                                && rule.matcher.is_none(),
                            "POLICY_UNSUPPORTED_OP",
                            "one unconditional retail closure cap",
                        )?;
                        require(
                            policy.rules.iter().any(|r| {
                                r.component == *component
                                    && matches!(r.operation, Operation::Premium(_))
                            }),
                            "POLICY_MISSING_BASIS",
                            "cap closure premium",
                        )?;
                    }
                    Operation::Share {
                        percent,
                        ceiling,
                        retail_component,
                    } => {
                        share_count += 1;
                        percent.percent()?;
                        ceiling.atoms_exact(scale)?;
                        slug(retail_component)?;
                        require(
                            b.book == Book::Supplier
                                && rule.on == EventKind::Acquired
                                && rule.matcher.is_some(),
                            "POLICY_UNSUPPORTED_OP",
                            "supplier closure share",
                        )?;
                        require(
                            policies[0].rules.iter().any(|r| {
                                r.component == *retail_component
                                    && r.on == EventKind::Acquired
                                    && matches!(r.operation, Operation::Premium(_))
                            }),
                            "POLICY_MISSING_BASIS",
                            "retail closure premium",
                        )?;
                        require(
                            b.roles.bearer() == policies[0].binding.roles.recipient(),
                            "POLICY_UNAUTHORIZED_MODIFIER",
                            "share bearer must be retail recipient",
                        )?;
                        if b.allocation_view {
                            for suffix in [".supplier", ".host"] {
                                slug(&format!("{}{suffix}", rule.component))?;
                            }
                        }
                    }
                    Operation::ObserveCost => require(
                        b.book == Book::CostObservation && rule.on.is_work(),
                        "POLICY_UNSUPPORTED_OP",
                        "retained work cost only",
                    )?,
                }
                order.push((pi, ri));
            }
            for rule in &policy.rules {
                if b.allocation_view && matches!(rule.operation, Operation::Share { .. }) {
                    for suffix in [".supplier", ".host"] {
                        require(
                            !components.contains(&format!("{}{suffix}", rule.component)),
                            "POLICY_AMBIGUOUS_MATCH",
                            "reserved allocation component",
                        )?;
                    }
                }
            }
            require(
                !b.allocation_view
                    || policy
                        .rules
                        .iter()
                        .any(|r| matches!(r.operation, Operation::Share { .. })),
                "POLICY_UNSUPPORTED_OP",
                "allocation requires share",
            )?;
        }
        require(
            order.len() <= 64 && nodes <= 256 && cap_count <= 1 && share_count <= 1,
            "POLICY_LIMIT",
            "rules, AST, one cap/share",
        )?;
        order.sort_by_key(|&(p, r)| {
            (
                phase(&policies[p].rules[r].operation),
                policies[p].binding.book,
                p,
                r,
            )
        });
        Ok(Self {
            currency: currency.into(),
            scale,
            policies,
            order,
        })
    }
    pub fn policies(&self) -> &[Policy] {
        &self.policies
    }
}
fn modifier(b: &Binding, component: &str) -> Result<()> {
    require(
        b.book == Book::Retail
            || b.book == Book::Supplier && b.allowed_modifiers.iter().any(|s| s == component),
        "POLICY_UNAUTHORIZED_MODIFIER",
        "supplier discount requires accepted modifier",
    )
}
fn validate_price(p: &Price, scale: u8) -> Result<()> {
    match p {
        Price::Fixed(d) => {
            d.atoms_exact(scale)?;
        }
        Price::Unit { unit, .. } => slug(unit)?,
        Price::Percent { percent, component } => {
            percent.percent()?;
            slug(component)?;
        }
    }
    Ok(())
}
fn validate_discount(p: &DiscountAmount, scale: u8) -> Result<()> {
    match p {
        DiscountAmount::Fixed(d) => {
            d.atoms_exact(scale)?;
        }
        DiscountAmount::Percent(d) => {
            d.percent()?;
        }
    }
    Ok(())
}
fn validate_matcher(on: EventKind, m: Matcher) -> Result<()> {
    let valid = match m {
        Matcher::AcquisitionOptimization => on == EventKind::Acquired,
        Matcher::Direct { relation, target } => match (on, relation, target) {
            (EventKind::Optimized, Relation::OptimizedFrom, EventKind::Generated)
            | (
                EventKind::Published,
                Relation::PublishedAs,
                EventKind::Generated | EventKind::Optimized,
            )
            | (EventKind::Acquired, Relation::AttributedTo, EventKind::Published) => true,
            (k, Relation::ConsumesService, EventKind::ToolCompleted) => matches!(
                k,
                EventKind::Generated | EventKind::Optimized | EventKind::Published
            ),
            _ => false,
        },
    };
    require(valid, "POLICY_AMBIGUOUS_MATCH", "unsupported typed path")
}

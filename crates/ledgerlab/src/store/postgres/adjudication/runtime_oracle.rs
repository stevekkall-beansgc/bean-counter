//! Numeric prefix expectations pinned before canonical authoring in the
//! independent oracle CUSTOMER-STORY.md/EXPECTATIONS.json. These are not
//! generated from this adapter, fixture results, or recorded PostgreSQL roots.
use super::*;
use r3::runtime as rt;
use serde_json::Value;
async fn actual_point(host: &Host, key: HeadKey, _class: GuardClass) -> ObservedHead {
    let f = &host.stores[key.journal.host.as_str()];
    crate::store::postgres::adjudication::read::point(&f.owner.client, &key, r3::SEGMENT_BYTES)
        .await
        .unwrap()
}
pub(super) struct CustomerOracle {
    retail: i128,
    supplier: i128,
    actions: Vec<wire::Action>,
    late_before_close: Vec<ObservedHead>,
    pub(super) checkpoints: Vec<Value>,
}
impl Default for CustomerOracle {
    fn default() -> Self {
        Self {
            retail: 10000,
            supplier: 3000,
            actions: vec![],
            late_before_close: vec![],
            checkpoints: vec![],
        }
    }
}
impl CustomerOracle {
    pub(super) async fn observe(
        &mut self,
        store: &Host,
        through: usize,
        input: &Value,
        result: &wire::CommandResult,
    ) {
        for effect in &result.effects {
            if let wire::Effect::Action { body } = effect {
                assert_eq!(
                    body.magnitude.value(),
                    body.signed_atoms.value().unsigned_abs()
                );
                match body.book {
                    wire::ActionBook::Retail => self.retail += body.signed_atoms.value(),
                    wire::ActionBook::Supplier => self.supplier += body.signed_atoms.value(),
                }
                self.actions.push(body.clone());
            }
        }
        const THROUGH: [usize; 15] = [5, 11, 12, 18, 19, 25, 26, 32, 38, 44, 91, 92, 93, 94, 95];
        const RETAIL: [i128; 15] = [
            10000, 10000, 11200, 11200, 11200, 11200, 11700, 11700, 11700, 11700, 11700, 11800,
            11650, 11650, 11450,
        ];
        const ENTITLEMENTS: [usize; 15] = [0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 4, 5, 5];
        let Some(step) = THROUGH.iter().position(|n| *n == through) else {
            return;
        };
        assert_eq!(self.retail, RETAIL[step], "oracle S{step:02} retail");
        assert_eq!(self.supplier, 3000, "oracle S{step:02} supplier");
        let mut families = Vec::new();
        for terms in input["commands"][4]["payload"]["families"]
            .as_array()
            .unwrap()
        {
            let key: wire::Family = serde_json::from_value(terms["key"].clone()).unwrap();
            let point = rt::points::Point::family(&key).unwrap();
            let head = actual_point(
                store,
                HeadKey {
                    journal: journal("center"),
                    kind: HeadKind::Family,
                    full_key: point.key,
                },
                GuardClass::FamilyPrerequisite,
            )
            .await;
            let State::Family(family) =
                serde_json::from_slice(head.value.as_deref().unwrap()).unwrap()
            else {
                panic!("family")
            };
            families.push(*family);
        }
        let consumed = families
            .iter()
            .filter(|f| matches!(f.entitlement, wire::EntitlementHead::Consumed { .. }))
            .count();
        assert_eq!(
            consumed, ENTITLEMENTS[step],
            "oracle S{step:02} entitlements"
        );
        assert_eq!(
            families.iter().filter(|f| f.closed).count(),
            if step >= 10 { 5 } else { 0 }
        );
        assert_eq!(
            families
                .iter()
                .map(|f| f.ordinary_positive.value())
                .sum::<u128>(),
            if step >= 6 {
                1700
            } else if step >= 2 {
                1200
            } else {
                0
            },
            "correction cannot restore premium usage"
        );
        let mut pools = Vec::new();
        for name in ["positive", "negative", "zero"] {
            let head = actual_point(
                store,
                HeadKey {
                    journal: journal("center"),
                    kind: HeadKind::Adjustment,
                    full_key: rt::index_key(*b"ADJPOOL_", &[name.as_bytes()]).unwrap(),
                },
                GuardClass::AdjustmentPool,
            )
            .await;
            let State::Adjustment(pool) =
                serde_json::from_slice(head.value.as_deref().unwrap()).unwrap()
            else {
                panic!("pool")
            };
            pools.push(*pool);
        }
        let positive = if step >= 11 { 100 } else { 0 };
        let negative = if step >= 12 { 150 } else { 0 };
        assert_eq!(
            pools.iter().map(|p| p.gross_used.value()).sum::<u128>(),
            positive + negative
        );
        assert_eq!(
            pools.iter().map(|p| p.positive_used.value()).sum::<u128>(),
            positive
        );
        assert_eq!(
            pools.iter().map(|p| p.negative_used.value()).sum::<u128>(),
            negative
        );
        assert_eq!(
            pools
                .iter()
                .map(|p| p.funding_used.value())
                .collect::<Vec<_>>(),
            vec![positive, negative, 0]
        );
        if step >= 4 {
            let denied = oracle_case(store, input, 16).await;
            assert_eq!(denied.status, rt::points::CaseStatus::FinalDeny);
            assert_eq!(denied.revision, Count::ZERO);
            assert!(!self.actions.iter().any(|a| a.case == denied.input.case));
        }
        if step >= 6 {
            let qualified = oracle_case(store, input, 23).await;
            let wire::EntitlementHead::Consumed { case, revision, .. } = &families[1].entitlement
            else {
                panic!("upsell consumer")
            };
            assert_eq!(**case, qualified.input.case);
            assert_eq!(revision.value(), if step == 14 { 2 } else { 1 });
            assert_eq!(qualified.signed.value(), if step == 14 { 300 } else { 500 });
        }
        if step == 9 || step == 10 {
            for (i, command) in [30, 36, 42].into_iter().enumerate() {
                let key: wire::Case = serde_json::from_value(
                    input["commands"][command]["payload"]["submission"]["case"].clone(),
                )
                .unwrap();
                let head = actual_point(
                    store,
                    HeadKey {
                        journal: journal("center"),
                        kind: HeadKind::Case,
                        full_key: rt::points::Point::case(&key).unwrap().key,
                    },
                    GuardClass::Case,
                )
                .await;
                if step == 9 {
                    self.late_before_close.push(head);
                } else {
                    assert_eq!(
                        head.revision, self.late_before_close[i].revision,
                        "virtual close writes no case"
                    );
                    assert_eq!(
                        head.value, self.late_before_close[i].value,
                        "virtual close writes no case"
                    );
                    let State::Case(c) =
                        serde_json::from_slice(head.value.as_deref().unwrap()).unwrap()
                    else {
                        panic!("late case")
                    };
                    assert_eq!(c.status, rt::points::CaseStatus::OrdinaryPending);
                    assert_eq!(
                        c.effective_status(&families[i + 2]),
                        rt::points::CaseStatus::AdjustmentPending
                    );
                    assert!(families[i + 2].first_closure.is_some());
                }
            }
        }
        if step == 4 || step == 13 {
            assert!(
                result.effects.is_empty(),
                "DENY and zero ALLOW post no action"
            );
        }
        if step == 11 || step == 12 {
            let wire::Effect::Action { body } = &result.effects[0] else {
                panic!("adjustment action")
            };
            assert_eq!(result.effects.len(), 1);
            assert_eq!(
                body.roles.payer.as_str(),
                if step == 11 { "customer" } else { "vendor" }
            );
            assert_eq!(
                body.roles.recipient.as_str(),
                if step == 11 { "vendor" } else { "customer" }
            );
            assert_eq!(body.kind, wire::ActionKind::Adjustment);
        }
        if step == 14 {
            assert_eq!(result.effects.len(), 2);
            let wire::Effect::Action { body: inverse } = &result.effects[0] else {
                panic!("inverse")
            };
            let wire::Effect::Action { body: replacement } = &result.effects[1] else {
                panic!("replacement")
            };
            assert_eq!(
                (inverse.kind.clone(), inverse.signed_atoms.value()),
                (wire::ActionKind::Inverse, -500)
            );
            assert_eq!(
                (replacement.kind.clone(), replacement.signed_atoms.value()),
                (wire::ActionKind::Replacement, 300)
            );
            assert_eq!(self.actions.len(), 6);
        }
        self.checkpoints.push(serde_json::json!({"id":format!("S{step:02}"),"through":through,"retail":self.retail.to_string(),"supplier":self.supplier.to_string(),"entitlements":consumed,"gross":(positive+negative).to_string(),"closed":step>=10}));
    }
}
async fn oracle_case(store: &Host, input: &Value, receive: usize) -> rt::points::CaseState {
    let case: wire::Case =
        serde_json::from_value(input["commands"][receive]["payload"]["submission"]["case"].clone())
            .unwrap();
    let head = actual_point(
        store,
        HeadKey {
            journal: journal("center"),
            kind: HeadKind::Case,
            full_key: rt::points::Point::case(&case).unwrap().key,
        },
        GuardClass::Case,
    )
    .await;
    let State::Case(c) = serde_json::from_slice(head.value.as_deref().unwrap()).unwrap() else {
        panic!("case")
    };
    *c
}

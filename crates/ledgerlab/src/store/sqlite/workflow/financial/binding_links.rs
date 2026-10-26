//! One paid immutable row at a time. Binding membership is only locator DATA;
//! canonical producer/endpoint validation and held chain-root equality follow.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;

pub(crate) struct BindingLinkCursor {
    pub child: AppClaimLocator,
    pub slot: AuditedBytes,
}
pub(crate) struct BindingLinkLocator {
    pub cursor: BindingLinkCursor,
    pub predecessor: AppClaimLocator,
    pub record: AppClaimLocator,
}
macro_rules! link_query {
    ($columns:literal) => {
        concat!($columns,
            " FROM workflow_financial_links l WHERE l.logical_store_id=? AND l.tenant=? AND l.environment=? AND l.binding_kind=? AND l.binding_id=? AND l.binding_digest=? AND (?='1' OR (l.child_kind,l.child_id,l.child_digest,l.slot)>(?,?,?,?)) AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=l.record_kind AND m.id=l.record_id AND m.digest=l.record_digest AND o.logical_store_id=l.logical_store_id AND o.tenant=l.tenant AND o.environment=l.environment AND o.commit_sequence<?) ORDER BY l.child_kind,l.child_id,l.child_digest,l.slot LIMIT 1")
    };
}
const LENGTHS: &str = link_query!("SELECT octet_length(l.child_kind),octet_length(l.child_id),octet_length(l.child_digest),octet_length(l.slot),octet_length(l.predecessor_kind),octet_length(l.predecessor_id),octet_length(l.predecessor_digest),octet_length(l.record_kind),octet_length(l.record_id),octet_length(l.record_digest)");
const VALUES: &str = link_query!("SELECT l.child_kind,l.child_id,l.child_digest,l.slot,l.predecessor_kind,l.predecessor_id,l.predecessor_digest,l.record_kind,l.record_id,l.record_digest");
const MAXIMUM: [i64; 10] = [48, 104, 71, 128, 48, 104, 71, 48, 104, 71];
fn text(bytes: &[u8]) -> Result<&str, FinancialReadError> {
    std::str::from_utf8(bytes).map_err(|_| invalid())
}
fn bind<'a>(
    sql: &'static str,
    parameters: &[&'a str],
    before: i64,
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    let mut query = sqlx::query(sql);
    for value in parameters {
        query = query.bind(*value);
    }
    query.bind(before)
}
impl SqliteTx {
    pub(crate) async fn workflow_local_binding_link_next(
        &mut self,
        store: &str,
        scope: &[String; 2],
        binding: &ExactRef,
        before: i64,
        after: Option<&BindingLinkCursor>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<BindingLinkLocator>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(invalid());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.binding_link_next_inner(store, scope, binding, before, after, session),
        )
        .await
        .map_err(|_| FinancialReadError::Store(self.workflow_timeout_error()))?;
        if result.is_ok() {
            session.check_cancelled()?;
            if self.financial_io.is_some()
                || self
                    .workflow_proof_failed
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                return Err(invalid());
            }
            self.failed = false;
        }
        result
    }
    async fn binding_link_next_inner(
        &mut self,
        store: &str,
        scope: &[String; 2],
        binding: &ExactRef,
        before: i64,
        after: Option<&BindingLinkCursor>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<BindingLinkLocator>, FinancialReadError> {
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        if binding.kind != "workflow-binding" || !resource_compare(session, before, 0)?.is_gt() {
            return Err(invalid());
        }
        let previous = if let Some(row) = after {
            [
                text(row.child.kind.bytes())?,
                text(row.child.id.bytes())?,
                text(row.child.digest.bytes())?,
                text(row.slot.bytes())?,
            ]
        } else {
            ["", "", "", ""]
        };
        let first = if after.is_none() { "1" } else { "0" };
        let parameters = [
            store,
            &scope[0],
            &scope[1],
            &binding.kind,
            &binding.id,
            &binding.digest,
            first,
            previous[0],
            previous[1],
            previous[2],
            previous[3],
        ];
        for value in parameters {
            heads::locator_text(value, 256, session)?;
        }
        let cutoff = before.to_be_bytes();
        let bound: [&[u8]; 12] = std::array::from_fn(|index| {
            if index < parameters.len() {
                parameters[index].as_bytes()
            } else {
                &cutoff
            }
        });
        let payload = io::binding_payload(&bound, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let lengths = bind(LENGTHS, &parameters, before)
            .fetch_optional(self.conn())
            .await?;
        let Some(lengths) = lengths else {
            self.financial_finish_io(session).await?;
            return Ok(None);
        };
        for (position, maximum) in MAXIMUM.iter().enumerate() {
            session.semantic_work().visit(ValidationVisit::Field)?;
            let bytes: i64 = lengths.try_get(position)?;
            session.semantic_work().arithmetic(
                ArithmeticOperation::Multiply,
                &[&(*maximum).into(), &2i64.into()],
            )?;
            if !resource_compare(session, bytes, 0)?.is_gt()
                || resource_compare(session, bytes, 2 * maximum)?.is_gt()
            {
                return Err(invalid());
            }
        }
        drop(lengths);
        self.financial_finish_io(session).await?;
        let materialized = 8 * MAXIMUM.iter().sum::<i64>() as usize;
        session.semantic_work().scan_bytes(materialized)?;
        session.semantic_work().write_bytes(materialized)?;
        let payload = io::binding_payload(&bound, &[], 0, materialized, session)?;
        self.financial_reserve_io(session, payload)?;
        let row = bind(VALUES, &parameters, before)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(invalid)?;
        for (position, maximum) in MAXIMUM.iter().enumerate() {
            session.semantic_work().visit(ValidationVisit::Field)?;
            let value: &str = row.try_get(position)?;
            if !resource_compare(session, value.len() as i64, 0)?.is_gt()
                || resource_compare(session, value.len() as i64, *maximum)?.is_gt()
            {
                return Err(invalid());
            }
        }
        let actual: [&str; 4] = [
            row.try_get(0)?,
            row.try_get(1)?,
            row.try_get(2)?,
            row.try_get(3)?,
        ];
        session
            .semantic_work()
            .visit(ValidationVisit::MemberCompare)?;
        if after.is_some() && actual <= previous {
            return Err(invalid());
        }
        let mut copy = |index| -> Result<AuditedBytes, FinancialReadError> {
            Ok(session.copy_representation(
                row.try_get::<&str, _>(index)?.as_bytes(),
                ledgerlab_core::validation_work::RepresentationKind::Record,
            )?)
        };
        let found = BindingLinkLocator {
            cursor: BindingLinkCursor {
                child: AppClaimLocator {
                    kind: copy(0)?,
                    id: copy(1)?,
                    digest: copy(2)?,
                },
                slot: copy(3)?,
            },
            predecessor: AppClaimLocator {
                kind: copy(4)?,
                id: copy(5)?,
                digest: copy(6)?,
            },
            record: AppClaimLocator {
                kind: copy(7)?,
                id: copy(8)?,
                digest: copy(9)?,
            },
        };
        drop(row);
        self.financial_finish_io(session).await?;
        Ok(Some(found))
    }
}

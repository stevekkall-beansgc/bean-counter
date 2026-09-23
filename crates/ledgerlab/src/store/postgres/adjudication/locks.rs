use super::*;
use crate::store::outcomes::{OutcomeLockClass, OutcomeLockMode};
#[derive(Default)]
pub(in crate::store::postgres) struct Locked {
    pub journal: Option<JournalIdentity>,
    pub guards: Vec<Guard>,
    pub appended: bool,
}
impl Locked {
    pub fn require(&self, j: &JournalIdentity) -> Result<(), StoreError> {
        if self.journal.as_ref() != Some(j) || self.guards.is_empty() || self.appended {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(super) async fn acquire<C: GenericClient + Sync>(
    c: &C,
    held: &mut Locked,
    legacy: &mut super::super::outcomes::Locked,
    j: &JournalIdentity,
    guards: &[Guard],
) -> Result<(), StoreError> {
    if held.journal.is_some()
        || !super::super::outcomes::native_empty(legacy)
        || guards.is_empty()
        || guards.len() > 1024
        || !guards
            .windows(2)
            .all(|w| w[0].order_key() < w[1].order_key())
        || !matches!(&guards[0],Guard::Legacy(l) if l.class==OutcomeLockClass::Admission && l.mode==OutcomeLockMode::Write)
    {
        return Err(invalid());
    }
    c.query_one(
        "SELECT singleton FROM ledgerlab.installation WHERE singleton=1 FOR UPDATE",
        &[],
    )
    .await?;
    let installation = super::super::read::installation(c).await?;
    if installation.logical_store_id != j.store.as_str()
        || installation.scope.tenant != j.scope.0.as_str()
        || installation.scope.environment != j.scope.1.as_str()
    {
        return Err(invalid());
    }
    let jk = journal_key(j)?;
    let mut olds = Vec::new();
    for g in guards {
        match g {
            Guard::Legacy(l) => {
                let v = ledgerlab_core::canonical::parse_bounded(&l.key, 4096).map_err(core)?;
                if v[0] != json!(j.scope) {
                    return Err(invalid());
                }
                super::super::outcomes::lock_one(c, l).await?;
                olds.push(l.clone());
            }
            Guard::R3 { class, host, key } => {
                if *host != j.host || key.is_empty() || key.len() > r3::MAX_KEY_BYTES {
                    return Err(invalid());
                }
                let tag = i16::try_from(class.storage_tag()).map_err(|_| invalid())?;
                c.execute("INSERT INTO ledgerlab.r3_scope_locks(journal,class,full_key) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",&[&jk,&tag,key]).await?;
                c.query_one("SELECT full_key FROM ledgerlab.r3_scope_locks WHERE journal=$1 AND class=$2 AND full_key=$3 FOR UPDATE",&[&jk,&tag,key]).await?;
            }
        }
    }
    // Lock existing journal row as well: a waiter with an old SERIALIZABLE
    // snapshot must conflict before resolving a head advanced by another writer.
    c.query_opt(
        "SELECT ordinal FROM ledgerlab.r3_journals WHERE journal=$1 FOR UPDATE",
        &[&jk],
    )
    .await?;
    super::super::outcomes::native_record(legacy, olds)?;
    held.journal = Some(j.clone());
    held.guards = guards.to_vec();
    Ok(())
}

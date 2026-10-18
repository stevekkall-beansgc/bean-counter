//! SAME fence transition with the consuming original's resource owner.
//! This owns representations and I/O; it grants no journal eligibility.
use super::*;
use crate::store::sqlite::workflow::financial::FinancialReadError as E;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedByteBuilder, AuditedBytes, ExternalBufferLease},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession as Session, RepresentationKind as K,
        ValidationVisit as V,
    },
};
use serde_json::Value;
type Result<T> = std::result::Result<T, E>;
fn native<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|e| E::Store(e.into()))
}
type Head = (Vec<u8>, Vec<u8>, String, String);
pub(in crate::store::sqlite) struct PaidWitness {
    witness: Witness,
    _storage: ExternalBufferLease,
}
fn check(ok: bool, s: &mut Session) -> Result<()> {
    s.semantic_work().visit(V::Constraint)?;
    if !ok {
        return Err(invalid().into());
    }
    Ok(())
}
fn equal(a: &str, b: &str, s: &mut Session) -> Result<bool> {
    s.semantic_work().scan_bytes(a.len())?;
    s.semantic_work().scan_bytes(b.len())?;
    s.semantic_work().visit(V::MemberCompare)?;
    Ok(a == b)
}
fn bounded(n: i64, max: i64, s: &mut Session) -> Result<()> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &max.into()])?;
    check(n <= max, s)
}
fn copy(text: &str, s: &mut Session) -> Result<String> {
    s.semantic_work().scan_bytes(text.len())?;
    s.semantic_work().write_bytes(text.len())?;
    let mut value = String::new();
    value.try_reserve_exact(text.len()).map_err(|_| invalid())?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&value.capacity().into(), &text.len().into()])?;
    check(value.capacity() == text.len(), s)?;
    value.push_str(text);
    Ok(value)
}
fn append(out: &mut AuditedByteBuilder, bytes: &[u8], s: &mut Session) -> Result<()> {
    out.append(bytes, s.semantic_work())?;
    Ok(())
}
fn quoted(out: &mut AuditedByteBuilder, text: &str, s: &mut Session) -> Result<()> {
    s.semantic_work().arithmetic(
        Op::Add,
        &[&std::mem::size_of::<Value>().into(), &text.len().into()],
    )?;
    let _temporary = s.reserve_external_payload(std::mem::size_of::<Value>() + text.len())?;
    s.semantic_work()
        .write_bytes(std::mem::size_of::<Value>())?;
    let value = Value::String(copy(text, s)?);
    let bytes = s.canonical_representation(&value, K::SourceDefinition)?;
    append(out, bytes.bytes(), s)
}
fn number(out: &mut AuditedByteBuilder, value: u8, s: &mut Session) -> Result<()> {
    // Fixed stack decimal producer, exactly the serde u8 spelling.
    let mut digits = [0u8; 3];
    s.semantic_work()
        .arithmetic(Op::Compare, &[&value.into(), &100u8.into()])?;
    let used = if value >= 100 {
        3
    } else {
        s.semantic_work()
            .arithmetic(Op::Compare, &[&value.into(), &10u8.into()])?;
        if value >= 10 { 2 } else { 1 }
    };
    let mut n = value;
    for digit in digits[..used].iter_mut().rev() {
        s.semantic_work().visit(V::Element)?;
        s.semantic_work()
            .arithmetic(Op::DivideRemainder, &[&n.into(), &10u8.into()])?;
        let remainder = n % 10;
        s.semantic_work()
            .arithmetic(Op::Add, &[&b'0'.into(), &remainder.into()])?;
        s.semantic_work().write_bytes(1)?;
        *digit = b'0' + remainder;
        s.semantic_work()
            .arithmetic(Op::DivideRemainder, &[&n.into(), &10u8.into()])?;
        n /= 10;
    }
    append(out, &digits[..used], s)
}
fn octets(out: &mut AuditedByteBuilder, raw: &[u8], s: &mut Session) -> Result<()> {
    append(out, b"[", s)?;
    for (i, n) in raw.iter().enumerate() {
        s.semantic_work().visit(V::Element)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&i.into(), &0usize.into()])?;
        if i != 0 {
            append(out, b",", s)?;
        }
        number(out, *n, s)?;
    }
    append(out, b"]", s)
}
fn witness(out: &mut AuditedByteBuilder, w: &Witness, s: &mut Session) -> Result<()> {
    append(out, b"{\"hash\":", s)?;
    quoted(out, &w.hash, s)?;
    append(out, b"}", s)
}
fn preimage(
    id: &str,
    old: &Witness,
    nonce: &[u8; 32],
    heads: &[Head],
    s: &mut Session,
) -> Result<AuditedBytes> {
    let (arena, work) = s.parts();
    let mut out = arena.byte_builder(16384, work)?;
    append(&mut out, b"[", s)?;
    quoted(&mut out, id, s)?;
    append(&mut out, b",", s)?;
    witness(&mut out, old, s)?;
    append(&mut out, b",", s)?;
    octets(&mut out, nonce, s)?;
    append(&mut out, b",[", s)?;
    for (i, (journal, ordinal, segment, root)) in heads.iter().enumerate() {
        s.semantic_work().visit(V::Element)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&i.into(), &0usize.into()])?;
        if i != 0 {
            append(&mut out, b",", s)?;
        }
        append(&mut out, b"[", s)?;
        octets(&mut out, journal, s)?;
        append(&mut out, b",", s)?;
        octets(&mut out, ordinal, s)?;
        append(&mut out, b",", s)?;
        quoted(&mut out, segment, s)?;
        append(&mut out, b",", s)?;
        quoted(&mut out, root, s)?;
        append(&mut out, b"]", s)?;
    }
    append(&mut out, b"]]", s)?;
    Ok(out.finish(s.semantic_work())?)
}
fn encoded_record(id: &str, state: &State, s: &mut Session) -> Result<AuditedBytes> {
    let (arena, work) = s.parts();
    let mut out = arena.byte_builder(8192, work)?;
    // Preserve the existing derived Record/State field order, including tags.
    append(&mut out, b"{\"version\":1,\"anchor\":", s)?;
    quoted(&mut out, id, s)?;
    append(&mut out, b",\"state\":{\"state\":", s)?;
    s.semantic_work().visit(V::Constraint)?;
    match state {
        State::Stable { witness: w } => {
            append(&mut out, b"\"Stable\",\"witness\":", s)?;
            witness(&mut out, w, s)?;
        }
        State::Pending { old, new } => {
            append(&mut out, b"\"Pending\",\"old\":", s)?;
            witness(&mut out, old, s)?;
            append(&mut out, b",\"new\":", s)?;
            witness(&mut out, new, s)?;
        }
    }
    append(&mut out, b"}}", s)?;
    Ok(out.finish(s.semantic_work())?)
}
impl Fence {
    fn publish_with_session(
        &self,
        state: State,
        storage: ExternalBufferLease,
        s: &mut Session,
    ) -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let bytes = encoded_record(&self.id, &state, s)?;
        let path_bytes = self.directory.as_os_str().as_encoded_bytes().len();
        s.semantic_work()
            .arithmetic(Op::Add, &[&path_bytes.into(), &11usize.into()])?;
        let path_max = path_bytes + 11;
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&path_max.into(), &2usize.into()])?;
        s.semantic_work().arithmetic(
            Op::Add,
            &[
                &(2 * path_max).into(),
                &(2 * std::mem::size_of::<PathBuf>()).into(),
            ],
        )?;
        let _paths =
            s.reserve_external_payload(2 * path_max + 2 * std::mem::size_of::<PathBuf>())?;
        s.semantic_work().scan_bytes(path_bytes)?;
        s.semantic_work().scan_bytes(path_bytes)?;
        s.semantic_work().write_bytes(path_max)?;
        s.semantic_work().write_bytes(path_max)?;
        s.semantic_work()
            .write_bytes(2 * std::mem::size_of::<PathBuf>())?;
        // Reserve final path capacity BEFORE copying either component. A
        // clone-then-grow join would require a separate old/new overlap.
        let mut staging = PathBuf::new();
        let mut target = PathBuf::new();
        for path in [&mut staging, &mut target] {
            s.semantic_work().visit(V::Element)?;
            path.try_reserve_exact(path_max).map_err(|_| invalid())?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&path.capacity().into(), &path_max.into()])?;
            check(path.capacity() <= path_max, s)?;
            path.push(&self.directory);
        }
        s.semantic_work().scan_bytes(20)?;
        staging.push("state.next");
        target.push("state.json");
        s.semantic_work().visit(V::RefLookup)?;
        reject_link(&staging)?;
        s.semantic_work().visit(V::RefLookup)?;
        let mut file = native(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&staging),
        )?;
        s.semantic_work().scan_bytes(bytes.bytes().len())?;
        s.semantic_work().visit(V::RefLookup)?;
        native(file.write_all(bytes.bytes()))?;
        s.semantic_work().visit(V::RefLookup)?;
        native(file.sync_all())?;
        s.semantic_work().visit(V::RefLookup)?;
        native(fs::rename(staging, target))?;
        s.semantic_work().visit(V::RefLookup)?;
        let directory = native(File::open(&self.directory))?;
        s.semantic_work().visit(V::RefLookup)?;
        native(directory.sync_all())?;
        // The persistent cloned hash strings keep their original owner beyond
        // the worker/session; release only when the actual state is replaced.
        let mut current = self.state.lock().map_err(|_| invalid())?;
        let mut retained = self.state_storage.lock().map_err(|_| invalid())?;
        *current = Some(state);
        *retained = Some(storage);
        Ok(())
    }
    pub(in crate::store::sqlite) async fn prepare_with_session(
        &self,
        c: &mut SqliteConnection,
        s: &mut Session,
    ) -> Result<PaidWitness> {
        type Limits = (i64, i64, i64, i64);
        const LIMIT_STORAGE: usize =
            std::mem::size_of::<Vec<Limits>>() + 4 * std::mem::size_of::<Limits>();
        // Native TEXT<=256 is not a UTF8 output bound. Pinned SQLite
        // MemTranslate requests2*n+1; retain native statement/row copies,
        // conversion+terminator and owned UTF8 separately in this conservative
        // 8*n+2 per-cell envelope, including every decoding refusal prefix.
        const TEXT_STORAGE: usize = 8 * 256 + 2;
        const HEAD_STORAGE: usize = 2 * (1115 + 16 + 2 * TEXT_STORAGE)
            + 4 * std::mem::size_of::<Head>()
            + std::mem::size_of::<Vec<Head>>();
        const IO_STORAGE: usize = HEAD_STORAGE
            + LIMIT_STORAGE
            + 2 * TEXT_STORAGE
            + 2 * std::mem::size_of::<String>()
            + 8 * std::mem::size_of::<sqlx::sqlite::SqliteArgumentValue>()
            + std::mem::size_of::<sqlx::sqlite::SqliteQueryResult>();
        let _inputs = s.reserve_external_payload(IO_STORAGE)?;
        s.semantic_work()
            .write_bytes(4 * std::mem::size_of::<i64>())?;
        s.semantic_work().visit(V::RefLookup)?;
        let (a,b):(i64,i64)=sqlx::query_as("SELECT octet_length(anchor),octet_length(hash) FROM r3_commit_witness WHERE singleton=1").fetch_one(&mut*c).await?;
        bounded(a, 256, s)?;
        bounded(b, 256, s)?;
        s.semantic_work().scan_bytes(2 * TEXT_STORAGE)?;
        s.semantic_work()
            .write_bytes(2 * TEXT_STORAGE + 2 * std::mem::size_of::<String>())?;
        s.semantic_work().visit(V::RefLookup)?;
        let (id, hash): (String, String) =
            sqlx::query_as("SELECT anchor,hash FROM r3_commit_witness WHERE singleton=1")
                .fetch_one(&mut *c)
                .await?;
        check(equal(&id, &self.id, s)?, s)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&hash.len().into(), &64usize.into()])?;
        check(hash.len() == 64, s)?;
        let old = Witness { hash };
        {
            let current = self.state.lock().map_err(|_| invalid())?;
            s.semantic_work().visit(V::Constraint)?;
            let Some(State::Stable { witness }) = current.as_ref() else {
                return Err(invalid().into());
            };
            check(equal(&witness.hash, &old.hash, s)?, s)?;
        }
        let mut nonce = [0u8; 32];
        s.semantic_work().write_bytes(32)?;
        s.semantic_work().visit(V::RefLookup)?;
        native(native(File::open("/dev/urandom"))?.read_exact(&mut nonce))?;
        // Probe columns directly: a projected-text subquery could materialize
        // a NUL-tailed corrupt TEXT before an outer octet_length sees it.
        s.semantic_work()
            .write_bytes(std::mem::size_of::<Vec<Limits>>() + 4 * std::mem::size_of::<Limits>())?;
        s.semantic_work().visit(V::RefLookup)?;
        let limits:Vec<Limits>=sqlx::query_as("SELECT octet_length(journal),octet_length(ordinal),octet_length(segment),octet_length(replay_root) FROM r3_journals ORDER BY journal LIMIT 2").fetch_all(&mut*c).await?;
        for limits in limits {
            s.semantic_work().visit(V::Element)?;
            bounded(limits.0, 1115, s)?;
            bounded(limits.1, 16, s)?;
            bounded(limits.2, 256, s)?;
            bounded(limits.3, 256, s)?;
        }
        s.semantic_work().scan_bytes(HEAD_STORAGE)?;
        s.semantic_work().write_bytes(HEAD_STORAGE)?;
        s.semantic_work().visit(V::RefLookup)?;
        let heads: Vec<Head> = sqlx::query_as(
            "SELECT journal,ordinal,segment,replay_root FROM r3_journals ORDER BY journal LIMIT 2",
        )
        .fetch_all(&mut *c)
        .await?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&heads.len().into(), &1usize.into()])?;
        check(heads.len() <= 1, s)?;
        let bytes = preimage(&self.id, &old, &nonce, &heads, s)?;
        let hash = s.hash_representation(bytes.bytes(), K::SourceDefinition)?;
        let returned = s.reserve_external_payload(
            64 + std::mem::size_of::<PaidWitness>() + std::mem::size_of::<State>(),
        )?;
        s.semantic_work()
            .write_bytes(64 + std::mem::size_of::<PaidWitness>())?;
        let mut text = String::new();
        text.try_reserve_exact(64).map_err(|_| invalid())?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&text.capacity().into(), &64usize.into()])?;
        check(text.capacity() == 64, s)?;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in hash {
            s.semantic_work().visit(V::Element)?;
            s.semantic_work()
                .arithmetic(Op::DivideRemainder, &[&byte.into(), &16u8.into()])?;
            let high = byte / 16;
            s.semantic_work()
                .arithmetic(Op::DivideRemainder, &[&byte.into(), &16u8.into()])?;
            let low = byte % 16;
            s.semantic_work().visit(V::RefLookup)?;
            s.semantic_work().visit(V::RefLookup)?;
            text.push(HEX[high as usize] as char);
            text.push(HEX[low as usize] as char);
        }
        let new = Witness { hash: text };
        s.semantic_work().scan_bytes(new.hash.len())?;
        s.semantic_work().scan_bytes(self.id.len())?;
        s.semantic_work().scan_bytes(old.hash.len())?;
        s.semantic_work().write_bytes(IO_STORAGE)?;
        s.semantic_work().visit(V::RefLookup)?;
        let changed = sqlx::query(
            "UPDATE r3_commit_witness SET hash=? WHERE singleton=1 AND anchor=? AND hash=?",
        )
        .bind(&new.hash)
        .bind(&self.id)
        .bind(&old.hash)
        .execute(c)
        .await?
        .rows_affected();
        s.semantic_work()
            .arithmetic(Op::Compare, &[&changed.into(), &1u64.into()])?;
        check(changed == 1, s)?;
        let state_storage = s.reserve_external_payload(128 + std::mem::size_of::<State>())?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<State>())?;
        let pending = State::Pending {
            old: Witness {
                hash: copy(&old.hash, s)?,
            },
            new: Witness {
                hash: copy(&new.hash, s)?,
            },
        };
        self.publish_with_session(pending, state_storage, s)?;
        Ok(PaidWitness {
            witness: new,
            _storage: returned,
        })
    }
    pub(in crate::store::sqlite) fn finish_with_session(
        &self,
        new: PaidWitness,
        s: &mut Session,
    ) -> Result<()> {
        // Move the live hash and its storage into persistent STABLE, no clone.
        s.require_external_owner(&new._storage)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<State>())?;
        let PaidWitness { witness, _storage } = new;
        self.publish_with_session(State::Stable { witness }, _storage, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::audit_arena::AuditArena;
    #[test]
    fn resource_fence_exact_existing_serde_frames() {
        let arena = AuditArena::new().unwrap();
        let mut s = Session::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let id = "a".repeat(64);
        let old = Witness {
            hash: "b".repeat(64),
        };
        let nonce = std::array::from_fn(|i| (i * 8) as u8);
        let heads = vec![(
            vec![0, 1, 9, 10, 31, 99, 100, 255],
            vec![0; 16],
            "é\n\u{0000}\"\\".into(),
            "c".repeat(64),
        )];
        let expected = serde_json::to_vec(&(&id, &old, nonce, &heads)).unwrap();
        let bytes = preimage(&id, &old, &nonce, &heads, &mut s).unwrap();
        assert_eq!(bytes.bytes(), expected);
        drop(bytes);
        for state in [
            State::Stable {
                witness: old.clone(),
            },
            State::Pending {
                old: old.clone(),
                new: Witness {
                    hash: "d".repeat(64),
                },
            },
        ] {
            let expected = serde_json::to_vec(&Record {
                version: 1,
                anchor: id.clone(),
                state: state.clone(),
            })
            .unwrap();
            let bytes = encoded_record(&id, &state, &mut s).unwrap();
            assert_eq!(bytes.bytes(), expected);
            drop(bytes);
        }
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}

//! Fixed workflow response endpoint, never admission or a commit capability.
//! Render while the responsible original is live; retain owners through ACK
//! and the actual CLI write instead of serializing an escaping naked Value.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedByteBuilder, AuditedBytes, ExternalBufferLease},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind as K,
        ValidationVisit as V,
    },
};
use std::io::Write;

pub struct WorkflowReply {
    value: Value,
    compact: AuditedBytes,
    pretty: AuditedBytes,
    _storage: ExternalBufferLease,
    _stdout: ExternalBufferLease,
}
impl std::fmt::Debug for WorkflowReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkflowReply(owned)")
    }
}
impl WorkflowReply {
    /// Borrowed rendering, no new parser/formatter/heap copy in the CLI.
    pub fn json_bytes(&self) -> &[u8] {
        self.compact.bytes()
    }
    pub fn pretty_bytes(&self) -> &[u8] {
        self.pretty.bytes()
    }
    /// One actual response write retains all owners, including native output
    /// workspace, through the writer's acknowledgement. Failure is response
    /// loss after commit; it cannot manufacture a rollback or another receipt.
    pub fn write_stdout(self, pretty: bool) -> std::io::Result<()> {
        let mut output = std::io::stdout().lock();
        output.write_all(if pretty {
            self.pretty_bytes()
        } else {
            self.json_bytes()
        })?;
        output.write_all(b"\n")?;
        output.flush()
    }
    // Preserve the released Rust Value endpoint. Its existing caller owns that
    // returned tree; all financial creation/copies and ACK overlap are paid
    // before this final move. CLI uses write_stdout and never takes this path.
    pub(super) fn into_legacy_value(self) -> Value {
        self.value
    }
    pub(super) fn party(
        journal: &wire::PartyJournalData,
        evidence: &evidence::RequestEvidence,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        Self::from_parts(journal.receipt_bytes(), evidence.bytes(), None, false, s)
    }
    pub(super) fn original(value: &Value, s: &mut OriginalAuditSession) -> Result<Self> {
        let receipt = request_wrapper::field(value, "receipt", s)?;
        let evidence = request_wrapper::field(value, "request_evidence", s)?;
        let receipt = s
            .canonical_representation(receipt, K::ReceiptFrame)
            .map_err(core_error)?;
        let evidence = s
            .canonical_representation(evidence, K::OperationFrame)
            .map_err(core_error)?;
        Self::from_parts(receipt.bytes(), evidence.bytes(), Some("identity"), false, s)
    }
    pub(super) fn status(original:&prefix::LocatedWrappers,s:&mut OriginalAuditSession)->Result<Self> {
        let receipt=s.canonical_representation(original.wrappers().receipt(),K::ReceiptFrame).map_err(core_error)?;
        let evidence=request_wrapper::field(original.wrappers().operation(),"request_evidence",s)?;
        let evidence=s.canonical_representation(evidence,K::OperationFrame).map_err(core_error)?;
        Self::from_parts(receipt.bytes(),evidence.bytes(),None,true,s)
    }
    fn from_parts(
        receipt: &[u8],
        evidence: &[u8],
        duplicate: Option<&str>,
        status: bool,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        const FIELDS: usize = 5 * std::mem::size_of::<(&str, &[u8])>();
        let _fields = s.reserve_external_payload(FIELDS).map_err(core_error)?;
        s.semantic_work().write_bytes(FIELDS).map_err(core_error)?;
        s.semantic_work().visit(V::Constraint).map_err(core_error)?;
        let body = if status {
            wire::object(&[("receipt",receipt),("request_evidence",evidence),
                ("schema",b"\"ledger-operation-status/1\""),("status",b"\"committed\"")],4194304,s)?
        } else if duplicate.is_some() {
            wire::object(
                &[
                    ("duplicate_kind", b"\"identity\""),
                    ("receipt", receipt),
                    ("request_evidence", evidence),
                    ("schema", b"\"ledger-operation-result/1\""),
                    ("status", b"\"committed\""),
                ],
                4194304,
                s,
            )?
        } else {
            wire::object(
                &[
                    ("receipt", receipt),
                    ("request_evidence", evidence),
                    ("schema", b"\"ledger-operation-result/1\""),
                    ("status", b"\"committed\""),
                ],
                4194304,
                s,
            )?
        };
        let parsed = s
            .parse_representation(body.bytes(), K::Record)
            .map_err(core_error)?;
        let compact = render(parsed.value(), false, s)?;
        let pretty = render(parsed.value(), true, s)?;
        let (arena, work) = s.parts();
        let payload = arena
            .value_copy_payload_bound(parsed.value(), work)
            .map_err(core_error)?;
        let payload = add(payload, std::mem::size_of::<Self>(), s)?;
        let storage = s.reserve_external_payload(payload).map_err(core_error)?;
        s.semantic_work().write_bytes(payload).map_err(core_error)?;
        // SAME paid clone-footprint traversal above, same dictionary/tree.
        let value = parsed.value().clone();
        // Existing native I/O workspace, not a new quota. Prepay both format
        // lengths conservatively before commit; CLI selects one, with no
        // unmetered String/formatting allocation after session transfer.
        const STDOUT_CONTROLS: usize = std::mem::size_of::<std::io::Stdout>()
            + std::mem::size_of::<std::io::StdoutLock<'static>>()
            + std::mem::size_of::<std::io::Result<()>>();
        let stdout = s
            .reserve_external_payload(262144 + STDOUT_CONTROLS)
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(STDOUT_CONTROLS)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(compact.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(pretty.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(compact.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(pretty.bytes().len())
            .map_err(core_error)?;
        s.semantic_work().scan_bytes(1).map_err(core_error)?;
        s.semantic_work().write_bytes(1).map_err(core_error)?;
        s.semantic_work().visit(V::Constraint).map_err(core_error)?;
        s.semantic_work().visit(V::RefLookup).map_err(core_error)?;
        s.semantic_work().visit(V::RefLookup).map_err(core_error)?;
        s.semantic_work().visit(V::RefLookup).map_err(core_error)?;
        Ok(Self {
            value,
            compact,
            pretty,
            _storage: storage,
            _stdout: stdout,
        })
    }
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
enum Frame<'a> {
    Value(&'a Value, usize),
    Array(
        std::iter::Enumerate<std::slice::Iter<'a, Value>>,
        usize,
        bool,
    ),
    Object(std::iter::Enumerate<serde_json::map::Iter<'a>>, usize, bool),
}
struct Sink {
    length: usize,
    bytes: Option<AuditedByteBuilder>,
}
impl Sink {
    fn write(&mut self, raw: &[u8], s: &mut OriginalAuditSession) -> Result<()> {
        self.length = add(self.length, raw.len(), s)?;
        s.semantic_work().visit(V::Constraint).map_err(core_error)?;
        if let Some(bytes) = self.bytes.as_mut() {
            bytes.append(raw, s.semantic_work()).map_err(core_error)?;
        }
        Ok(())
    }
    fn indent(&mut self, depth: usize, s: &mut OriginalAuditSession) -> Result<()> {
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&depth.into(), &2usize.into()])
            .map_err(core_error)?;
        let length = depth.checked_mul(2).ok_or(ServiceError::Unavailable)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&length.into(), &64usize.into()])
            .map_err(core_error)?;
        require(length <= 64, "INTEGRITY_FAILURE")?;
        const SPACES: [u8; 64] = [b' '; 64];
        self.write(&SPACES[..length], s)
    }
}
fn push<'a>(
    stack: &mut Vec<Frame<'a>>,
    frame: Frame<'a>,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    s.semantic_work()
        .write_bytes(std::mem::size_of::<Frame>())
        .map_err(core_error)?;
    stack.push(frame);
    Ok(())
}
fn walk(value: &Value, pretty: bool, sink: &mut Sink, s: &mut OriginalAuditSession) -> Result<()> {
    // Strict existing owned parser/copy depth<=32. Array/object continuation
    // plus child uses at most34 fixed frames, no recursion/hidden tree stack.
    const MAX: usize = 34;
    let payload = MAX * std::mem::size_of::<Frame>() + 2 * std::mem::size_of::<Vec<Frame>>();
    let _frames = s.reserve_external_payload(payload).map_err(core_error)?;
    s.semantic_work().write_bytes(payload).map_err(core_error)?;
    let mut stack = Vec::new();
    stack
        .try_reserve_exact(MAX)
        .map_err(|_| ServiceError::Unavailable)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&stack.capacity().into(), &MAX.into()])
        .map_err(core_error)?;
    require(stack.capacity() <= MAX, "INTEGRITY_FAILURE")?;
    push(&mut stack, Frame::Value(value, 0), s)?;
    loop {
        s.semantic_work().visit(V::Constraint).map_err(core_error)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<Option<Frame>>())
            .map_err(core_error)?;
        let Some(frame) = stack.pop() else { break };
        s.semantic_work().visit(V::Element).map_err(core_error)?;
        match frame {
            Frame::Value(value, depth) => {
                s.semantic_work().visit(V::Value).map_err(core_error)?;
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&depth.into(), &32usize.into()])
                    .map_err(core_error)?;
                require(depth <= 32, "INTEGRITY_FAILURE")?;
                match value {
                    Value::Array(values) => {
                        s.semantic_work()
                            .arithmetic(Op::Compare, &[&values.len().into(), &0usize.into()])
                            .map_err(core_error)?;
                        sink.write(b"[", s)?;
                        push(
                            &mut stack,
                            Frame::Array(values.iter().enumerate(), depth, !values.is_empty()),
                            s,
                        )?;
                    }
                    Value::Object(values) => {
                        s.semantic_work()
                            .arithmetic(Op::Compare, &[&values.len().into(), &0usize.into()])
                            .map_err(core_error)?;
                        sink.write(b"{", s)?;
                        push(
                            &mut stack,
                            Frame::Object(values.iter().enumerate(), depth, !values.is_empty()),
                            s,
                        )?;
                    }
                    Value::Null => sink.write(b"null", s)?,
                    scalar => {
                        let raw = s
                            .canonical_representation(scalar, K::Record)
                            .map_err(core_error)?;
                        sink.write(raw.bytes(), s)?;
                    }
                }
            }
            Frame::Array(mut iter, depth, nonempty) => {
                s.semantic_work().visit(V::Constraint).map_err(core_error)?;
                s.semantic_work()
                    .write_bytes(std::mem::size_of::<Frame>())
                    .map_err(core_error)?;
                if let Some((index, value)) = iter.next() {
                    s.semantic_work()
                        .arithmetic(Op::Compare, &[&index.into(), &0usize.into()])
                        .map_err(core_error)?;
                    if index != 0 {
                        sink.write(b",", s)?;
                    }
                    let next = add(depth, 1, s)?;
                    s.semantic_work().visit(V::Constraint).map_err(core_error)?;
                    if pretty {
                        sink.write(b"\n", s)?;
                        sink.indent(next, s)?;
                    }
                    push(&mut stack, Frame::Array(iter, depth, nonempty), s)?;
                    push(&mut stack, Frame::Value(value, next), s)?;
                } else {
                    s.semantic_work().visit(V::Constraint).map_err(core_error)?;
                    if pretty && nonempty {
                        sink.write(b"\n", s)?;
                        sink.indent(depth, s)?;
                    }
                    sink.write(b"]", s)?;
                }
            }
            Frame::Object(mut iter, depth, nonempty) => {
                s.semantic_work().visit(V::Constraint).map_err(core_error)?;
                s.semantic_work()
                    .write_bytes(std::mem::size_of::<Frame>())
                    .map_err(core_error)?;
                if let Some((index, (key, value))) = iter.next() {
                    s.semantic_work().visit(V::Field).map_err(core_error)?;
                    s.semantic_work()
                        .arithmetic(Op::Compare, &[&index.into(), &0usize.into()])
                        .map_err(core_error)?;
                    if index != 0 {
                        sink.write(b",", s)?;
                    }
                    let next = add(depth, 1, s)?;
                    s.semantic_work().visit(V::Constraint).map_err(core_error)?;
                    if pretty {
                        sink.write(b"\n", s)?;
                        sink.indent(next, s)?;
                    }
                    let key = wire::string(key, s)?;
                    sink.write(key.bytes(), s)?;
                    sink.write(if pretty { b": " } else { b":" }, s)?;
                    push(&mut stack, Frame::Object(iter, depth, nonempty), s)?;
                    push(&mut stack, Frame::Value(value, next), s)?;
                } else {
                    s.semantic_work().visit(V::Constraint).map_err(core_error)?;
                    if pretty && nonempty {
                        sink.write(b"\n", s)?;
                        sink.indent(depth, s)?;
                    }
                    sink.write(b"}", s)?;
                }
            }
        }
    }
    Ok(())
}
fn render(value: &Value, pretty: bool, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    const CONTROLS: usize =
        2 * std::mem::size_of::<Sink>() + std::mem::size_of::<AuditedByteBuilder>();
    let _controls = s.reserve_external_payload(CONTROLS).map_err(core_error)?;
    s.semantic_work()
        .write_bytes(CONTROLS)
        .map_err(core_error)?;
    let mut measure = Sink {
        length: 0,
        bytes: None,
    };
    walk(value, pretty, &mut measure, s)?;
    let (arena, work) = s.parts();
    let output = arena
        .byte_builder(measure.length, work)
        .map_err(core_error)?;
    let mut actual = Sink {
        length: 0,
        bytes: Some(output),
    };
    walk(value, pretty, &mut actual, s)?;
    actual
        .bytes
        .unwrap()
        .finish(s.semantic_work())
        .map_err(core_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::audit_arena::AuditArena;
    #[test]
    fn owning_reply_matches_existing_compact_pretty_and_release() {
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        // Representation-only data, no authority/receipt admission fixture.
        let receipt = json!({"empty_array":[],"empty_object":{},"nested":[true,false,null,42,{"é":"line\n\t\u{0000}\\\"","𐀀":"x","\u{e000}":"y"}]});
        let evidence = json!({"raw":"cGFpZA==","normalized":{"items":[[],{},[1,2,3]]}});
        let raw_receipt = serde_json::to_vec(&receipt).unwrap();
        let raw_evidence = serde_json::to_vec(&evidence).unwrap();
        for duplicate in [None, Some("identity")] {
            let mut expected = json!({"receipt":receipt,"request_evidence":evidence,"schema":"ledger-operation-result/1","status":"committed"});
            if duplicate.is_some() {
                expected["duplicate_kind"] = json!("identity");
            }
            let reply =
                WorkflowReply::from_parts(&raw_receipt, &raw_evidence, duplicate, false, &mut s).unwrap();
            assert_eq!(reply.json_bytes(), serde_json::to_vec(&expected).unwrap());
            assert_eq!(
                reply.pretty_bytes(),
                serde_json::to_vec_pretty(&expected).unwrap()
            );
            assert!(arena.usage().unwrap().live_bytes > baseline);
            assert_eq!(&reply.value, &expected);
            drop(reply);
            assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        }
    }
}

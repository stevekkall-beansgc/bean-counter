//! Prefix production before the observation documents have content identities.
//! The same hash seed and bounded pointer-batch kernel is used by verification.
//! Inputs and outputs are DATA: completed original membership, current custody,
//! canonical header emission and the later combined commitment remain the host's
//! responsibility. No provisional observation Ref or asserted digest is used.
use super::*;

/// Actual source seed fields, before origin_count/prefix_digest are derived.
/// `fields` is an owning-normalized borrowed object containing the seven
/// SOURCE_PREFIX_KEYS. Extra header fields are not used by this hash recipe.
pub struct SourcePrefixInput<'a> {
    pub fields: &'a Value,
    pub through: u64,
}
/// Actual held financial seed fields, before origin_count/prefix_digest exist.
pub struct FinancialPrefixInput<'a> {
    pub fields: &'a Value,
    pub through: u64,
}
pub struct ProducedOriginPrefix {
    digest: String,
    count: u64,
}
impl ProducedOriginPrefix {
    pub fn prefix_digest(&self) -> &str {
        &self.digest
    }
    pub fn origin_count(&self) -> u64 {
        self.count
    }
}
pub struct ProducedOriginPrefixes {
    sources: Vec<ProducedOriginPrefix>,
    financial: ProducedOriginPrefix,
    _ownership: ExternalBufferLease,
}
impl ProducedOriginPrefixes {
    pub fn sources(&self) -> &[ProducedOriginPrefix] {
        &self.sources
    }
    pub fn financial(&self) -> &ProducedOriginPrefix {
        &self.financial
    }
}
fn seed_key(m: &mut CoreMeter, seed: &Value, key: &OperationKey) -> Result<()> {
    let store = shape::text(m, seed, "logical_store_id")?;
    if !shape::same_text(m, store, &key.logical_store_id)? {
        return Err(fail("prefix seed store"));
    }
    let scope = shape::array(m, seed, "scope")?;
    if !w::compare(m, scope.len(), 2usize)?.is_eq() {
        return Err(fail("prefix seed scope"));
    }
    for (value, want) in scope.iter().zip(&key.scope) {
        m.visit("audit.prefix_seed_scope")?;
        let value = value
            .as_str()
            .ok_or_else(|| fail("prefix seed scope string"))?;
        if !shape::same_text(m, value, want)? {
            return Err(fail("prefix seed scope association"));
        }
    }
    Ok(())
}
impl<'a> EconomicAuditStream<'a> {
    /// Start the real first pass with its captured pending key and actual source
    /// count. This mode cannot return RecomputedAuditDigest or financial proof.
    pub fn prefix_producer(
        key: &'a OperationKey,
        source_count: usize,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let result = (|| {
            s.check_cancelled()?;
            let arena = s.arena().shared_handle();
            let m = s.meter_mut();
            if m.resource_vector_usage().is_none() {
                return Err(fail("prefix production requires RP01"));
            }
            if w::compare(m, source_count, 32usize)?.is_gt() {
                return Err(fail("prefix source count"));
            }
            ValidationWork::arithmetic(
                m,
                ArithmeticOperation::Multiply,
                &[&source_count.into(), &mem::size_of::<Prefix>().into()],
            )?;
            let bytes = source_count
                .checked_mul(mem::size_of::<Prefix>())
                .ok_or_else(|| fail("prefix source allocation"))?;
            let bytes = w::usize_add(m, bytes, mem::size_of::<Self>())?;
            let ownership = arena.reserve_external_payload(bytes, m)?;
            let mut sources = Vec::new();
            sources
                .try_reserve_exact(source_count)
                .map_err(|_| fail("prefix source vector"))?;
            Ok(Self {
                context: None,
                financial_ref: None,
                production_key: Some(key),
                sources,
                source_count,
                previous_source: None,
                financial: None,
                combined: None,
                last_sequence: None,
                union_count: 0,
                phase: 0,
                sealed: false,
                cancellation: arena.cancellation_handle(),
                ownership: Some(ownership),
            })
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
    /// Source seed order is the caller's actual source-frame order. Actual
    /// observation Ref ordering is established after these digests are emitted.
    pub fn add_source_prefix(
        &mut self,
        input: SourcePrefixInput<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<usize> {
        let result = (|| {
            self.check_owner(s)?;
            let m = s.meter_mut();
            m.visit("audit.prefix_producer_mode")?;
            let key = self
                .production_key
                .ok_or_else(|| fail("prefix production mode"))?;
            if !w::compare(m, self.phase, 0u8)?.is_eq()
                || !w::compare(m, self.sources.len(), self.source_count)?.is_lt()
            {
                return Err(fail("prefix source phase/count"));
            }
            seed_key(m, input.fields, key)?;
            let through = counter(m, input.fields, "verified_through_sequence")?;
            if !w::compare(m, through, input.through)?.is_eq() {
                return Err(fail("prefix source snapshot association"));
            }
            let hash = prefix_start(
                m,
                b"bean-counter/authority-origin-prefix/1\0",
                SOURCE_PREFIX_KEYS,
                "audit.source_header_part",
                input.fields,
            )?;
            let index = self.sources.len();
            m.write_bytes(mem::size_of::<Prefix>())?;
            self.sources.push(Prefix {
                hash,
                expected: None,
                through: input.through,
                count: 0,
            });
            Ok(index)
        })();
        self.terminal(s, result)
    }
    pub fn set_financial_prefix(
        &mut self,
        input: FinancialPrefixInput<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let result = (|| {
            self.check_owner(s)?;
            let arena = s.arena().shared_handle();
            let m = s.meter_mut();
            m.visit("audit.prefix_producer_mode")?;
            let key = self
                .production_key
                .ok_or_else(|| fail("prefix production mode"))?;
            if !w::compare(m, self.phase, 0u8)?.is_eq()
                || !w::compare(m, self.sources.len(), self.source_count)?.is_eq()
                || self.financial.is_some()
            {
                return Err(fail("prefix financial phase"));
            }
            seed_key(m, input.fields, key)?;
            let actual = operation_key_value(m, &arena, key)?;
            let pending = shape::field(m, input.fields, "pending_operation_key")?;
            shape::equal(m, pending, actual.value())?;
            let through = counter(m, input.fields, "snapshot_through_sequence")?;
            if !w::compare(m, through, input.through)?.is_eq() {
                return Err(fail("prefix snapshot association"));
            }
            let hash = prefix_start(
                m,
                b"bean-counter/financial-origin-prefix/1\0",
                FINANCIAL_PREFIX_KEYS,
                "audit.financial_header_part",
                input.fields,
            )?;
            self.financial = Some(Prefix {
                hash,
                expected: None,
                through,
                count: 0,
            });
            self.phase = 1;
            Ok(())
        })();
        self.terminal(s, result)
    }
    pub fn finish_prefixes(
        mut self,
        s: &mut OriginalAuditSession,
    ) -> Result<ProducedOriginPrefixes> {
        let result = (|| {
            self.check_owner(s)?;
            let arena = s.arena().shared_handle();
            let m = s.meter_mut();
            m.visit("audit.prefix_producer_mode")?;
            if self.production_key.is_none() || !w::compare(m, self.phase, 1u8)?.is_eq() {
                return Err(fail("prefix finish mode/phase"));
            }
            let count = w::usize_add(m, self.sources.len(), 1)?;
            let stride = w::usize_add(m, mem::size_of::<ProducedOriginPrefix>(), 71)?;
            ValidationWork::arithmetic(
                m,
                ArithmeticOperation::Multiply,
                &[&count.into(), &stride.into()],
            )?;
            let bytes = count
                .checked_mul(stride)
                .ok_or_else(|| fail("prefix output allocation"))?;
            let bytes = w::usize_add(m, bytes, mem::size_of::<ProducedOriginPrefixes>() + 512)?;
            let ownership = arena.reserve_external_payload(bytes, m)?;
            let mut sources = Vec::new();
            sources
                .try_reserve_exact(self.sources.len())
                .map_err(|_| fail("prefix output vector"))?;
            for state in &mut self.sources {
                m.visit("audit.prefix_finish")?;
                let value = finish_prefix(m, state)?;
                m.write_bytes(mem::size_of::<ProducedOriginPrefix>())?;
                sources.push(value);
            }
            let financial = finish_prefix(
                m,
                self.financial
                    .as_mut()
                    .ok_or_else(|| fail("prefix financial state missing"))?,
            )?;
            self.sealed = true;
            Ok(ProducedOriginPrefixes {
                sources,
                financial,
                _ownership: ownership,
            })
        })();
        if result.is_err() {
            self.cancellation.cancel();
            s.invalidate();
        }
        result
    }
}
fn finish_prefix(m: &mut CoreMeter, state: &mut Prefix<'_>) -> Result<ProducedOriginPrefix> {
    syntax(m, &mut state.hash, b"]]")?;
    let hash = state.hash.clone().finalize();
    let text = canonical::hex_with_work(&hash, m)?;
    m.write_bytes(71)?;
    let digest = format!("sha256:{text}");
    Ok(ProducedOriginPrefix {
        digest,
        count: state.count,
    })
}

//! R8 Gate-B exact virtualized materialization.
//!
//! This module preserves the R8A Q(d,n) encoding and IEEE-754 state contract
//! while replacing full-vector operation execution with exact adaptive pages,
//! bounded worker-local scratch, sound early reduction, immutable cache reuse,
//! and shared bounded tile materialization.
//!
//! It deliberately makes no portable speedup or memory-win claim. Thresholds,
//! page sizes, tile sizes, worker counts, and cache budgets are configuration,
//! not scientific constants.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use num_complex::Complex64;
use qsolqec_core::SystemSpec;
use qsolqec_fly_phi664::Phi664Codec;
use qsolqec_glassbox::{
    sha256_hex, ApproximationDeclaration, ObservableState, RepresentationIdentity, SemanticHasher,
    StateSnapshot,
};
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_ops::{Operation, OperationSupport};
use qsolqec_storage::{
    content_digest, ObservableStorage, PersistenceBoundary, PhysicalBacking, StorageExactness,
    StorageSnapshot, StorageSnapshotFacts,
};

use super::{
    decode_amplitude, encode_amplitude, expected_state_len, is_implicit_positive_zero,
    validate_capacity, validate_finite, FlyQdnError, FlyQdnState, AMPLITUDE_BYTES, ENCODING_ID,
    STATE_DIGEST_DOMAIN,
};

pub const VIRTUALIZED_REPRESENTATION_ID: &str = "fly-phi664-qdn-virtualized";
pub const VIRTUALIZED_PERSISTENCE_FORMAT: &str = "qsolqec.fly-qdn.virtual-pages";

type Payload = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualizationConfig {
    page_span: usize,
    tile_span: usize,
    sparse_max_occupancy: usize,
    bitmap_max_occupancy: usize,
    worker_count: usize,
    owner_count: u32,
    max_cached_states: usize,
    max_in_flight_generations: usize,
}

impl VirtualizationConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        page_span: usize,
        tile_span: usize,
        sparse_max_occupancy: usize,
        bitmap_max_occupancy: usize,
        worker_count: usize,
        owner_count: u32,
        max_cached_states: usize,
        max_in_flight_generations: usize,
    ) -> Result<Self, VirtualizationConfigError> {
        if page_span == 0 {
            return Err(VirtualizationConfigError::ZeroPageSpan);
        }
        if tile_span == 0 {
            return Err(VirtualizationConfigError::ZeroTileSpan);
        }
        if sparse_max_occupancy > bitmap_max_occupancy {
            return Err(VirtualizationConfigError::ThresholdOrder {
                sparse: sparse_max_occupancy,
                bitmap: bitmap_max_occupancy,
            });
        }
        if bitmap_max_occupancy > page_span {
            return Err(VirtualizationConfigError::ThresholdExceedsPage {
                bitmap: bitmap_max_occupancy,
                page_span,
            });
        }
        if worker_count == 0 {
            return Err(VirtualizationConfigError::ZeroWorkerCount);
        }
        if owner_count == 0 {
            return Err(VirtualizationConfigError::ZeroOwnerCount);
        }
        if max_cached_states == 0 {
            return Err(VirtualizationConfigError::ZeroCacheCapacity);
        }
        if max_in_flight_generations == 0 {
            return Err(VirtualizationConfigError::ZeroInFlightCapacity);
        }
        Ok(Self {
            page_span,
            tile_span,
            sparse_max_occupancy,
            bitmap_max_occupancy,
            worker_count,
            owner_count,
            max_cached_states,
            max_in_flight_generations,
        })
    }

    pub const fn page_span(self) -> usize {
        self.page_span
    }

    pub const fn tile_span(self) -> usize {
        self.tile_span
    }

    pub const fn sparse_max_occupancy(self) -> usize {
        self.sparse_max_occupancy
    }

    pub const fn bitmap_max_occupancy(self) -> usize {
        self.bitmap_max_occupancy
    }

    pub const fn worker_count(self) -> usize {
        self.worker_count
    }

    pub const fn owner_count(self) -> u32 {
        self.owner_count
    }

    pub const fn max_cached_states(self) -> usize {
        self.max_cached_states
    }

    pub const fn max_in_flight_generations(self) -> usize {
        self.max_in_flight_generations
    }

    pub fn digest(self) -> String {
        let mut hasher = SemanticHasher::new();
        hasher.update(b"qsolqec.fly-qdn.virtualization-config.v1");
        for value in [
            self.page_span as u128,
            self.tile_span as u128,
            self.sparse_max_occupancy as u128,
            self.bitmap_max_occupancy as u128,
            self.worker_count as u128,
            u128::from(self.owner_count),
            self.max_cached_states as u128,
            self.max_in_flight_generations as u128,
        ] {
            hasher.update(&value.to_be_bytes());
        }
        hasher.finalize_hex()
    }

    fn validate_for(self, spec: SystemSpec) -> Result<(), VirtualizationError> {
        if self.tile_span < spec.dimension() {
            return Err(VirtualizationError::TileTooSmallForDimension {
                tile_span: self.tile_span,
                dimension: spec.dimension(),
            });
        }
        Ok(())
    }
}

impl Default for VirtualizationConfig {
    fn default() -> Self {
        Self {
            page_span: 256,
            tile_span: 256,
            sparse_max_occupancy: 16,
            bitmap_max_occupancy: 128,
            worker_count: 4,
            owner_count: 4,
            max_cached_states: 32,
            max_in_flight_generations: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualizationConfigError {
    ZeroPageSpan,
    ZeroTileSpan,
    ThresholdOrder { sparse: usize, bitmap: usize },
    ThresholdExceedsPage { bitmap: usize, page_span: usize },
    ZeroWorkerCount,
    ZeroOwnerCount,
    ZeroCacheCapacity,
    ZeroInFlightCapacity,
}

impl std::fmt::Display for VirtualizationConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroPageSpan => f.write_str("virtual page span must be nonzero"),
            Self::ZeroTileSpan => f.write_str("worker tile span must be nonzero"),
            Self::ThresholdOrder { sparse, bitmap } => write!(
                f,
                "sparse occupancy threshold {sparse} exceeds bitmap threshold {bitmap}"
            ),
            Self::ThresholdExceedsPage { bitmap, page_span } => write!(
                f,
                "bitmap occupancy threshold {bitmap} exceeds page span {page_span}"
            ),
            Self::ZeroWorkerCount => f.write_str("virtual worker count must be nonzero"),
            Self::ZeroOwnerCount => f.write_str("coordination owner count must be nonzero"),
            Self::ZeroCacheCapacity => f.write_str("state cache capacity must be nonzero"),
            Self::ZeroInFlightCapacity => {
                f.write_str("in-flight materialization capacity must be nonzero")
            }
        }
    }
}

impl std::error::Error for VirtualizationConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdaptivePageKind {
    Empty,
    Sparse,
    Bitmap,
    Dense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageKindCounts {
    pub sparse: usize,
    pub bitmap: usize,
    pub dense: usize,
}

#[derive(Debug, Clone)]
enum AdaptivePageData {
    Empty,
    Sparse(Vec<(usize, Payload)>),
    Bitmap {
        bits: Vec<u64>,
        payloads: Vec<Payload>,
    },
    Dense {
        bits: Vec<u64>,
        payloads: Vec<Payload>,
    },
}

#[derive(Debug, Clone)]
struct AdaptivePage {
    span: usize,
    occupancy: usize,
    data: AdaptivePageData,
}

impl AdaptivePage {
    fn from_entries(
        span: usize,
        entries: Vec<(usize, Payload)>,
        config: VirtualizationConfig,
    ) -> Result<Self, VirtualizationError> {
        for (offset, _) in &entries {
            if *offset >= span {
                return Err(VirtualizationError::PageOffsetOutOfRange {
                    offset: *offset,
                    span,
                });
            }
        }
        for pair in entries.windows(2) {
            if pair[0].0 >= pair[1].0 {
                return Err(VirtualizationError::NonCanonicalPageEntries);
            }
        }

        let occupancy = entries.len();
        let data = if occupancy == 0 {
            AdaptivePageData::Empty
        } else if occupancy <= config.sparse_max_occupancy {
            AdaptivePageData::Sparse(entries)
        } else if occupancy <= config.bitmap_max_occupancy {
            let mut bits = vec![0u64; bit_words(span)];
            let mut payloads = Vec::with_capacity(occupancy);
            for (offset, payload) in entries {
                set_bit(&mut bits, offset);
                payloads.push(payload);
            }
            AdaptivePageData::Bitmap { bits, payloads }
        } else {
            let mut bits = vec![0u64; bit_words(span)];
            let mut payloads = vec![[0u8; 16]; span];
            for (offset, payload) in entries {
                set_bit(&mut bits, offset);
                payloads[offset] = payload;
            }
            AdaptivePageData::Dense { bits, payloads }
        };

        Ok(Self {
            span,
            occupancy,
            data,
        })
    }

    fn kind(&self) -> AdaptivePageKind {
        match self.data {
            AdaptivePageData::Empty => AdaptivePageKind::Empty,
            AdaptivePageData::Sparse(_) => AdaptivePageKind::Sparse,
            AdaptivePageData::Bitmap { .. } => AdaptivePageKind::Bitmap,
            AdaptivePageData::Dense { .. } => AdaptivePageKind::Dense,
        }
    }

    fn get(&self, offset: usize) -> Option<Payload> {
        if offset >= self.span {
            return None;
        }
        match &self.data {
            AdaptivePageData::Empty => None,
            AdaptivePageData::Sparse(entries) => entries
                .binary_search_by_key(&offset, |(entry_offset, _)| *entry_offset)
                .ok()
                .map(|position| entries[position].1),
            AdaptivePageData::Bitmap { bits, payloads } => {
                if !get_bit(bits, offset) {
                    return None;
                }
                let rank = bitmap_rank(bits, offset);
                Some(payloads[rank])
            }
            AdaptivePageData::Dense { bits, payloads } => {
                get_bit(bits, offset).then_some(payloads[offset])
            }
        }
    }

    fn entries(&self) -> Vec<(usize, Payload)> {
        match &self.data {
            AdaptivePageData::Empty => Vec::new(),
            AdaptivePageData::Sparse(entries) => entries.clone(),
            AdaptivePageData::Bitmap { bits, payloads } => {
                let mut output = Vec::with_capacity(self.occupancy);
                let mut payload_position = 0usize;
                for offset in 0..self.span {
                    if get_bit(bits, offset) {
                        output.push((offset, payloads[payload_position]));
                        payload_position += 1;
                    }
                }
                output
            }
            AdaptivePageData::Dense { bits, payloads } => {
                let mut output = Vec::with_capacity(self.occupancy);
                for (offset, payload) in payloads.iter().copied().enumerate().take(self.span) {
                    if get_bit(bits, offset) {
                        output.push((offset, payload));
                    }
                }
                output
            }
        }
    }

    fn deterministic_bytes(&self) -> u128 {
        let usize_bytes = std::mem::size_of::<usize>() as u128;
        match &self.data {
            AdaptivePageData::Empty => 0,
            AdaptivePageData::Sparse(entries) => {
                entries.len() as u128 * (usize_bytes + AMPLITUDE_BYTES)
            }
            AdaptivePageData::Bitmap { bits, payloads } => {
                bits.len() as u128 * 8 + payloads.len() as u128 * AMPLITUDE_BYTES
            }
            AdaptivePageData::Dense { bits, payloads } => {
                bits.len() as u128 * 8 + payloads.len() as u128 * AMPLITUDE_BYTES
            }
        }
    }
}

fn bit_words(span: usize) -> usize {
    span.div_ceil(64)
}

fn set_bit(bits: &mut [u64], offset: usize) {
    bits[offset / 64] |= 1u64 << (offset % 64);
}

fn get_bit(bits: &[u64], offset: usize) -> bool {
    bits[offset / 64] & (1u64 << (offset % 64)) != 0
}

fn bitmap_rank(bits: &[u64], offset: usize) -> usize {
    let word = offset / 64;
    let bit = offset % 64;
    let prior_words: usize = bits[..word]
        .iter()
        .map(|value| value.count_ones() as usize)
        .sum();
    let mask = if bit == 0 { 0 } else { (1u64 << bit) - 1 };
    prior_words + (bits[word] & mask).count_ones() as usize
}

#[derive(Debug, Default)]
struct PageBuilder {
    entries: BTreeMap<usize, BTreeMap<usize, Payload>>,
}

impl PageBuilder {
    fn insert(
        &mut self,
        state_len: usize,
        page_span: usize,
        index: usize,
        payload: Payload,
    ) -> Result<(), VirtualizationError> {
        if index >= state_len {
            return Err(VirtualizationError::OutputIndexOutOfRange { index, state_len });
        }
        let page_index = index / page_span;
        let offset = index % page_span;
        let page = self.entries.entry(page_index).or_default();
        if page.insert(offset, payload).is_some() {
            return Err(VirtualizationError::DuplicateOutputIndex { index });
        }
        Ok(())
    }

    fn finish(
        self,
        state_len: usize,
        config: VirtualizationConfig,
    ) -> Result<BTreeMap<usize, Arc<AdaptivePage>>, VirtualizationError> {
        let mut pages = BTreeMap::new();
        for (page_index, page_entries) in self.entries {
            let start = page_index
                .checked_mul(config.page_span)
                .ok_or(VirtualizationError::IndexArithmeticOverflow)?;
            let remaining = state_len.saturating_sub(start);
            let span = remaining.min(config.page_span);
            let entries = page_entries.into_iter().collect::<Vec<_>>();
            let page = AdaptivePage::from_entries(span, entries, config)?;
            if page.occupancy != 0 {
                pages.insert(page_index, Arc::new(page));
            }
        }
        Ok(pages)
    }
}

#[derive(Debug, Clone)]
pub struct VirtualFlyQdnState {
    spec: SystemSpec,
    codec: Phi664Codec,
    state_len: usize,
    config: VirtualizationConfig,
    pages: BTreeMap<usize, Arc<AdaptivePage>>,
    state_digest: String,
    norm_squared: f64,
}

impl VirtualFlyQdnState {
    pub fn from_amplitudes(
        codec: Phi664Codec,
        spec: SystemSpec,
        amplitudes: Vec<Complex64>,
        config: VirtualizationConfig,
    ) -> Result<Self, VirtualizationError> {
        config.validate_for(spec)?;
        let expected = expected_state_len(spec)?;
        if amplitudes.len() != expected {
            return Err(FlyQdnError::AmplitudeCountMismatch {
                expected,
                actual: amplitudes.len(),
            }
            .into());
        }
        validate_capacity(&codec, expected)?;
        validate_finite(&amplitudes)?;

        let mut builder = PageBuilder::default();
        for (index, amplitude) in amplitudes.iter().copied().enumerate() {
            if !is_implicit_positive_zero(amplitude) {
                builder.insert(
                    expected,
                    config.page_span,
                    index,
                    encode_amplitude(amplitude),
                )?;
            }
        }
        let pages = builder.finish(expected, config)?;
        Self::from_pages(codec, spec, expected, config, pages)
    }

    pub fn from_gate_a(
        state: &FlyQdnState,
        config: VirtualizationConfig,
    ) -> Result<Self, VirtualizationError> {
        let amplitudes = state.reconstruct()?;
        Self::from_amplitudes(
            state.storage().codec().clone(),
            state.spec(),
            amplitudes,
            config,
        )
    }

    pub fn basis(
        codec: Phi664Codec,
        spec: SystemSpec,
        basis_index: usize,
        config: VirtualizationConfig,
    ) -> Result<Self, VirtualizationError> {
        config.validate_for(spec)?;
        let state_len = expected_state_len(spec)?;
        validate_capacity(&codec, state_len)?;
        if basis_index >= state_len {
            return Err(FlyQdnError::BasisIndexOutOfRange {
                index: basis_index,
                state_len,
            }
            .into());
        }
        let mut builder = PageBuilder::default();
        builder.insert(
            state_len,
            config.page_span,
            basis_index,
            encode_amplitude(Complex64::new(1.0, 0.0)),
        )?;
        let pages = builder.finish(state_len, config)?;
        Self::from_pages(codec, spec, state_len, config, pages)
    }

    pub fn zero(
        codec: Phi664Codec,
        spec: SystemSpec,
        config: VirtualizationConfig,
    ) -> Result<Self, VirtualizationError> {
        Self::basis(codec, spec, 0, config)
    }

    fn from_pages(
        codec: Phi664Codec,
        spec: SystemSpec,
        state_len: usize,
        config: VirtualizationConfig,
        pages: BTreeMap<usize, Arc<AdaptivePage>>,
    ) -> Result<Self, VirtualizationError> {
        let mut state = Self {
            spec,
            codec,
            state_len,
            config,
            pages,
            state_digest: String::new(),
            norm_squared: 0.0,
        };
        state.refresh_semantic_metadata()?;
        Ok(state)
    }

    pub const fn spec(&self) -> SystemSpec {
        self.spec
    }

    pub const fn state_len(&self) -> usize {
        self.state_len
    }

    pub const fn config(&self) -> VirtualizationConfig {
        self.config
    }

    pub fn codec(&self) -> &Phi664Codec {
        &self.codec
    }

    pub fn state_digest(&self) -> &str {
        &self.state_digest
    }

    pub const fn norm_squared(&self) -> f64 {
        self.norm_squared
    }

    pub fn materialized_amplitudes(&self) -> usize {
        self.pages.values().map(|page| page.occupancy).sum()
    }

    pub fn page_kind_counts(&self) -> PageKindCounts {
        let mut counts = PageKindCounts::default();
        for page in self.pages.values() {
            match page.kind() {
                AdaptivePageKind::Empty => {}
                AdaptivePageKind::Sparse => counts.sparse += 1,
                AdaptivePageKind::Bitmap => counts.bitmap += 1,
                AdaptivePageKind::Dense => counts.dense += 1,
            }
        }
        counts
    }

    pub fn page_kind(&self, page_index: usize) -> AdaptivePageKind {
        self.pages
            .get(&page_index)
            .map_or(AdaptivePageKind::Empty, |page| page.kind())
    }

    pub fn amplitude_at(&self, index: usize) -> Result<Complex64, VirtualizationError> {
        if index >= self.state_len {
            return Err(VirtualizationError::OutputIndexOutOfRange {
                index,
                state_len: self.state_len,
            });
        }
        Ok(self.amplitude_at_validated(index))
    }

    fn amplitude_at_validated(&self, index: usize) -> Complex64 {
        let page_index = index / self.config.page_span;
        let offset = index % self.config.page_span;
        match self
            .pages
            .get(&page_index)
            .and_then(|page| page.get(offset))
        {
            Some(payload) => {
                // Fixed-size pages can only contain payloads admitted through the
                // finite-amplitude constructors or exact operation paths.
                decode_amplitude(index, &payload)
                    .expect("virtual page payload invariant must remain valid")
            }
            None => Complex64::new(0.0, 0.0),
        }
    }

    fn materialized_entries(&self) -> Vec<(usize, Payload)> {
        let mut entries = Vec::with_capacity(self.materialized_amplitudes());
        for (page_index, page) in &self.pages {
            let base = page_index * self.config.page_span;
            for (offset, payload) in page.entries() {
                entries.push((base + offset, payload));
            }
        }
        entries
    }

    pub fn reconstruct(&self) -> Result<Vec<Complex64>, VirtualizationError> {
        let mut amplitudes = Vec::new();
        amplitudes.try_reserve_exact(self.state_len).map_err(|_| {
            VirtualizationError::AllocationFailed {
                elements: self.state_len,
                kind: "full reconstruction",
            }
        })?;
        for index in 0..self.state_len {
            amplitudes.push(self.amplitude_at_validated(index));
        }
        Ok(amplitudes)
    }

    pub fn compare_gate_a(
        &self,
        gate_a: &FlyQdnState,
    ) -> Result<BitwiseParity, VirtualizationError> {
        if self.spec != gate_a.spec() || self.state_len != gate_a.state_len() {
            return Ok(BitwiseParity {
                exact_bits: false,
                first_mismatch: Some(0),
            });
        }
        let reference = gate_a.reconstruct()?;
        for (index, expected) in reference.iter().enumerate() {
            let actual = self.amplitude_at_validated(index);
            if actual.re.to_bits() != expected.re.to_bits()
                || actual.im.to_bits() != expected.im.to_bits()
            {
                return Ok(BitwiseParity {
                    exact_bits: false,
                    first_mismatch: Some(index),
                });
            }
        }
        Ok(BitwiseParity {
            exact_bits: true,
            first_mismatch: None,
        })
    }

    pub fn support_for_spec(
        spec: SystemSpec,
        operation: &Operation,
    ) -> Result<OperationSupport, VirtualizationError> {
        Ok(FlyQdnState::support_for_spec(spec, operation)?)
    }

    pub fn support_for(
        &self,
        operation: &Operation,
    ) -> Result<OperationSupport, VirtualizationError> {
        Self::support_for_spec(self.spec, operation)
    }

    pub fn logical_state_bytes(&self) -> u128 {
        self.state_len as u128 * AMPLITUDE_BYTES
    }

    pub fn tracked_resident_bytes(&self) -> u128 {
        let body_id_index = self.codec.macro_body_ids().len() as u128 * 8;
        let page_keys = self.pages.len() as u128 * std::mem::size_of::<usize>() as u128;
        let page_bytes: u128 = self
            .pages
            .values()
            .map(|page| page.deterministic_bytes())
            .sum();
        body_id_index + page_keys + page_bytes
    }

    pub fn storage_snapshot(&self) -> StorageSnapshot {
        let materialized = self.materialized_amplitudes() as u128;
        StorageSnapshot::from_facts(StorageSnapshotFacts {
            geometry: self.codec.geometry().clone(),
            source: self.codec.manifest().source_identity().clone(),
            logical_address_count: self.codec.geometry().logical_address_count(),
            materialized_address_count: materialized,
            materialized_payload_bytes: materialized * AMPLITUDE_BYTES,
            resident_working_set_bytes: self.tracked_resident_bytes(),
            backing: PhysicalBacking::Sparse,
            exactness: StorageExactness::Exact,
            persistence: PersistenceBoundary::new(VIRTUALIZED_PERSISTENCE_FORMAT, 1)
                .expect("static virtualized persistence identity is valid"),
            storage_digest: self.storage_digest(),
        })
        .expect("virtualized state maintains R6 storage invariants")
    }

    fn storage_digest(&self) -> String {
        let mut bytes = Vec::new();
        push_len_bytes(
            &mut bytes,
            self.codec.manifest().source_identity().digest().as_bytes(),
        );
        push_len_bytes(&mut bytes, self.codec.geometry().digest().as_bytes());
        push_len_bytes(&mut bytes, self.config.digest().as_bytes());
        for (page_index, page) in &self.pages {
            bytes.extend_from_slice(&(*page_index as u128).to_be_bytes());
            bytes.push(match page.kind() {
                AdaptivePageKind::Empty => 0,
                AdaptivePageKind::Sparse => 1,
                AdaptivePageKind::Bitmap => 2,
                AdaptivePageKind::Dense => 3,
            });
            for (offset, payload) in page.entries() {
                bytes.extend_from_slice(&(offset as u128).to_be_bytes());
                bytes.extend_from_slice(&payload);
            }
        }
        content_digest(&bytes)
    }

    fn refresh_semantic_metadata(&mut self) -> Result<(), VirtualizationError> {
        let mut hasher = SemanticHasher::new();
        hasher.update(STATE_DIGEST_DOMAIN);
        hasher.update(&(self.spec.dimension() as u128).to_be_bytes());
        hasher.update(&(self.spec.subsystems() as u128).to_be_bytes());
        hasher.update(&[1]);
        hasher.update(ENCODING_ID.as_bytes());

        let mut norm_squared = 0.0;
        for index in 0..self.state_len {
            let amplitude = self.amplitude_at_validated(index);
            hasher.update(&amplitude.re.to_bits().to_be_bytes());
            hasher.update(&amplitude.im.to_bits().to_be_bytes());
            norm_squared += amplitude.norm_sqr();
        }
        self.state_digest = hasher.finalize_hex();
        self.norm_squared = norm_squared;
        Ok(())
    }
}

impl ObservableState for VirtualFlyQdnState {
    fn observation_snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            representation: RepresentationIdentity {
                id: VIRTUALIZED_REPRESENTATION_ID.into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            system: self.spec,
            approximation: ApproximationDeclaration::Exact,
            state_digest: self.state_digest.clone(),
            norm_squared: self.norm_squared,
            logical_bytes: self.logical_state_bytes(),
        }
    }
}

impl ObservableStorage for VirtualFlyQdnState {
    fn storage_observation_snapshot(&self) -> StorageSnapshot {
        self.storage_snapshot()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitwiseParity {
    pub exact_bits: bool,
    pub first_mismatch: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VirtualizationMetrics {
    pub operations_executed: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub invariant_reuses: u64,
    pub worker_dispatches: u64,
    pub addresses_scanned: u128,
    pub addresses_soundly_skipped: u128,
    pub fourier_lanes_executed: u128,
    pub fourier_lanes_pruned: u128,
}

#[derive(Debug)]
struct WorkerScratch {
    input_re: Vec<f64>,
    input_im: Vec<f64>,
    output_re: Vec<f64>,
    output_im: Vec<f64>,
}

impl WorkerScratch {
    fn new(capacity: usize) -> Result<Self, VirtualizationError> {
        fn touched(capacity: usize) -> Result<Vec<f64>, VirtualizationError> {
            let mut values = Vec::new();
            values.try_reserve_exact(capacity).map_err(|_| {
                VirtualizationError::AllocationFailed {
                    elements: capacity,
                    kind: "worker-local SoA scratch",
                }
            })?;
            values.resize(capacity, 0.0);
            values.clear();
            Ok(values)
        }
        Ok(Self {
            input_re: touched(capacity)?,
            input_im: touched(capacity)?,
            output_re: touched(capacity)?,
            output_im: touched(capacity)?,
        })
    }

    fn clear(&mut self) {
        self.input_re.clear();
        self.input_im.clear();
        self.output_re.clear();
        self.output_im.clear();
    }

    fn push_input(&mut self, amplitude: Complex64) {
        self.input_re.push(amplitude.re);
        self.input_im.push(amplitude.im);
    }

    fn push_output(&mut self, amplitude: Complex64) {
        self.output_re.push(amplitude.re);
        self.output_im.push(amplitude.im);
    }

    fn input(&self, position: usize) -> Complex64 {
        Complex64::new(self.input_re[position], self.input_im[position])
    }

    fn output(&self, position: usize) -> Complex64 {
        Complex64::new(self.output_re[position], self.output_im[position])
    }
}

#[derive(Debug)]
struct WorkerScratchPool {
    workers: Vec<WorkerScratch>,
    tile_span: usize,
}

impl WorkerScratchPool {
    fn new(config: VirtualizationConfig) -> Result<Self, VirtualizationError> {
        let mut workers = Vec::new();
        workers
            .try_reserve_exact(config.worker_count)
            .map_err(|_| VirtualizationError::AllocationFailed {
                elements: config.worker_count,
                kind: "worker scratch pool",
            })?;
        for _ in 0..config.worker_count {
            workers.push(WorkerScratch::new(config.tile_span)?);
        }
        Ok(Self {
            workers,
            tile_span: config.tile_span,
        })
    }

    fn worker_mut(&mut self, dispatch: usize) -> &mut WorkerScratch {
        let worker = dispatch % self.workers.len();
        &mut self.workers[worker]
    }

    fn deterministic_capacity_bytes(&self) -> u128 {
        self.workers.len() as u128 * self.tile_span as u128 * 4 * 8
    }
}

#[derive(Debug)]
pub struct VirtualExecutor {
    config: VirtualizationConfig,
    scratch: WorkerScratchPool,
    state_cache: BTreeMap<String, Arc<VirtualFlyQdnState>>,
    cache_order: VecDeque<String>,
    metrics: VirtualizationMetrics,
}

impl VirtualExecutor {
    pub fn new(config: VirtualizationConfig) -> Result<Self, VirtualizationError> {
        Ok(Self {
            config,
            scratch: WorkerScratchPool::new(config)?,
            state_cache: BTreeMap::new(),
            cache_order: VecDeque::new(),
            metrics: VirtualizationMetrics::default(),
        })
    }

    pub const fn config(&self) -> VirtualizationConfig {
        self.config
    }

    pub const fn metrics(&self) -> VirtualizationMetrics {
        self.metrics
    }

    pub fn worker_scratch_capacity_bytes(&self) -> u128 {
        self.scratch.deterministic_capacity_bytes()
    }

    pub fn apply_operation(
        &mut self,
        state: &mut VirtualFlyQdnState,
        operation: &Operation,
    ) -> Result<(), VirtualizationError> {
        self.apply_operations(state, std::slice::from_ref(operation))
    }

    pub fn apply_operations(
        &mut self,
        state: &mut VirtualFlyQdnState,
        operations: &[Operation],
    ) -> Result<(), VirtualizationError> {
        self.ensure_config(state)?;
        for operation in operations {
            match VirtualFlyQdnState::support_for_spec(state.spec, operation)? {
                OperationSupport::Exact => {}
                OperationSupport::Approximate => {
                    return Err(VirtualizationError::UnexpectedApproximateSupport)
                }
                OperationSupport::Unsupported => {
                    return Err(VirtualizationError::UnsupportedOperation {
                        kind: operation.kind(),
                    })
                }
            }
        }

        let mut candidate = state.clone();
        let mut pending = Vec::new();

        for operation in operations {
            if operation_is_bitwise_identity(candidate.spec, operation) {
                self.metrics.invariant_reuses = self.metrics.invariant_reuses.saturating_add(1);
                self.metrics.operations_executed =
                    self.metrics.operations_executed.saturating_add(1);
                continue;
            }

            let signature = operation_signature(&candidate, operation);
            if let Some(cached) = self.state_cache.get(&signature) {
                candidate = cached.as_ref().clone();
                self.metrics.cache_hits = self.metrics.cache_hits.saturating_add(1);
                self.metrics.operations_executed =
                    self.metrics.operations_executed.saturating_add(1);
                continue;
            }

            self.metrics.cache_misses = self.metrics.cache_misses.saturating_add(1);
            let next = self.apply_fresh(&candidate, operation)?;
            pending.push((signature, Arc::new(next.clone())));
            candidate = next;
            self.metrics.operations_executed = self.metrics.operations_executed.saturating_add(1);
        }

        // Publish reusable generations only after the entire requested sequence
        // has succeeded. A failed partial sequence never blesses its candidates.
        for (signature, cached) in pending {
            self.publish_cache(signature, cached);
        }

        *state = candidate;
        Ok(())
    }

    fn ensure_config(&self, state: &VirtualFlyQdnState) -> Result<(), VirtualizationError> {
        if self.config != state.config {
            return Err(VirtualizationError::ExecutorConfigMismatch);
        }
        Ok(())
    }

    fn publish_cache(&mut self, signature: String, state: Arc<VirtualFlyQdnState>) {
        if self.state_cache.contains_key(&signature) {
            return;
        }
        while self.state_cache.len() >= self.config.max_cached_states {
            if let Some(oldest) = self.cache_order.pop_front() {
                self.state_cache.remove(&oldest);
            } else {
                break;
            }
        }
        self.cache_order.push_back(signature.clone());
        self.state_cache.insert(signature, state);
    }

    fn apply_fresh(
        &mut self,
        state: &VirtualFlyQdnState,
        operation: &Operation,
    ) -> Result<VirtualFlyQdnState, VirtualizationError> {
        match operation {
            Operation::WeylX { target, shift } => self.apply_weyl_x(state, *target, *shift),
            Operation::WeylZ { target, power } => self.apply_weyl_z(state, *target, *power),
            Operation::Fourier { target } => self.apply_fourier(state, *target),
            Operation::ControlledShift {
                control,
                target,
                shift,
            } => self.apply_controlled_shift(state, *control, *target, *shift),
            Operation::Swap { a, b } => self.apply_swap(state, *a, *b),
            Operation::LocalPermutation { .. } | Operation::LocalUnitary(_) => {
                Err(VirtualizationError::UnsupportedOperation {
                    kind: operation.kind(),
                })
            }
        }
    }

    fn apply_weyl_x(
        &mut self,
        state: &VirtualFlyQdnState,
        target: usize,
        shift: usize,
    ) -> Result<VirtualFlyQdnState, VirtualizationError> {
        let dimension = state.spec.dimension();
        let stride = subsystem_stride(state.spec, target)?;
        let reduced = shift % dimension;
        let entries = state.materialized_entries();
        let mut builder = PageBuilder::default();

        self.metrics.addresses_scanned = self
            .metrics
            .addresses_scanned
            .saturating_add(entries.len() as u128);
        self.metrics.addresses_soundly_skipped = self
            .metrics
            .addresses_soundly_skipped
            .saturating_add((state.state_len - entries.len()) as u128);

        for (index, payload) in entries {
            let digit = (index / stride) % dimension;
            let destination = replace_digit(index, digit, (digit + reduced) % dimension, stride)?;
            builder.insert(state.state_len, self.config.page_span, destination, payload)?;
        }
        self.finish_operation(state, builder)
    }

    fn apply_controlled_shift(
        &mut self,
        state: &VirtualFlyQdnState,
        control: usize,
        target: usize,
        shift: usize,
    ) -> Result<VirtualFlyQdnState, VirtualizationError> {
        let dimension = state.spec.dimension();
        let control_stride = subsystem_stride(state.spec, control)?;
        let target_stride = subsystem_stride(state.spec, target)?;
        let reduced = shift % dimension;
        let entries = state.materialized_entries();
        let mut builder = PageBuilder::default();

        self.metrics.addresses_scanned = self
            .metrics
            .addresses_scanned
            .saturating_add(entries.len() as u128);
        self.metrics.addresses_soundly_skipped = self
            .metrics
            .addresses_soundly_skipped
            .saturating_add((state.state_len - entries.len()) as u128);

        for (index, payload) in entries {
            let control_digit = (index / control_stride) % dimension;
            let target_digit = (index / target_stride) % dimension;
            let delta = mul_mod(control_digit, reduced, dimension);
            let destination = replace_digit(
                index,
                target_digit,
                (target_digit + delta) % dimension,
                target_stride,
            )?;
            builder.insert(state.state_len, self.config.page_span, destination, payload)?;
        }
        self.finish_operation(state, builder)
    }

    fn apply_swap(
        &mut self,
        state: &VirtualFlyQdnState,
        a: usize,
        b: usize,
    ) -> Result<VirtualFlyQdnState, VirtualizationError> {
        let dimension = state.spec.dimension();
        let stride_a = subsystem_stride(state.spec, a)?;
        let stride_b = subsystem_stride(state.spec, b)?;
        let entries = state.materialized_entries();
        let mut builder = PageBuilder::default();

        self.metrics.addresses_scanned = self
            .metrics
            .addresses_scanned
            .saturating_add(entries.len() as u128);
        self.metrics.addresses_soundly_skipped = self
            .metrics
            .addresses_soundly_skipped
            .saturating_add((state.state_len - entries.len()) as u128);

        for (index, payload) in entries {
            let digit_a = (index / stride_a) % dimension;
            let digit_b = (index / stride_b) % dimension;
            let first = replace_digit(index, digit_a, digit_b, stride_a)?;
            let destination = replace_digit(first, digit_b, digit_a, stride_b)?;
            builder.insert(state.state_len, self.config.page_span, destination, payload)?;
        }
        self.finish_operation(state, builder)
    }

    fn apply_weyl_z(
        &mut self,
        state: &VirtualFlyQdnState,
        target: usize,
        power: usize,
    ) -> Result<VirtualFlyQdnState, VirtualizationError> {
        let dimension = state.spec.dimension();
        let stride = subsystem_stride(state.spec, target)?;
        let reduced = power % dimension;
        let mut builder = PageBuilder::default();

        // Exact signed-zero semantics make a blanket sparse skip unsound here.
        // We therefore scan the whole logical Q(d,n) state, but only through
        // bounded, reusable worker-local SoA scratch.
        for (dispatch, start) in (0..state.state_len)
            .step_by(self.config.tile_span)
            .enumerate()
        {
            let end = (start + self.config.tile_span).min(state.state_len);
            let scratch = self.scratch.worker_mut(dispatch);
            scratch.clear();
            for index in start..end {
                scratch.push_input(state.amplitude_at_validated(index));
            }
            for (offset, index) in (start..end).enumerate() {
                let digit = (index / stride) % dimension;
                let exponent = mul_mod(reduced, digit, dimension);
                let angle = std::f64::consts::TAU * exponent as f64 / dimension as f64;
                let output = scratch.input(offset) * Complex64::from_polar(1.0, angle);
                scratch.push_output(output);
            }
            for (offset, index) in (start..end).enumerate() {
                let output = scratch.output(offset);
                if !is_implicit_positive_zero(output) {
                    builder.insert(
                        state.state_len,
                        self.config.page_span,
                        index,
                        encode_amplitude(output),
                    )?;
                }
            }
            self.metrics.worker_dispatches = self.metrics.worker_dispatches.saturating_add(1);
            self.metrics.addresses_scanned = self
                .metrics
                .addresses_scanned
                .saturating_add((end - start) as u128);
        }
        self.finish_operation(state, builder)
    }

    fn apply_fourier(
        &mut self,
        state: &VirtualFlyQdnState,
        target: usize,
    ) -> Result<VirtualFlyQdnState, VirtualizationError> {
        let dimension = state.spec.dimension();
        let stride = subsystem_stride(state.spec, target)?;
        let block = stride
            .checked_mul(dimension)
            .ok_or(VirtualizationError::IndexArithmeticOverflow)?;
        let scale = 1.0 / (dimension as f64).sqrt();
        let mut active_lanes = BTreeSet::new();

        for (index, _) in state.materialized_entries() {
            let lane_base = (index / block)
                .checked_mul(block)
                .and_then(|base| base.checked_add(index % stride))
                .ok_or(VirtualizationError::IndexArithmeticOverflow)?;
            active_lanes.insert(lane_base);
        }

        let total_lanes = state.state_len / dimension;
        let pruned = total_lanes.saturating_sub(active_lanes.len());
        self.metrics.fourier_lanes_pruned = self
            .metrics
            .fourier_lanes_pruned
            .saturating_add(pruned as u128);
        self.metrics.addresses_soundly_skipped = self
            .metrics
            .addresses_soundly_skipped
            .saturating_add((pruned * dimension) as u128);

        let mut builder = PageBuilder::default();
        for (dispatch, lane_base) in active_lanes.into_iter().enumerate() {
            let scratch = self.scratch.worker_mut(dispatch);
            scratch.clear();
            for digit in 0..dimension {
                let index = lane_base
                    .checked_add(
                        digit
                            .checked_mul(stride)
                            .ok_or(VirtualizationError::IndexArithmeticOverflow)?,
                    )
                    .ok_or(VirtualizationError::IndexArithmeticOverflow)?;
                scratch.push_input(state.amplitude_at_validated(index));
            }

            for output_digit in 0..dimension {
                let mut sum = Complex64::new(0.0, 0.0);
                for input_digit in 0..dimension {
                    let exponent = mul_mod(input_digit, output_digit, dimension);
                    let angle = std::f64::consts::TAU * exponent as f64 / dimension as f64;
                    sum += scratch.input(input_digit) * Complex64::from_polar(1.0, angle);
                }
                scratch.push_output(sum * scale);
            }

            for output_digit in 0..dimension {
                let index = lane_base
                    .checked_add(
                        output_digit
                            .checked_mul(stride)
                            .ok_or(VirtualizationError::IndexArithmeticOverflow)?,
                    )
                    .ok_or(VirtualizationError::IndexArithmeticOverflow)?;
                let output = scratch.output(output_digit);
                if !is_implicit_positive_zero(output) {
                    builder.insert(
                        state.state_len,
                        self.config.page_span,
                        index,
                        encode_amplitude(output),
                    )?;
                }
            }

            self.metrics.worker_dispatches = self.metrics.worker_dispatches.saturating_add(1);
            self.metrics.fourier_lanes_executed =
                self.metrics.fourier_lanes_executed.saturating_add(1);
            self.metrics.addresses_scanned = self
                .metrics
                .addresses_scanned
                .saturating_add(dimension as u128);
        }

        self.finish_operation(state, builder)
    }

    fn finish_operation(
        &self,
        state: &VirtualFlyQdnState,
        builder: PageBuilder,
    ) -> Result<VirtualFlyQdnState, VirtualizationError> {
        let pages = builder.finish(state.state_len, self.config)?;
        VirtualFlyQdnState::from_pages(
            state.codec.clone(),
            state.spec,
            state.state_len,
            self.config,
            pages,
        )
    }
}

fn operation_is_bitwise_identity(spec: SystemSpec, operation: &Operation) -> bool {
    match operation {
        Operation::WeylX { shift, .. } => shift % spec.dimension() == 0,
        Operation::ControlledShift { shift, .. } => shift % spec.dimension() == 0,
        _ => false,
    }
}

fn operation_signature(state: &VirtualFlyQdnState, operation: &Operation) -> String {
    let mut canonical = Vec::new();
    push_len_bytes(
        &mut canonical,
        b"qsolqec.fly-qdn.virtualized-operation-generation.v1",
    );
    push_len_bytes(&mut canonical, state.state_digest.as_bytes());
    push_len_bytes(
        &mut canonical,
        state.codec.manifest().source_identity().digest().as_bytes(),
    );
    push_len_bytes(&mut canonical, state.codec.geometry().digest().as_bytes());
    push_len_bytes(&mut canonical, ENCODING_ID.as_bytes());
    push_len_bytes(&mut canonical, state.config.digest().as_bytes());
    push_len_bytes(&mut canonical, &operation.canonical_bytes());
    format!("sha256:{}", sha256_hex(&canonical))
}

fn push_len_bytes(target: &mut Vec<u8>, bytes: &[u8]) {
    target.extend_from_slice(&(bytes.len() as u128).to_be_bytes());
    target.extend_from_slice(bytes);
}

fn subsystem_stride(spec: SystemSpec, subsystem: usize) -> Result<usize, VirtualizationError> {
    let exponent =
        u32::try_from(subsystem).map_err(|_| VirtualizationError::IndexArithmeticOverflow)?;
    spec.dimension()
        .checked_pow(exponent)
        .ok_or(VirtualizationError::IndexArithmeticOverflow)
}

fn replace_digit(
    index: usize,
    old_digit: usize,
    new_digit: usize,
    stride: usize,
) -> Result<usize, VirtualizationError> {
    let old_term = old_digit
        .checked_mul(stride)
        .ok_or(VirtualizationError::IndexArithmeticOverflow)?;
    let new_term = new_digit
        .checked_mul(stride)
        .ok_or(VirtualizationError::IndexArithmeticOverflow)?;
    index
        .checked_sub(old_term)
        .and_then(|value| value.checked_add(new_term))
        .ok_or(VirtualizationError::IndexArithmeticOverflow)
}

fn mul_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 * b as u128) % modulus as u128) as usize
}

#[derive(Debug, Clone)]
pub struct MaterializedTile {
    state_digest: String,
    tile_index: usize,
    owner: u32,
    start: usize,
    amplitudes: Vec<Complex64>,
}

impl MaterializedTile {
    pub fn state_digest(&self) -> &str {
        &self.state_digest
    }

    pub const fn tile_index(&self) -> usize {
        self.tile_index
    }

    pub const fn owner(&self) -> u32 {
        self.owner
    }

    pub const fn start(&self) -> usize {
        self.start
    }

    pub fn amplitudes(&self) -> &[Complex64] {
        &self.amplitudes
    }
}

#[derive(Debug, Clone)]
pub struct TileRequest {
    owner: u32,
    deadline: Option<Instant>,
    cancellation: Option<Arc<AtomicBool>>,
}

impl TileRequest {
    pub const fn new(owner: u32) -> Self {
        Self {
            owner,
            deadline: None,
            cancellation: None,
        }
    }

    pub const fn owner(&self) -> u32 {
        self.owner
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_cancellation(mut self, cancellation: Arc<AtomicBool>) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    fn deadline_elapsed(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
}

type SharedCell = OnceLock<Result<Arc<MaterializedTile>, SharedMaterializationError>>;

#[derive(Debug)]
pub struct SharedTileMaterializer {
    state: Arc<VirtualFlyQdnState>,
    cells: Mutex<BTreeMap<usize, Arc<SharedCell>>>,
    in_flight: AtomicUsize,
    generations: AtomicU64,
    coalesced_waiters: AtomicU64,
}

impl SharedTileMaterializer {
    pub fn new(state: Arc<VirtualFlyQdnState>) -> Self {
        Self {
            state,
            cells: Mutex::new(BTreeMap::new()),
            in_flight: AtomicUsize::new(0),
            generations: AtomicU64::new(0),
            coalesced_waiters: AtomicU64::new(0),
        }
    }

    pub fn tile_count(&self) -> usize {
        self.state.state_len.div_ceil(self.state.config.tile_span)
    }

    pub fn owner_for_tile(&self, tile_index: usize) -> Result<u32, SharedMaterializationError> {
        if tile_index >= self.tile_count() {
            return Err(SharedMaterializationError::TileOutOfRange {
                tile_index,
                tile_count: self.tile_count(),
            });
        }
        Ok((tile_index as u128 % u128::from(self.state.config.owner_count)) as u32)
    }

    pub fn generation_count(&self) -> u64 {
        self.generations.load(Ordering::Acquire)
    }

    pub fn coalesced_waiter_count(&self) -> u64 {
        self.coalesced_waiters.load(Ordering::Acquire)
    }

    pub fn materialize(
        &self,
        tile_index: usize,
        request: &TileRequest,
    ) -> Result<Arc<MaterializedTile>, SharedMaterializationError> {
        self.check_request(tile_index, request)?;

        let (cell, inserted) = {
            let mut cells = self
                .cells
                .lock()
                .map_err(|_| SharedMaterializationError::CoordinationPoisoned)?;
            if let Some(existing) = cells.get(&tile_index) {
                if existing.get().is_none() {
                    self.coalesced_waiters.fetch_add(1, Ordering::AcqRel);
                }
                (Arc::clone(existing), false)
            } else {
                let current = self.in_flight.load(Ordering::Acquire);
                if current >= self.state.config.max_in_flight_generations {
                    return Err(SharedMaterializationError::AdmissionDenied {
                        in_flight: current,
                        max: self.state.config.max_in_flight_generations,
                    });
                }
                self.in_flight.fetch_add(1, Ordering::AcqRel);
                let cell = Arc::new(OnceLock::new());
                cells.insert(tile_index, Arc::clone(&cell));
                (cell, true)
            }
        };

        let result = cell
            .get_or_init(|| {
                self.generations.fetch_add(1, Ordering::AcqRel);
                let generated = self.generate_tile(tile_index);
                self.in_flight.fetch_sub(1, Ordering::AcqRel);
                generated
            })
            .clone();

        // A duplicate requester shares the same immutable cell whether it
        // arrived before or after generation completed.
        let _ = inserted;

        if request.is_cancelled() {
            return Err(SharedMaterializationError::Cancelled);
        }
        if request.deadline_elapsed() {
            return Err(SharedMaterializationError::DeadlineExceeded);
        }

        if result.is_err() {
            let mut cells = self
                .cells
                .lock()
                .map_err(|_| SharedMaterializationError::CoordinationPoisoned)?;
            if cells
                .get(&tile_index)
                .is_some_and(|existing| Arc::ptr_eq(existing, &cell))
            {
                cells.remove(&tile_index);
            }
        }
        result
    }

    fn check_request(
        &self,
        tile_index: usize,
        request: &TileRequest,
    ) -> Result<(), SharedMaterializationError> {
        if request.is_cancelled() {
            return Err(SharedMaterializationError::Cancelled);
        }
        if request.deadline_elapsed() {
            return Err(SharedMaterializationError::DeadlineExceeded);
        }
        let expected_owner = self.owner_for_tile(tile_index)?;
        if request.owner != expected_owner {
            return Err(SharedMaterializationError::OwnerMismatch {
                tile_index,
                expected: expected_owner,
                actual: request.owner,
            });
        }
        Ok(())
    }

    fn generate_tile(
        &self,
        tile_index: usize,
    ) -> Result<Arc<MaterializedTile>, SharedMaterializationError> {
        let start = tile_index
            .checked_mul(self.state.config.tile_span)
            .ok_or(SharedMaterializationError::IndexArithmeticOverflow)?;
        let end = (start + self.state.config.tile_span).min(self.state.state_len);
        let len = end.saturating_sub(start);

        let mut amplitudes = Vec::new();
        amplitudes
            .try_reserve_exact(len)
            .map_err(|_| SharedMaterializationError::AllocationFailed { elements: len })?;
        for index in start..end {
            amplitudes.push(self.state.amplitude_at_validated(index));
        }

        Ok(Arc::new(MaterializedTile {
            state_digest: self.state.state_digest.clone(),
            tile_index,
            owner: (tile_index as u128 % u128::from(self.state.config.owner_count)) as u32,
            start,
            amplitudes,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedMaterializationError {
    TileOutOfRange {
        tile_index: usize,
        tile_count: usize,
    },
    OwnerMismatch {
        tile_index: usize,
        expected: u32,
        actual: u32,
    },
    AdmissionDenied {
        in_flight: usize,
        max: usize,
    },
    Cancelled,
    DeadlineExceeded,
    AllocationFailed {
        elements: usize,
    },
    CoordinationPoisoned,
    IndexArithmeticOverflow,
}

impl std::fmt::Display for SharedMaterializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TileOutOfRange {
                tile_index,
                tile_count,
            } => write!(
                f,
                "tile {tile_index} is outside virtual tile count {tile_count}"
            ),
            Self::OwnerMismatch {
                tile_index,
                expected,
                actual,
            } => write!(
                f,
                "tile {tile_index} belongs to owner {expected}, request used owner {actual}"
            ),
            Self::AdmissionDenied { in_flight, max } => write!(
                f,
                "materialization admission denied with {in_flight} generations in flight (max {max})"
            ),
            Self::Cancelled => f.write_str("materialization request was cancelled"),
            Self::DeadlineExceeded => f.write_str("materialization deadline elapsed"),
            Self::AllocationFailed { elements } => {
                write!(f, "failed to allocate shared tile with {elements} amplitudes")
            }
            Self::CoordinationPoisoned => f.write_str("materialization coordination lock poisoned"),
            Self::IndexArithmeticOverflow => {
                f.write_str("materialization tile index arithmetic overflowed")
            }
        }
    }
}

impl std::error::Error for SharedMaterializationError {}

#[derive(Debug)]
pub enum VirtualizationError {
    Config(VirtualizationConfigError),
    GateA(FlyQdnError),
    TileTooSmallForDimension { tile_span: usize, dimension: usize },
    PageOffsetOutOfRange { offset: usize, span: usize },
    NonCanonicalPageEntries,
    DuplicateOutputIndex { index: usize },
    OutputIndexOutOfRange { index: usize, state_len: usize },
    AllocationFailed { elements: usize, kind: &'static str },
    IndexArithmeticOverflow,
    ExecutorConfigMismatch,
    UnexpectedApproximateSupport,
    UnsupportedOperation { kind: &'static str },
}

impl std::fmt::Display for VirtualizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(source) => write!(f, "invalid virtualization configuration: {source}"),
            Self::GateA(source) => write!(f, "Gate-A contract error: {source}"),
            Self::TileTooSmallForDimension {
                tile_span,
                dimension,
            } => write!(
                f,
                "worker tile span {tile_span} is smaller than local dimension {dimension}"
            ),
            Self::PageOffsetOutOfRange { offset, span } => {
                write!(f, "page offset {offset} is outside page span {span}")
            }
            Self::NonCanonicalPageEntries => {
                f.write_str("adaptive page entries are not strictly increasing")
            }
            Self::DuplicateOutputIndex { index } => {
                write!(f, "operation produced duplicate output index {index}")
            }
            Self::OutputIndexOutOfRange { index, state_len } => {
                write!(
                    f,
                    "output index {index} is outside Q(d,n) state length {state_len}"
                )
            }
            Self::AllocationFailed { elements, kind } => {
                write!(f, "failed to allocate {elements} elements for {kind}")
            }
            Self::IndexArithmeticOverflow => {
                f.write_str("virtualized operation index arithmetic overflowed")
            }
            Self::ExecutorConfigMismatch => {
                f.write_str("virtual executor configuration does not match state configuration")
            }
            Self::UnexpectedApproximateSupport => {
                f.write_str("exact Gate-B path unexpectedly reported approximate support")
            }
            Self::UnsupportedOperation { kind } => {
                write!(
                    f,
                    "operation {kind} is unsupported by the Gate-B exact adapter"
                )
            }
        }
    }
}

impl std::error::Error for VirtualizationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(source) => Some(source),
            Self::GateA(source) => Some(source),
            _ => None,
        }
    }
}

impl From<VirtualizationConfigError> for VirtualizationError {
    fn from(value: VirtualizationConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<FlyQdnError> for VirtualizationError {
    fn from(value: FlyQdnError) -> Self {
        Self::GateA(value)
    }
}

pub fn virtualized_module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: VIRTUALIZED_REPRESENTATION_ID.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            Capability::StateRepresentation,
            Capability::OperationExecution,
        ],
        consumes: vec![DataKind::EncodedState, DataKind::OperationStream],
        produces: vec![
            DataKind::EncodedState,
            DataKind::QuditState,
            DataKind::StateTransition,
            DataKind::Artifact,
        ],
        experimental: true,
        maturity: Maturity::E3OracleCompared,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsolqec_dense::DenseState;
    use qsolqec_fly_phi664::MacroSourceSpec;

    fn codec() -> Phi664Codec {
        Phi664Codec::from_body_ids(
            MacroSourceSpec {
                dataset_id: "r8b-fixture:v1".into(),
                release_date: "2026-09-22".into(),
                license: "CC0".into(),
                node_source_uri: "fixture://nodes".into(),
                edge_source_uri: "fixture://edges".into(),
                node_identity_field: "bodyId".into(),
                node_projection: "fixture bodyId set".into(),
                edge_interpretation: "fixture directed edges".into(),
                graph_projection: "fixture identity only".into(),
            },
            vec![10, 20],
        )
        .unwrap()
    }

    fn config() -> VirtualizationConfig {
        VirtualizationConfig::new(8, 8, 1, 4, 2, 2, 8, 2).unwrap()
    }

    fn assert_bits_equal(left: &[Complex64], right: &[Complex64]) {
        assert_eq!(left.len(), right.len());
        for (index, (left, right)) in left.iter().zip(right).enumerate() {
            assert_eq!(
                (left.re.to_bits(), left.im.to_bits()),
                (right.re.to_bits(), right.im.to_bits()),
                "mismatch at amplitude {index}"
            );
        }
    }

    #[test]
    fn adaptive_pages_cover_sparse_bitmap_and_dense_states() {
        let spec = SystemSpec::new(2, 3).unwrap();
        let make = |occupied: usize| {
            let mut amplitudes = vec![Complex64::new(0.0, 0.0); 8];
            for amplitude in amplitudes.iter_mut().take(occupied) {
                *amplitude = Complex64::new(1.0, 0.0);
            }
            VirtualFlyQdnState::from_amplitudes(codec(), spec, amplitudes, config()).unwrap()
        };

        assert_eq!(make(1).page_kind(0), AdaptivePageKind::Sparse);
        assert_eq!(make(3).page_kind(0), AdaptivePageKind::Bitmap);
        assert_eq!(make(6).page_kind(0), AdaptivePageKind::Dense);
    }

    #[test]
    fn permutation_path_skips_implicit_positive_zero_exactly() {
        let spec = SystemSpec::new(2, 6).unwrap();
        let mut amplitudes = vec![Complex64::new(0.0, 0.0); 64];
        amplitudes[1] = Complex64::new(0.25, -0.5);
        amplitudes[19] = Complex64::new(-0.75, 0.125);

        let mut gate_a =
            FlyQdnState::from_amplitudes(codec(), 64, spec, amplitudes.clone()).unwrap();
        let mut virtualized =
            VirtualFlyQdnState::from_amplitudes(codec(), spec, amplitudes, config()).unwrap();
        let operation = Operation::WeylX {
            target: 3,
            shift: 1,
        };
        gate_a.apply_operation(&operation).unwrap();

        let mut executor = VirtualExecutor::new(config()).unwrap();
        executor
            .apply_operation(&mut virtualized, &operation)
            .unwrap();

        assert!(virtualized.compare_gate_a(&gate_a).unwrap().exact_bits);
        assert_eq!(executor.metrics().addresses_scanned, 2);
        assert_eq!(executor.metrics().addresses_soundly_skipped, 62);
    }

    #[test]
    fn weyl_z_preserves_gate_a_signed_zero_bits() {
        let spec = SystemSpec::new(3, 3).unwrap();
        let amplitudes = vec![Complex64::new(0.0, 0.0); 27];
        let mut gate_a =
            FlyQdnState::from_amplitudes(codec(), 64, spec, amplitudes.clone()).unwrap();
        let mut virtualized =
            VirtualFlyQdnState::from_amplitudes(codec(), spec, amplitudes, config()).unwrap();
        let operation = Operation::WeylZ {
            target: 0,
            power: 1,
        };

        gate_a.apply_operation(&operation).unwrap();
        let mut executor = VirtualExecutor::new(config()).unwrap();
        executor
            .apply_operation(&mut virtualized, &operation)
            .unwrap();

        assert!(virtualized.compare_gate_a(&gate_a).unwrap().exact_bits);
        assert_eq!(executor.metrics().addresses_scanned, 27);
        assert!(virtualized.materialized_amplitudes() > 0);
    }

    #[test]
    fn fourier_prunes_only_empty_lanes_and_matches_gate_a() {
        let spec = SystemSpec::new(3, 3).unwrap();
        let mut amplitudes = vec![Complex64::new(0.0, 0.0); 27];
        amplitudes[0] = Complex64::new(1.0, 0.0);
        let mut gate_a =
            FlyQdnState::from_amplitudes(codec(), 64, spec, amplitudes.clone()).unwrap();
        let mut virtualized =
            VirtualFlyQdnState::from_amplitudes(codec(), spec, amplitudes, config()).unwrap();
        let operation = Operation::Fourier { target: 0 };

        gate_a.apply_operation(&operation).unwrap();
        let mut executor = VirtualExecutor::new(config()).unwrap();
        executor
            .apply_operation(&mut virtualized, &operation)
            .unwrap();

        assert!(virtualized.compare_gate_a(&gate_a).unwrap().exact_bits);
        assert_eq!(executor.metrics().fourier_lanes_executed, 1);
        assert_eq!(executor.metrics().fourier_lanes_pruned, 8);
        assert_eq!(executor.metrics().addresses_soundly_skipped, 24);
    }

    #[test]
    fn mixed_sequence_is_bitwise_equal_to_gate_a_and_dense() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let amplitudes = vec![
            Complex64::new(0.10, 0.02),
            Complex64::new(-0.20, 0.03),
            Complex64::new(0.05, -0.07),
            Complex64::new(0.15, 0.11),
            Complex64::new(-0.09, 0.04),
            Complex64::new(0.12, -0.08),
            Complex64::new(0.03, 0.06),
            Complex64::new(-0.02, -0.05),
            Complex64::new(0.07, 0.01),
        ];
        let operations = vec![
            Operation::WeylX {
                target: 0,
                shift: 2,
            },
            Operation::WeylZ {
                target: 1,
                power: 1,
            },
            Operation::Fourier { target: 0 },
            Operation::ControlledShift {
                control: 0,
                target: 1,
                shift: 2,
            },
            Operation::Swap { a: 0, b: 1 },
        ];

        let mut dense = DenseState::from_amplitudes(spec, amplitudes.clone()).unwrap();
        let mut gate_a =
            FlyQdnState::from_amplitudes(codec(), 64, spec, amplitudes.clone()).unwrap();
        let mut virtualized =
            VirtualFlyQdnState::from_amplitudes(codec(), spec, amplitudes, config()).unwrap();

        dense.apply_operations(&operations).unwrap();
        gate_a.apply_operations(&operations).unwrap();
        let mut executor = VirtualExecutor::new(config()).unwrap();
        executor
            .apply_operations(&mut virtualized, &operations)
            .unwrap();

        assert!(virtualized.compare_gate_a(&gate_a).unwrap().exact_bits);
        let reconstructed = virtualized.reconstruct().unwrap();
        assert_bits_equal(&reconstructed, dense.amplitudes());
        assert_eq!(
            virtualized.observation_snapshot().state_digest,
            gate_a.observation_snapshot().state_digest
        );
        assert_eq!(
            virtualized.observation_snapshot().norm_squared.to_bits(),
            gate_a.observation_snapshot().norm_squared.to_bits()
        );
    }

    #[test]
    fn signature_cache_reuses_only_committed_exact_generation() {
        let spec = SystemSpec::new(2, 4).unwrap();
        let initial = VirtualFlyQdnState::zero(codec(), spec, config()).unwrap();
        let operation = Operation::WeylX {
            target: 1,
            shift: 1,
        };
        let mut first = initial.clone();
        let mut second = initial;
        let mut executor = VirtualExecutor::new(config()).unwrap();

        executor.apply_operation(&mut first, &operation).unwrap();
        assert_eq!(executor.metrics().cache_misses, 1);
        executor.apply_operation(&mut second, &operation).unwrap();

        assert_eq!(executor.metrics().cache_hits, 1);
        assert_eq!(first.state_digest(), second.state_digest());
        assert_bits_equal(
            &first.reconstruct().unwrap(),
            &second.reconstruct().unwrap(),
        );
    }

    #[test]
    fn named_identity_invariant_avoids_work() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let mut state = VirtualFlyQdnState::zero(codec(), spec, config()).unwrap();
        let before = state.state_digest().to_owned();
        let mut executor = VirtualExecutor::new(config()).unwrap();

        executor
            .apply_operation(
                &mut state,
                &Operation::WeylX {
                    target: 0,
                    shift: 3,
                },
            )
            .unwrap();

        assert_eq!(state.state_digest(), before);
        assert_eq!(executor.metrics().invariant_reuses, 1);
        assert_eq!(executor.metrics().addresses_scanned, 0);
    }

    #[test]
    fn failed_sequence_does_not_mutate_state() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let mut state = VirtualFlyQdnState::zero(codec(), spec, config()).unwrap();
        let before = state.state_digest().to_owned();
        let operations = vec![
            Operation::WeylX {
                target: 0,
                shift: 1,
            },
            Operation::LocalPermutation {
                target: 0,
                map: vec![1, 2, 0],
            },
        ];
        let mut executor = VirtualExecutor::new(config()).unwrap();

        assert!(matches!(
            executor.apply_operations(&mut state, &operations),
            Err(VirtualizationError::UnsupportedOperation { .. })
        ));
        assert_eq!(state.state_digest(), before);
        assert_eq!(executor.metrics().cache_hits, 0);
        assert_eq!(executor.metrics().cache_misses, 0);
    }

    #[test]
    fn shared_materialization_reuses_one_immutable_tile() {
        let spec = SystemSpec::new(2, 5).unwrap();
        let state = Arc::new(VirtualFlyQdnState::zero(codec(), spec, config()).unwrap());
        let materializer = SharedTileMaterializer::new(state);
        let owner = materializer.owner_for_tile(0).unwrap();
        let request = TileRequest::new(owner);

        let first = materializer.materialize(0, &request).unwrap();
        let second = materializer.materialize(0, &request).unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(materializer.generation_count(), 1);
        assert_eq!(first.amplitudes().len(), 8);
    }

    #[test]
    fn shared_materialization_enforces_owner_cancellation_and_deadline() {
        let spec = SystemSpec::new(2, 3).unwrap();
        let state = Arc::new(VirtualFlyQdnState::zero(codec(), spec, config()).unwrap());
        let materializer = SharedTileMaterializer::new(state);
        let owner = materializer.owner_for_tile(0).unwrap();

        assert!(matches!(
            materializer.materialize(0, &TileRequest::new((owner + 1) % 2)),
            Err(SharedMaterializationError::OwnerMismatch { .. })
        ));

        let cancellation = Arc::new(AtomicBool::new(true));
        assert_eq!(
            materializer
                .materialize(0, &TileRequest::new(owner).with_cancellation(cancellation))
                .unwrap_err(),
            SharedMaterializationError::Cancelled
        );

        assert_eq!(
            materializer
                .materialize(0, &TileRequest::new(owner).with_deadline(Instant::now()))
                .unwrap_err(),
            SharedMaterializationError::DeadlineExceeded
        );
    }

    #[test]
    fn virtual_storage_snapshot_keeps_logical_and_materialized_scale_separate() {
        let spec = SystemSpec::new(2, 5).unwrap();
        let state = VirtualFlyQdnState::zero(codec(), spec, config()).unwrap();
        let snapshot = state.storage_snapshot();
        assert_eq!(snapshot.facts().materialized_address_count, 1);
        assert_eq!(snapshot.facts().materialized_payload_bytes, 16);
        assert_eq!(
            snapshot.facts().logical_address_count,
            codec().geometry().logical_address_count()
        );
        assert_eq!(snapshot.facts().exactness, StorageExactness::Exact);
    }

    #[test]
    fn descriptor_remains_exact_oracle_compared_not_benchmarked() {
        let descriptor = virtualized_module_descriptor();
        descriptor.validate().unwrap();
        assert_eq!(descriptor.maturity, Maturity::E3OracleCompared);
        assert!(descriptor
            .capabilities
            .contains(&Capability::OperationExecution));
    }
}

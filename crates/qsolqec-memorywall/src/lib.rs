//! QSOLQEC R5 memory-wall benchmark runtime.
//!
//! The harness separates deterministic experiment identity from host identity
//! and separates logical representation bytes from process RSS. A measured
//! point is intended to run in a fresh process so Linux VmHWM is scoped to that
//! experiment rather than contaminated by earlier representations.

use std::fmt;
use std::fs;
use std::process::Command;
use std::time::Instant;

use num_complex::Complex64;
use qsolqec_core::SystemSpec;
use qsolqec_dense::{DenseState, DenseStateError};
use qsolqec_fly_phi664::{MacroSourceSpec, Phi664Codec};
use qsolqec_fly_qdn::virtualized::{
    VirtualExecutor, VirtualFlyQdnState, VirtualizationConfig, VirtualizationError,
    VIRTUALIZED_REPRESENTATION_ID,
};
use qsolqec_fly_qdn::FlyQdnError;
use qsolqec_glassbox::{sha256_hex, ObservableState, SemanticHasher};
use qsolqec_ops::{Operation, OperationSupport};
use qsolqec_stabilizer::{PrimeStabilizerState, StabilizerError};
use serde::{Deserialize, Serialize};

pub const RECEIPT_SCHEMA: &str = "qsolqec.memorywall.receipt.v2";
pub const SWEEP_SCHEMA: &str = "qsolqec.memorywall.sweep.v2";
pub const HOST_SCHEMA: &str = "qsolqec.memorywall.host.v1";
pub const WORKLOAD_SCHEMA: &str = "qsolqec.memorywall.clifford-ring.v1";
pub const SOURCE_REPOSITORY: &str = "https://github.com/QSOLKCB/QSOLQEC";
pub const DEFAULT_ORACLE_LOGICAL_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
const BUILD_SOURCE_SHA: &str = env!("QSOLQEC_BUILD_SOURCE_SHA");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepresentationKind {
    Dense,
    PrimeStabilizer,
    FlyPhi664Virtualized,
}

impl RepresentationKind {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Dense => "dense-reference",
            Self::PrimeStabilizer => "prime-stabilizer",
            Self::FlyPhi664Virtualized => VIRTUALIZED_REPRESENTATION_ID,
        }
    }
}

impl fmt::Display for RepresentationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dense => "dense",
            Self::PrimeStabilizer => "stabilizer",
            Self::FlyPhi664Virtualized => "fly-phi664",
        })
    }
}

pub const BUILTIN_FLY_BODY_IDS: [u64; 2] = [12781, 556329];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlyBodyIdSource {
    BuiltinR7Fixture,
    ExternalCanonicalList,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlyExperimentConfig {
    pub body_ids: Vec<u64>,
    pub body_id_source: FlyBodyIdSource,
    pub page_span: usize,
    pub tile_span: usize,
    pub sparse_max_occupancy: usize,
    pub bitmap_max_occupancy: usize,
    pub scratch_domains: usize,
    pub owner_count: u32,
    pub max_cached_states: usize,
    pub max_in_flight_generations: usize,
}

impl Default for FlyExperimentConfig {
    fn default() -> Self {
        Self {
            body_ids: BUILTIN_FLY_BODY_IDS.to_vec(),
            body_id_source: FlyBodyIdSource::BuiltinR7Fixture,
            page_span: 256,
            tile_span: 256,
            sparse_max_occupancy: 16,
            bitmap_max_occupancy: 128,
            scratch_domains: 4,
            owner_count: 4,
            max_cached_states: 32,
            max_in_flight_generations: 4,
        }
    }
}

impl FlyExperimentConfig {
    fn virtualization_config(&self) -> Result<VirtualizationConfig, HarnessError> {
        VirtualizationConfig::new(
            self.page_span,
            self.tile_span,
            self.sparse_max_occupancy,
            self.bitmap_max_occupancy,
            self.scratch_domains,
            self.owner_count,
            self.max_cached_states,
            self.max_in_flight_generations,
        )
        .map_err(|error| HarnessError::InvalidSpec(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentSpec {
    pub representation: RepresentationKind,
    pub dimension: usize,
    pub subsystems: usize,
    pub rounds: usize,
    pub max_logical_bytes: Option<u64>,
    pub oracle_logical_limit_bytes: u64,
    pub fly: FlyExperimentConfig,
}

impl ExperimentSpec {
    pub fn new(
        representation: RepresentationKind,
        dimension: usize,
        subsystems: usize,
        rounds: usize,
    ) -> Self {
        Self {
            representation,
            dimension,
            subsystems,
            rounds,
            max_logical_bytes: None,
            oracle_logical_limit_bytes: DEFAULT_ORACLE_LOGICAL_LIMIT_BYTES,
            fly: FlyExperimentConfig::default(),
        }
    }

    pub fn system(&self) -> Result<SystemSpec, HarnessError> {
        if self.rounds == 0 {
            return Err(HarnessError::InvalidSpec(
                "rounds must be at least 1".into(),
            ));
        }
        SystemSpec::new(self.dimension, self.subsystems)
            .map_err(|error| HarnessError::InvalidSpec(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub memory_total_bytes: Option<u64>,
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    pub schema: String,
    pub os: String,
    pub arch: String,
    pub hostname: Option<String>,
    pub cpu_model: Option<String>,
    pub logical_cpu_count: Option<usize>,
    pub total_memory_bytes: Option<u64>,
    pub gpus: Vec<GpuInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadIdentity {
    pub schema: String,
    pub id: String,
    pub operation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimingMeasurements {
    pub construction_ns: Option<u64>,
    pub execution_ns: Option<u64>,
    pub snapshot_ns: Option<u64>,
    pub oracle_verification_ns: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryMeasurements {
    pub estimated_logical_bytes: Option<u64>,
    pub logical_bytes: Option<u64>,
    pub materialized_payload_bytes: Option<u64>,
    pub resident_working_set_bytes: Option<u64>,
    pub rss_before_bytes: Option<u64>,
    pub rss_after_bytes: Option<u64>,
    pub peak_process_rss_before_bytes: Option<u64>,
    pub peak_process_rss_bytes: Option<u64>,
    pub incremental_peak_rss_bytes: Option<u64>,
    pub allocation_count: Option<u64>,
    pub materialization_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredCandidateMeasurements {
    pub body_id_source: FlyBodyIdSource,
    pub macro_node_count: u64,
    pub body_ids_digest: String,
    pub source_identity_digest: String,
    pub geometry_digest: String,
    pub logical_namespace_addresses: u64,
    pub materialized_address_count: u64,
    pub materialized_page_count: u64,
    pub sparse_page_count: u64,
    pub bitmap_page_count: u64,
    pub dense_page_count: u64,
    pub tracked_state_resident_bytes: u64,
    pub worker_scratch_capacity_bytes: u64,
    pub peak_tracked_active_bytes: u64,
    pub scratch_domains: usize,
    pub owner_count: u32,
    pub cached_state_count: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub invariant_reuses: u64,
    pub reused_generations: u64,
    pub recomputed_generations: u64,
    pub worker_dispatches: u64,
    pub addresses_scanned: u64,
    pub addresses_soundly_skipped: u64,
    pub fourier_lanes_executed: u64,
    pub fourier_lanes_pruned: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationSupportClass {
    Exact,
    Approximate,
    Unsupported,
}

impl From<OperationSupport> for OperationSupportClass {
    fn from(value: OperationSupport) -> Self {
        match value {
            OperationSupport::Exact => Self::Exact,
            OperationSupport::Approximate => Self::Approximate,
            OperationSupport::Unsupported => Self::Unsupported,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum OracleAgreement {
    SelfReference,
    Matched { tolerance: f64, max_error: f64 },
    Mismatch { tolerance: f64, max_error: f64 },
    Unavailable { reason: String },
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum RunOutcome {
    Success,
    Unsupported {
        reason: String,
    },
    SizeOverflow {
        reason: String,
    },
    LogicalBudgetExceeded {
        required_bytes: u64,
        limit_bytes: u64,
    },
    LogicalNamespaceInsufficient {
        required_addresses: u64,
        available_addresses: u64,
    },
    AllocationFailed {
        reason: String,
    },
    ExecutionFailed {
        reason: String,
    },
}

impl RunOutcome {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReceiptBody {
    pub experiment_id: String,
    pub source_revision: String,
    pub source_revision_url: String,
    pub representation: RepresentationKind,
    pub representation_id: String,
    pub compute_backend: String,
    pub worker_count: usize,
    pub dimension: usize,
    pub subsystems: usize,
    pub rounds: usize,
    pub max_logical_bytes: Option<u64>,
    pub oracle_logical_limit_bytes: u64,
    pub workload: WorkloadIdentity,
    pub operation_support: OperationSupportClass,
    pub host: HostInfo,
    pub memory: MemoryMeasurements,
    pub timings: TimingMeasurements,
    pub final_state_digest: Option<String>,
    pub norm_squared: Option<f64>,
    pub oracle_agreement: OracleAgreement,
    pub structured_candidate: Option<StructuredCandidateMeasurements>,
    pub outcome: RunOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryWallReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub body: ReceiptBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepChildFailure {
    pub representation: RepresentationKind,
    pub dimension: usize,
    pub subsystems: usize,
    pub rounds: usize,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepReceipt {
    pub schema: String,
    pub source_revision: String,
    pub source_revision_url: String,
    pub dimension: usize,
    pub start_n: usize,
    pub end_n: usize,
    pub step: usize,
    pub rounds: usize,
    pub representations: Vec<RepresentationKind>,
    pub points: Vec<MemoryWallReceipt>,
    pub child_failures: Vec<SweepChildFailure>,
}

#[derive(Debug)]
pub enum HarnessError {
    InvalidSpec(String),
    Workload(String),
    Serialization(String),
    Io(String),
    Child(String),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec(message) => write!(f, "invalid experiment spec: {message}"),
            Self::Workload(message) => write!(f, "workload error: {message}"),
            Self::Serialization(message) => write!(f, "serialization error: {message}"),
            Self::Io(message) => write!(f, "I/O error: {message}"),
            Self::Child(message) => write!(f, "child-process error: {message}"),
        }
    }
}

impl std::error::Error for HarnessError {}

pub fn probe_host() -> HostInfo {
    HostInfo {
        schema: HOST_SCHEMA.into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        hostname: std::env::var("HOSTNAME")
            .ok()
            .filter(|value| !value.is_empty()),
        cpu_model: linux_cpu_model(),
        logical_cpu_count: std::thread::available_parallelism()
            .ok()
            .map(std::num::NonZeroUsize::get),
        total_memory_bytes: linux_kib_field("/proc/meminfo", "MemTotal:")
            .and_then(|kib| kib.checked_mul(1024)),
        gpus: probe_nvidia_gpus(),
    }
}

pub fn source_revision() -> String {
    BUILD_SOURCE_SHA.to_owned()
}

pub fn source_revision_url() -> String {
    format!("{SOURCE_REPOSITORY}/commit/{BUILD_SOURCE_SHA}")
}

pub fn run_experiment(spec: &ExperimentSpec) -> Result<MemoryWallReceipt, HarnessError> {
    let system = spec.system()?;
    let operations = workload_operations(system, spec.rounds)?;
    let workload = workload_identity(system, spec.rounds, &operations);
    let operation_support = workload_support_class(spec.representation, system, &operations)?;
    let experiment_id = experiment_id(spec, &workload);
    let host = probe_host();
    let revision = source_revision();
    let revision_url = source_revision_url();

    let estimated_logical_bytes = estimate_logical_bytes(spec.representation, system);
    let rss_before_bytes = linux_current_rss_bytes();
    let peak_before_bytes = linux_peak_rss_bytes();

    if operation_support == OperationSupportClass::Unsupported {
        return finalize_receipt(ReceiptBody {
            experiment_id,
            source_revision: revision,
            source_revision_url: revision_url,
            representation: spec.representation,
            representation_id: spec.representation.id().into(),
            compute_backend: compute_backend(spec.representation).into(),
            worker_count: 1,
            dimension: spec.dimension,
            subsystems: spec.subsystems,
            rounds: spec.rounds,
            max_logical_bytes: spec.max_logical_bytes,
            oracle_logical_limit_bytes: spec.oracle_logical_limit_bytes,
            workload,
            operation_support,
            host,
            memory: MemoryMeasurements {
                estimated_logical_bytes,
                logical_bytes: None,
                materialized_payload_bytes: None,
                resident_working_set_bytes: None,
                rss_before_bytes,
                rss_after_bytes: linux_current_rss_bytes(),
                peak_process_rss_before_bytes: peak_before_bytes,
                peak_process_rss_bytes: linux_peak_rss_bytes(),
                incremental_peak_rss_bytes: None,
                allocation_count: None,
                materialization_count: Some(0),
            },
            timings: empty_timings(),
            final_state_digest: None,
            norm_squared: None,
            oracle_agreement: OracleAgreement::NotApplicable,
            structured_candidate: None,
            outcome: RunOutcome::Unsupported {
                reason: format!(
                    "{} does not support the declared workload for Q({},{})",
                    spec.representation.id(),
                    spec.dimension,
                    spec.subsystems
                ),
            },
        });
    }

    if let (Some(required), Some(limit)) = (estimated_logical_bytes, spec.max_logical_bytes) {
        if required > limit {
            return finalize_receipt(ReceiptBody {
                experiment_id,
                source_revision: revision,
                source_revision_url: revision_url,
                representation: spec.representation,
                representation_id: spec.representation.id().into(),
                compute_backend: compute_backend(spec.representation).into(),
                worker_count: 1,
                dimension: spec.dimension,
                subsystems: spec.subsystems,
                rounds: spec.rounds,
                max_logical_bytes: spec.max_logical_bytes,
                oracle_logical_limit_bytes: spec.oracle_logical_limit_bytes,
                workload,
                operation_support,
                host,
                memory: MemoryMeasurements {
                    estimated_logical_bytes: Some(required),
                    logical_bytes: None,
                    materialized_payload_bytes: None,
                    resident_working_set_bytes: None,
                    rss_before_bytes,
                    rss_after_bytes: linux_current_rss_bytes(),
                    peak_process_rss_before_bytes: peak_before_bytes,
                    peak_process_rss_bytes: linux_peak_rss_bytes(),
                    incremental_peak_rss_bytes: None,
                    allocation_count: None,
                    materialization_count: Some(0),
                },
                timings: empty_timings(),
                final_state_digest: None,
                norm_squared: None,
                oracle_agreement: OracleAgreement::NotApplicable,
                structured_candidate: None,
                outcome: RunOutcome::LogicalBudgetExceeded {
                    required_bytes: required,
                    limit_bytes: limit,
                },
            });
        }
    }

    match spec.representation {
        RepresentationKind::Dense => run_dense(
            spec,
            system,
            operations,
            workload,
            operation_support,
            experiment_id,
            host,
            revision,
            revision_url,
            estimated_logical_bytes,
            rss_before_bytes,
            peak_before_bytes,
        ),
        RepresentationKind::PrimeStabilizer => run_stabilizer(
            spec,
            system,
            operations,
            workload,
            operation_support,
            experiment_id,
            host,
            revision,
            revision_url,
            estimated_logical_bytes,
            rss_before_bytes,
            peak_before_bytes,
        ),
        RepresentationKind::FlyPhi664Virtualized => run_fly_virtualized(
            spec,
            system,
            operations,
            workload,
            operation_support,
            experiment_id,
            host,
            revision,
            revision_url,
            estimated_logical_bytes,
            rss_before_bytes,
            peak_before_bytes,
        ),
    }
}

fn workload_support_class(
    representation: RepresentationKind,
    system: SystemSpec,
    operations: &[Operation],
) -> Result<OperationSupportClass, HarnessError> {
    let mut aggregate = OperationSupportClass::Exact;

    for operation in operations {
        let support = match representation {
            RepresentationKind::Dense => DenseState::support_for_spec(system, operation)
                .map_err(|error| HarnessError::Workload(error.to_string()))?,
            RepresentationKind::PrimeStabilizer => {
                match PrimeStabilizerState::support_for_spec(system, operation) {
                    Ok(support) => support,
                    Err(StabilizerError::NonPrimeDimension { .. }) => {
                        return Ok(OperationSupportClass::Unsupported);
                    }
                    Err(error) => return Err(HarnessError::Workload(error.to_string())),
                }
            }
            RepresentationKind::FlyPhi664Virtualized => {
                VirtualFlyQdnState::support_for_spec(system, operation)
                    .map_err(|error| HarnessError::Workload(error.to_string()))?
            }
        };

        match OperationSupportClass::from(support) {
            OperationSupportClass::Unsupported => {
                return Ok(OperationSupportClass::Unsupported);
            }
            OperationSupportClass::Approximate => {
                aggregate = OperationSupportClass::Approximate;
            }
            OperationSupportClass::Exact => {}
        }
    }

    Ok(aggregate)
}

#[allow(clippy::too_many_arguments)]
fn run_dense(
    spec: &ExperimentSpec,
    system: SystemSpec,
    operations: Vec<Operation>,
    workload: WorkloadIdentity,
    operation_support: OperationSupportClass,
    experiment_id: String,
    host: HostInfo,
    revision: String,
    revision_url: String,
    estimated_logical_bytes: Option<u64>,
    rss_before_bytes: Option<u64>,
    peak_before_bytes: Option<u64>,
) -> Result<MemoryWallReceipt, HarnessError> {
    let construction_start = Instant::now();
    let mut state = match DenseState::zero(system) {
        Ok(state) => state,
        Err(error) => {
            let outcome = match error {
                DenseStateError::StateSizeOverflow => RunOutcome::SizeOverflow {
                    reason: error.to_string(),
                },
                DenseStateError::AllocationFailed { .. } => RunOutcome::AllocationFailed {
                    reason: error.to_string(),
                },
                _ => RunOutcome::ExecutionFailed {
                    reason: error.to_string(),
                },
            };
            return failed_receipt(
                spec,
                workload,
                operation_support,
                experiment_id,
                host,
                revision,
                revision_url,
                estimated_logical_bytes,
                rss_before_bytes,
                peak_before_bytes,
                FailureEvidence::preconstruction(duration_ns(construction_start.elapsed())),
                outcome,
            );
        }
    };
    let construction_ns = duration_ns(construction_start.elapsed());
    let materialized_logical_bytes = dense_logical_bytes(&state);

    let execution_start = Instant::now();
    if let Err(error) = state.apply_operations(&operations) {
        let execution_ns = duration_ns(execution_start.elapsed());
        return failed_receipt(
            spec,
            workload,
            operation_support,
            experiment_id,
            host,
            revision,
            revision_url,
            estimated_logical_bytes,
            rss_before_bytes,
            peak_before_bytes,
            FailureEvidence::materialized(
                construction_ns,
                Some(execution_ns),
                materialized_logical_bytes,
            ),
            RunOutcome::ExecutionFailed {
                reason: error.to_string(),
            },
        );
    }
    let execution_ns = duration_ns(execution_start.elapsed());

    let snapshot_start = Instant::now();
    let snapshot = state.observation_snapshot();
    let snapshot_ns = duration_ns(snapshot_start.elapsed());

    let peak_after = linux_peak_rss_bytes();
    let rss_after = linux_current_rss_bytes();

    finalize_receipt(ReceiptBody {
        experiment_id,
        source_revision: revision,
        source_revision_url: revision_url,
        representation: spec.representation,
        representation_id: spec.representation.id().into(),
        compute_backend: compute_backend(spec.representation).into(),
        worker_count: 1,
        dimension: spec.dimension,
        subsystems: spec.subsystems,
        rounds: spec.rounds,
        max_logical_bytes: spec.max_logical_bytes,
        oracle_logical_limit_bytes: spec.oracle_logical_limit_bytes,
        workload,
        operation_support,
        host,
        memory: MemoryMeasurements {
            estimated_logical_bytes,
            logical_bytes: u64::try_from(snapshot.logical_bytes).ok(),
            materialized_payload_bytes: u64::try_from(snapshot.logical_bytes).ok(),
            resident_working_set_bytes: None,
            rss_before_bytes,
            rss_after_bytes: rss_after,
            peak_process_rss_before_bytes: peak_before_bytes,
            peak_process_rss_bytes: peak_after,
            incremental_peak_rss_bytes: peak_delta(peak_before_bytes, peak_after),
            allocation_count: None,
            materialization_count: Some(1),
        },
        timings: TimingMeasurements {
            construction_ns: Some(construction_ns),
            execution_ns: Some(execution_ns),
            snapshot_ns: Some(snapshot_ns),
            oracle_verification_ns: None,
        },
        final_state_digest: Some(snapshot.state_digest),
        norm_squared: Some(snapshot.norm_squared),
        oracle_agreement: OracleAgreement::SelfReference,
        structured_candidate: None,
        outcome: RunOutcome::Success,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_stabilizer(
    spec: &ExperimentSpec,
    system: SystemSpec,
    operations: Vec<Operation>,
    workload: WorkloadIdentity,
    operation_support: OperationSupportClass,
    experiment_id: String,
    host: HostInfo,
    revision: String,
    revision_url: String,
    estimated_logical_bytes: Option<u64>,
    rss_before_bytes: Option<u64>,
    peak_before_bytes: Option<u64>,
) -> Result<MemoryWallReceipt, HarnessError> {
    let construction_start = Instant::now();
    let mut state = match PrimeStabilizerState::zero(system) {
        Ok(state) => state,
        Err(error) => {
            return failed_receipt(
                spec,
                workload,
                operation_support,
                experiment_id,
                host,
                revision,
                revision_url,
                estimated_logical_bytes,
                rss_before_bytes,
                peak_before_bytes,
                FailureEvidence::preconstruction(duration_ns(construction_start.elapsed())),
                classify_stabilizer_error(error),
            );
        }
    };
    let construction_ns = duration_ns(construction_start.elapsed());
    let materialized_logical_bytes = u64::try_from(state.logical_bytes()).ok();

    let execution_start = Instant::now();
    if let Err(error) = state.apply_operations(&operations) {
        let execution_ns = duration_ns(execution_start.elapsed());
        return failed_receipt(
            spec,
            workload,
            operation_support,
            experiment_id,
            host,
            revision,
            revision_url,
            estimated_logical_bytes,
            rss_before_bytes,
            peak_before_bytes,
            FailureEvidence::materialized(
                construction_ns,
                Some(execution_ns),
                materialized_logical_bytes,
            ),
            classify_stabilizer_error(error),
        );
    }
    let execution_ns = duration_ns(execution_start.elapsed());

    let snapshot_start = Instant::now();
    let snapshot = state.observation_snapshot();
    let snapshot_ns = duration_ns(snapshot_start.elapsed());

    // Freeze candidate memory measurements before running the dense oracle.
    let peak_after = linux_peak_rss_bytes();
    let rss_after = linux_current_rss_bytes();

    let oracle_start = Instant::now();
    let oracle_agreement = match estimate_dense_bytes(system) {
        None => OracleAgreement::Unavailable {
            reason: "dense oracle size overflows the platform address space".into(),
        },
        Some(bytes) if bytes > spec.oracle_logical_limit_bytes => OracleAgreement::Unavailable {
            reason: format!(
                "dense oracle logical bytes {bytes} exceed oracle limit {}",
                spec.oracle_logical_limit_bytes
            ),
        },
        Some(_) => match DenseState::zero(system) {
            Err(error) => OracleAgreement::Unavailable {
                reason: format!("dense oracle construction failed: {error}"),
            },
            Ok(mut dense) => match dense.apply_operations(&operations) {
                Err(error) => OracleAgreement::Unavailable {
                    reason: format!("dense oracle execution failed: {error}"),
                },
                Ok(()) => {
                    let tolerance = 1.0e-10;
                    match stabilizer_max_error(&state, &dense) {
                        Ok(max_error) if max_error <= tolerance => OracleAgreement::Matched {
                            tolerance,
                            max_error,
                        },
                        Ok(max_error) => OracleAgreement::Mismatch {
                            tolerance,
                            max_error,
                        },
                        Err(error) => OracleAgreement::Unavailable {
                            reason: error.to_string(),
                        },
                    }
                }
            },
        },
    };
    let oracle_verification_ns = duration_ns(oracle_start.elapsed());

    finalize_receipt(ReceiptBody {
        experiment_id,
        source_revision: revision,
        source_revision_url: revision_url,
        representation: spec.representation,
        representation_id: spec.representation.id().into(),
        compute_backend: compute_backend(spec.representation).into(),
        worker_count: 1,
        dimension: spec.dimension,
        subsystems: spec.subsystems,
        rounds: spec.rounds,
        max_logical_bytes: spec.max_logical_bytes,
        oracle_logical_limit_bytes: spec.oracle_logical_limit_bytes,
        workload,
        operation_support,
        host,
        memory: MemoryMeasurements {
            estimated_logical_bytes,
            logical_bytes: u64::try_from(snapshot.logical_bytes).ok(),
            materialized_payload_bytes: u64::try_from(snapshot.logical_bytes).ok(),
            resident_working_set_bytes: None,
            rss_before_bytes,
            rss_after_bytes: rss_after,
            peak_process_rss_before_bytes: peak_before_bytes,
            peak_process_rss_bytes: peak_after,
            incremental_peak_rss_bytes: peak_delta(peak_before_bytes, peak_after),
            allocation_count: None,
            materialization_count: Some(1),
        },
        timings: TimingMeasurements {
            construction_ns: Some(construction_ns),
            execution_ns: Some(execution_ns),
            snapshot_ns: Some(snapshot_ns),
            oracle_verification_ns: Some(oracle_verification_ns),
        },
        final_state_digest: Some(snapshot.state_digest),
        norm_squared: Some(snapshot.norm_squared),
        oracle_agreement,
        structured_candidate: None,
        outcome: RunOutcome::Success,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_fly_virtualized(
    spec: &ExperimentSpec,
    system: SystemSpec,
    operations: Vec<Operation>,
    workload: WorkloadIdentity,
    operation_support: OperationSupportClass,
    experiment_id: String,
    host: HostInfo,
    revision: String,
    revision_url: String,
    estimated_logical_bytes: Option<u64>,
    rss_before_bytes: Option<u64>,
    peak_before_bytes: Option<u64>,
) -> Result<MemoryWallReceipt, HarnessError> {
    let virtualization = spec.fly.virtualization_config()?;

    let construction_start = Instant::now();
    let codec =
        match Phi664Codec::from_body_ids(MacroSourceSpec::male_cns_v1(), spec.fly.body_ids.clone())
        {
            Ok(codec) => codec,
            Err(error) => {
                return failed_receipt(
                    spec,
                    workload,
                    operation_support,
                    experiment_id,
                    host,
                    revision,
                    revision_url,
                    estimated_logical_bytes,
                    rss_before_bytes,
                    peak_before_bytes,
                    FailureEvidence::preconstruction(duration_ns(construction_start.elapsed())),
                    RunOutcome::ExecutionFailed {
                        reason: format!("Fly-Phi664 macrograph construction failed: {error}"),
                    },
                );
            }
        };

    let macro_node_count = u64::try_from(codec.manifest().macro_node_count()).map_err(|_| {
        HarnessError::InvalidSpec("macro-node count exceeds u64 receipt range".into())
    })?;
    let body_ids_digest = codec.manifest().body_ids_digest().to_owned();
    let source_identity_digest = codec.manifest().source_identity().digest().to_owned();
    let geometry_digest = codec.geometry().digest().to_owned();
    let logical_namespace_addresses = u64::try_from(codec.geometry().logical_address_count())
        .map_err(|_| {
            HarnessError::InvalidSpec("logical namespace exceeds u64 receipt range".into())
        })?;

    let mut state = match VirtualFlyQdnState::zero(codec, system, virtualization) {
        Ok(state) => state,
        Err(error) => {
            return failed_receipt(
                spec,
                workload,
                operation_support,
                experiment_id,
                host,
                revision,
                revision_url,
                estimated_logical_bytes,
                rss_before_bytes,
                peak_before_bytes,
                FailureEvidence::preconstruction(duration_ns(construction_start.elapsed())),
                classify_virtualization_error(error),
            );
        }
    };

    let mut executor = match VirtualExecutor::new(virtualization) {
        Ok(executor) => executor,
        Err(error) => {
            return failed_receipt(
                spec,
                workload,
                operation_support,
                experiment_id,
                host,
                revision,
                revision_url,
                estimated_logical_bytes,
                rss_before_bytes,
                peak_before_bytes,
                FailureEvidence::materialized(
                    duration_ns(construction_start.elapsed()),
                    None,
                    u64::try_from(state.logical_state_bytes()).ok(),
                ),
                classify_virtualization_error(error),
            );
        }
    };
    let construction_ns = duration_ns(construction_start.elapsed());

    let execution_start = Instant::now();
    if let Err(error) = executor.apply_operations(&mut state, &operations) {
        let execution_ns = duration_ns(execution_start.elapsed());
        return failed_receipt(
            spec,
            workload,
            operation_support,
            experiment_id,
            host,
            revision,
            revision_url,
            estimated_logical_bytes,
            rss_before_bytes,
            peak_before_bytes,
            FailureEvidence::materialized(
                construction_ns,
                Some(execution_ns),
                u64::try_from(state.logical_state_bytes()).ok(),
            ),
            classify_virtualization_error(error),
        );
    }
    let execution_ns = duration_ns(execution_start.elapsed());

    let snapshot_start = Instant::now();
    let snapshot = state.observation_snapshot();
    let storage = state.storage_snapshot();
    let page_counts = state.page_kind_counts();
    let metrics = executor.metrics();
    let snapshot_ns = duration_ns(snapshot_start.elapsed());

    // Freeze candidate memory evidence before dense-oracle construction.
    let peak_after = linux_peak_rss_bytes();
    let rss_after = linux_current_rss_bytes();

    let materialized_address_count = u64::try_from(storage.facts().materialized_address_count)
        .map_err(|_| {
            HarnessError::Serialization("materialized address count exceeds u64".into())
        })?;
    let materialized_payload_bytes = u64::try_from(storage.facts().materialized_payload_bytes)
        .map_err(|_| HarnessError::Serialization("materialized payload bytes exceed u64".into()))?;
    let materialized_page_count = u64::try_from(state.materialized_page_count())
        .map_err(|_| HarnessError::Serialization("materialized page count exceeds u64".into()))?;
    let tracked_state_resident_bytes = u64::try_from(state.tracked_resident_bytes())
        .map_err(|_| HarnessError::Serialization("tracked state bytes exceed u64".into()))?;
    let worker_scratch_capacity_bytes = u64::try_from(executor.worker_scratch_capacity_bytes())
        .map_err(|_| HarnessError::Serialization("worker scratch bytes exceed u64".into()))?;
    let peak_tracked_active_bytes = u64::try_from(metrics.peak_tracked_active_bytes)
        .map_err(|_| HarnessError::Serialization("tracked active bytes exceed u64".into()))?;

    let candidate_measurements = StructuredCandidateMeasurements {
        body_id_source: spec.fly.body_id_source.clone(),
        macro_node_count,
        body_ids_digest,
        source_identity_digest,
        geometry_digest,
        logical_namespace_addresses,
        materialized_address_count,
        materialized_page_count,
        sparse_page_count: u64::try_from(page_counts.sparse).unwrap_or(u64::MAX),
        bitmap_page_count: u64::try_from(page_counts.bitmap).unwrap_or(u64::MAX),
        dense_page_count: u64::try_from(page_counts.dense).unwrap_or(u64::MAX),
        tracked_state_resident_bytes,
        worker_scratch_capacity_bytes,
        peak_tracked_active_bytes,
        scratch_domains: spec.fly.scratch_domains,
        owner_count: spec.fly.owner_count,
        cached_state_count: u64::try_from(executor.cached_state_count()).unwrap_or(u64::MAX),
        cache_hits: metrics.cache_hits,
        cache_misses: metrics.cache_misses,
        invariant_reuses: metrics.invariant_reuses,
        reused_generations: metrics.cache_hits.saturating_add(metrics.invariant_reuses),
        recomputed_generations: metrics.cache_misses,
        worker_dispatches: metrics.worker_dispatches,
        addresses_scanned: u64::try_from(metrics.addresses_scanned).unwrap_or(u64::MAX),
        addresses_soundly_skipped: u64::try_from(metrics.addresses_soundly_skipped)
            .unwrap_or(u64::MAX),
        fourier_lanes_executed: u64::try_from(metrics.fourier_lanes_executed).unwrap_or(u64::MAX),
        fourier_lanes_pruned: u64::try_from(metrics.fourier_lanes_pruned).unwrap_or(u64::MAX),
    };

    let oracle_start = Instant::now();
    let oracle_agreement = match estimate_dense_bytes(system) {
        None => OracleAgreement::Unavailable {
            reason: "dense oracle size overflows the platform address space".into(),
        },
        Some(bytes) if bytes > spec.oracle_logical_limit_bytes => OracleAgreement::Unavailable {
            reason: format!(
                "dense oracle logical bytes {bytes} exceed oracle limit {}",
                spec.oracle_logical_limit_bytes
            ),
        },
        Some(_) => match DenseState::zero(system) {
            Err(error) => OracleAgreement::Unavailable {
                reason: format!("dense oracle construction failed: {error}"),
            },
            Ok(mut dense) => match dense.apply_operations(&operations) {
                Err(error) => OracleAgreement::Unavailable {
                    reason: format!("dense oracle execution failed: {error}"),
                },
                Ok(()) => match fly_dense_max_error(&state, &dense) {
                    Ok(max_error) if max_error == 0.0 => OracleAgreement::Matched {
                        tolerance: 0.0,
                        max_error,
                    },
                    Ok(max_error) => OracleAgreement::Mismatch {
                        tolerance: 0.0,
                        max_error,
                    },
                    Err(error) => OracleAgreement::Unavailable {
                        reason: error.to_string(),
                    },
                },
            },
        },
    };
    let oracle_verification_ns = duration_ns(oracle_start.elapsed());

    finalize_receipt(ReceiptBody {
        experiment_id,
        source_revision: revision,
        source_revision_url: revision_url,
        representation: spec.representation,
        representation_id: spec.representation.id().into(),
        compute_backend: compute_backend(spec.representation).into(),
        worker_count: 1,
        dimension: spec.dimension,
        subsystems: spec.subsystems,
        rounds: spec.rounds,
        max_logical_bytes: spec.max_logical_bytes,
        oracle_logical_limit_bytes: spec.oracle_logical_limit_bytes,
        workload,
        operation_support,
        host,
        memory: MemoryMeasurements {
            estimated_logical_bytes,
            logical_bytes: u64::try_from(snapshot.logical_bytes).ok(),
            materialized_payload_bytes: Some(materialized_payload_bytes),
            resident_working_set_bytes: Some(peak_tracked_active_bytes),
            rss_before_bytes,
            rss_after_bytes: rss_after,
            peak_process_rss_before_bytes: peak_before_bytes,
            peak_process_rss_bytes: peak_after,
            incremental_peak_rss_bytes: peak_delta(peak_before_bytes, peak_after),
            allocation_count: None,
            materialization_count: Some(materialized_page_count),
        },
        timings: TimingMeasurements {
            construction_ns: Some(construction_ns),
            execution_ns: Some(execution_ns),
            snapshot_ns: Some(snapshot_ns),
            oracle_verification_ns: Some(oracle_verification_ns),
        },
        final_state_digest: Some(snapshot.state_digest),
        norm_squared: Some(snapshot.norm_squared),
        oracle_agreement,
        structured_candidate: Some(candidate_measurements),
        outcome: RunOutcome::Success,
    })
}

fn classify_virtualization_error(error: VirtualizationError) -> RunOutcome {
    match error {
        VirtualizationError::GateA(FlyQdnError::StateSizeOverflow) => RunOutcome::SizeOverflow {
            reason: error.to_string(),
        },
        VirtualizationError::GateA(FlyQdnError::AllocationFailed { .. })
        | VirtualizationError::AllocationFailed { .. } => RunOutcome::AllocationFailed {
            reason: error.to_string(),
        },
        VirtualizationError::GateA(FlyQdnError::InsufficientLogicalAddresses {
            required,
            available,
        }) => RunOutcome::LogicalNamespaceInsufficient {
            required_addresses: u64::try_from(required).unwrap_or(u64::MAX),
            available_addresses: u64::try_from(available).unwrap_or(u64::MAX),
        },
        VirtualizationError::GateA(FlyQdnError::UnsupportedOperation { .. })
        | VirtualizationError::UnsupportedOperation { .. } => RunOutcome::Unsupported {
            reason: error.to_string(),
        },
        _ => RunOutcome::ExecutionFailed {
            reason: error.to_string(),
        },
    }
}

fn fly_dense_max_error(
    candidate: &VirtualFlyQdnState,
    dense: &DenseState,
) -> Result<f64, HarnessError> {
    if candidate.spec() != dense.spec() {
        return Err(HarnessError::Workload(
            "oracle and Fly-Phi664 SystemSpec differ".into(),
        ));
    }
    let amplitudes = candidate
        .reconstruct()
        .map_err(|error| HarnessError::Workload(error.to_string()))?;
    let mut max_error = 0.0f64;
    for (candidate, reference) in amplitudes.iter().zip(dense.amplitudes()) {
        max_error = max_error.max((*candidate - *reference).norm());
    }
    Ok(max_error)
}

#[derive(Debug, Clone, Copy)]
struct FailureEvidence {
    logical_bytes: Option<u64>,
    materialized_payload_bytes: Option<u64>,
    materialization_count: u64,
    construction_ns: u64,
    execution_ns: Option<u64>,
}

impl FailureEvidence {
    const fn preconstruction(construction_ns: u64) -> Self {
        Self {
            logical_bytes: None,
            materialized_payload_bytes: None,
            materialization_count: 0,
            construction_ns,
            execution_ns: None,
        }
    }

    const fn materialized(
        construction_ns: u64,
        execution_ns: Option<u64>,
        logical_bytes: Option<u64>,
    ) -> Self {
        Self {
            logical_bytes,
            materialized_payload_bytes: logical_bytes,
            materialization_count: 1,
            construction_ns,
            execution_ns,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn failed_receipt(
    spec: &ExperimentSpec,
    workload: WorkloadIdentity,
    operation_support: OperationSupportClass,
    experiment_id: String,
    host: HostInfo,
    revision: String,
    revision_url: String,
    estimated_logical_bytes: Option<u64>,
    rss_before_bytes: Option<u64>,
    peak_before_bytes: Option<u64>,
    evidence: FailureEvidence,
    outcome: RunOutcome,
) -> Result<MemoryWallReceipt, HarnessError> {
    let peak_after = linux_peak_rss_bytes();
    finalize_receipt(ReceiptBody {
        experiment_id,
        source_revision: revision,
        source_revision_url: revision_url,
        representation: spec.representation,
        representation_id: spec.representation.id().into(),
        compute_backend: compute_backend(spec.representation).into(),
        worker_count: 1,
        dimension: spec.dimension,
        subsystems: spec.subsystems,
        rounds: spec.rounds,
        max_logical_bytes: spec.max_logical_bytes,
        oracle_logical_limit_bytes: spec.oracle_logical_limit_bytes,
        workload,
        operation_support,
        host,
        memory: MemoryMeasurements {
            estimated_logical_bytes,
            logical_bytes: evidence.logical_bytes,
            materialized_payload_bytes: evidence.materialized_payload_bytes,
            resident_working_set_bytes: None,
            rss_before_bytes,
            rss_after_bytes: linux_current_rss_bytes(),
            peak_process_rss_before_bytes: peak_before_bytes,
            peak_process_rss_bytes: peak_after,
            incremental_peak_rss_bytes: peak_delta(peak_before_bytes, peak_after),
            allocation_count: None,
            materialization_count: Some(evidence.materialization_count),
        },
        timings: TimingMeasurements {
            construction_ns: Some(evidence.construction_ns),
            execution_ns: evidence.execution_ns,
            snapshot_ns: None,
            oracle_verification_ns: None,
        },
        final_state_digest: None,
        norm_squared: None,
        oracle_agreement: OracleAgreement::NotApplicable,
        structured_candidate: None,
        outcome,
    })
}

fn dense_logical_bytes(state: &DenseState) -> Option<u64> {
    let amplitudes = u64::try_from(state.amplitudes().len()).ok()?;
    amplitudes.checked_mul(std::mem::size_of::<Complex64>() as u64)
}

fn finalize_receipt(body: ReceiptBody) -> Result<MemoryWallReceipt, HarnessError> {
    let canonical = serde_json::to_vec(&body)
        .map_err(|error| HarnessError::Serialization(error.to_string()))?;
    Ok(MemoryWallReceipt {
        schema: RECEIPT_SCHEMA.into(),
        receipt_id: format!("sha256:{}", sha256_hex(&canonical)),
        body,
    })
}

fn empty_timings() -> TimingMeasurements {
    TimingMeasurements {
        construction_ns: None,
        execution_ns: None,
        snapshot_ns: None,
        oracle_verification_ns: None,
    }
}

fn classify_stabilizer_error(error: StabilizerError) -> RunOutcome {
    match error {
        StabilizerError::NonPrimeDimension { .. }
        | StabilizerError::UnsupportedOperation { .. } => RunOutcome::Unsupported {
            reason: error.to_string(),
        },
        StabilizerError::AllocationSizeOverflow { .. }
        | StabilizerError::AllocationFailed { .. } => RunOutcome::AllocationFailed {
            reason: error.to_string(),
        },
        _ => RunOutcome::ExecutionFailed {
            reason: error.to_string(),
        },
    }
}

pub fn workload_operations(
    system: SystemSpec,
    rounds: usize,
) -> Result<Vec<Operation>, HarnessError> {
    if rounds == 0 {
        return Err(HarnessError::Workload("rounds must be at least 1".into()));
    }

    let per_round = if system.subsystems() == 1 { 3 } else { 5 };
    let capacity = rounds
        .checked_mul(per_round)
        .ok_or_else(|| HarnessError::Workload("operation count overflow".into()))?;

    let mut operations = Vec::new();
    operations
        .try_reserve_exact(capacity)
        .map_err(|_| HarnessError::Workload(format!("cannot reserve {capacity} operations")))?;

    for round in 0..rounds {
        let target = round % system.subsystems();
        operations.push(Operation::Fourier { target });

        if system.subsystems() == 1 {
            operations.push(Operation::WeylZ { target, power: 1 });
            operations.push(Operation::WeylX { target, shift: 1 });
            continue;
        }

        let next = (target + 1) % system.subsystems();
        operations.push(Operation::ControlledShift {
            control: target,
            target: next,
            shift: 1,
        });
        operations.push(Operation::WeylZ {
            target: next,
            power: 1,
        });
        operations.push(Operation::WeylX { target, shift: 1 });
        operations.push(Operation::Swap { a: target, b: next });
    }

    Ok(operations)
}

pub fn workload_identity(
    system: SystemSpec,
    rounds: usize,
    operations: &[Operation],
) -> WorkloadIdentity {
    let mut hasher = SemanticHasher::new();
    hash_bytes(&mut hasher, WORKLOAD_SCHEMA.as_bytes());
    hasher.update(&(system.dimension() as u128).to_be_bytes());
    hasher.update(&(system.subsystems() as u128).to_be_bytes());
    hasher.update(&(rounds as u128).to_be_bytes());
    hasher.update(&(operations.len() as u128).to_be_bytes());

    for operation in operations {
        let bytes = operation.canonical_bytes();
        hasher.update(&(bytes.len() as u128).to_be_bytes());
        hasher.update(&bytes);
    }

    WorkloadIdentity {
        schema: WORKLOAD_SCHEMA.into(),
        id: format!("sha256:{}", hasher.finalize_hex()),
        operation_count: operations.len(),
    }
}

fn compute_backend(representation: RepresentationKind) -> &'static str {
    match representation {
        RepresentationKind::Dense | RepresentationKind::PrimeStabilizer => "scalar-cpu",
        RepresentationKind::FlyPhi664Virtualized => "virtualized-serial-cpu",
    }
}

fn experiment_id(spec: &ExperimentSpec, workload: &WorkloadIdentity) -> String {
    let mut hasher = SemanticHasher::new();
    hash_bytes(&mut hasher, b"qsolqec.memorywall.experiment.v1");
    hash_bytes(&mut hasher, spec.representation.id().as_bytes());
    hasher.update(&(spec.dimension as u128).to_be_bytes());
    hasher.update(&(spec.subsystems as u128).to_be_bytes());
    hasher.update(&(spec.rounds as u128).to_be_bytes());
    hash_bytes(&mut hasher, workload.id.as_bytes());
    match spec.max_logical_bytes {
        Some(bytes) => {
            hasher.update(&[1]);
            hasher.update(&bytes.to_be_bytes());
        }
        None => hasher.update(&[0]),
    }
    hasher.update(&spec.oracle_logical_limit_bytes.to_be_bytes());
    hash_bytes(&mut hasher, compute_backend(spec.representation).as_bytes());

    if spec.representation == RepresentationKind::FlyPhi664Virtualized {
        hash_bytes(&mut hasher, b"male-cns:v1.0");
        let mut body_ids = spec.fly.body_ids.clone();
        body_ids.sort_unstable();
        hasher.update(&(body_ids.len() as u128).to_be_bytes());
        for body_id in body_ids {
            hasher.update(&body_id.to_be_bytes());
        }
        for value in [
            spec.fly.page_span as u128,
            spec.fly.tile_span as u128,
            spec.fly.sparse_max_occupancy as u128,
            spec.fly.bitmap_max_occupancy as u128,
            spec.fly.scratch_domains as u128,
            u128::from(spec.fly.owner_count),
            spec.fly.max_cached_states as u128,
            spec.fly.max_in_flight_generations as u128,
        ] {
            hasher.update(&value.to_be_bytes());
        }
    }

    format!("sha256:{}", hasher.finalize_hex())
}

fn hash_bytes(hasher: &mut SemanticHasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u128).to_be_bytes());
    hasher.update(bytes);
}

pub fn estimate_logical_bytes(
    representation: RepresentationKind,
    system: SystemSpec,
) -> Option<u64> {
    match representation {
        RepresentationKind::Dense => estimate_dense_bytes(system),
        RepresentationKind::PrimeStabilizer => {
            let n = system.subsystems() as u128;
            let scalars = n.checked_mul(n.checked_mul(2)?.checked_add(1)?)?;
            let bytes = scalars.checked_mul(std::mem::size_of::<usize>() as u128)?;
            u64::try_from(bytes).ok()
        }
        RepresentationKind::FlyPhi664Virtualized => estimate_dense_bytes(system),
    }
}

fn estimate_dense_bytes(system: SystemSpec) -> Option<u64> {
    let amplitudes = system.dense_state_len()? as u128;
    let bytes = amplitudes.checked_mul(std::mem::size_of::<Complex64>() as u128)?;
    u64::try_from(bytes).ok()
}

fn stabilizer_max_error(
    stabilizer: &PrimeStabilizerState,
    dense: &DenseState,
) -> Result<f64, HarnessError> {
    if stabilizer.spec() != dense.spec() {
        return Err(HarnessError::Workload(
            "oracle and stabilizer SystemSpec differ".into(),
        ));
    }

    let spec = dense.spec();
    let d = spec.dimension();
    let n = spec.subsystems();
    let mut max_error = (dense.norm_squared() - 1.0).abs();

    for generator in stabilizer.generators() {
        for (source_index, amplitude) in dense.amplitudes().iter().copied().enumerate() {
            let mut remainder = source_index;
            let mut place = 1usize;
            let mut destination = 0usize;
            let mut exponent = generator.phase();

            for subsystem in 0..n {
                let digit = remainder % d;
                remainder /= d;

                exponent = add_mod(exponent, mul_mod(generator.z()[subsystem], digit, d), d);
                let destination_digit = add_mod(digit, generator.x()[subsystem], d);
                destination =
                    destination
                        .checked_add(destination_digit.checked_mul(place).ok_or_else(|| {
                            HarnessError::Workload("oracle index overflow".into())
                        })?)
                        .ok_or_else(|| HarnessError::Workload("oracle index overflow".into()))?;

                if subsystem + 1 < n {
                    place = place
                        .checked_mul(d)
                        .ok_or_else(|| HarnessError::Workload("oracle place overflow".into()))?;
                }
            }

            let angle = std::f64::consts::TAU * exponent as f64 / d as f64;
            let phase = Complex64::from_polar(1.0, angle);
            let error = (amplitude * phase - dense.amplitudes()[destination]).norm();
            max_error = max_error.max(error);
        }
    }

    Ok(max_error)
}

fn add_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 + b as u128) % modulus as u128) as usize
}

fn mul_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 * b as u128) % modulus as u128) as usize
}

fn duration_ns(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn peak_delta(before: Option<u64>, after: Option<u64>) -> Option<u64> {
    Some(after?.saturating_sub(before?))
}

fn linux_current_rss_bytes() -> Option<u64> {
    linux_kib_field("/proc/self/status", "VmRSS:").and_then(|kib| kib.checked_mul(1024))
}

fn linux_peak_rss_bytes() -> Option<u64> {
    linux_kib_field("/proc/self/status", "VmHWM:").and_then(|kib| kib.checked_mul(1024))
}

fn linux_kib_field(path: &str, field: &str) -> Option<u64> {
    let text = fs::read_to_string(path).ok()?;
    parse_kib_field(&text, field)
}

fn parse_kib_field(text: &str, field: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(field)?.trim();
        rest.split_whitespace().next()?.parse().ok()
    })
}

fn linux_cpu_model() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim() == "model name" {
            Some(value.trim().to_owned())
        } else {
            None
        }
    })
}

fn probe_nvidia_gpus() -> Vec<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return Vec::new();
    };

    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            let name = fields.next()?.to_owned();
            let memory_mib = fields.next()?.parse::<u64>().ok();
            let driver = fields
                .next()
                .map(str::to_owned)
                .filter(|value| !value.is_empty());
            Some(GpuInfo {
                name,
                memory_total_bytes: memory_mib.and_then(|mib| mib.checked_mul(1024 * 1024)),
                driver_version: driver,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_is_deterministic_and_representation_independent() {
        let system = SystemSpec::new(3, 3).unwrap();
        let first = workload_operations(system, 4).unwrap();
        let second = workload_operations(system, 4).unwrap();
        assert_eq!(first, second);

        let first_id = workload_identity(system, 4, &first);
        let second_id = workload_identity(system, 4, &second);
        assert_eq!(first_id, second_id);
        assert_eq!(first_id.operation_count, 20);
    }

    #[test]
    fn estimates_dense_and_stabilizer_bytes_separately() {
        let system = SystemSpec::new(2, 12).unwrap();
        assert_eq!(
            estimate_logical_bytes(RepresentationKind::Dense, system),
            Some(4096 * 16)
        );
        assert_eq!(
            estimate_logical_bytes(RepresentationKind::PrimeStabilizer, system),
            Some(12 * 25 * std::mem::size_of::<usize>() as u64)
        );
        assert_eq!(
            estimate_logical_bytes(RepresentationKind::FlyPhi664Virtualized, system),
            Some(4096 * 16)
        );
    }

    #[test]
    fn logical_budget_rejects_before_materialization() {
        let mut spec = ExperimentSpec::new(RepresentationKind::Dense, 2, 20, 1);
        spec.max_logical_bytes = Some(1024);

        let receipt = run_experiment(&spec).unwrap();
        assert!(matches!(
            receipt.body.outcome,
            RunOutcome::LogicalBudgetExceeded { .. }
        ));
        assert_eq!(receipt.body.memory.materialization_count, Some(0));
        assert!(receipt.body.final_state_digest.is_none());
    }

    #[test]
    fn dense_small_run_succeeds() {
        let spec = ExperimentSpec::new(RepresentationKind::Dense, 2, 3, 2);
        let receipt = run_experiment(&spec).unwrap();

        assert!(receipt.body.outcome.is_success());
        assert_eq!(
            receipt.body.oracle_agreement,
            OracleAgreement::SelfReference
        );
        assert_eq!(receipt.body.workload.operation_count, 10);
        assert!(receipt.body.final_state_digest.is_some());
        assert_eq!(receipt.body.memory.logical_bytes, Some(8 * 16));
    }

    #[test]
    fn stabilizer_small_run_matches_dense_oracle() {
        let spec = ExperimentSpec::new(RepresentationKind::PrimeStabilizer, 3, 2, 2);
        let receipt = run_experiment(&spec).unwrap();

        assert!(receipt.body.outcome.is_success());
        assert!(matches!(
            receipt.body.oracle_agreement,
            OracleAgreement::Matched { .. }
        ));
        assert!(receipt.body.final_state_digest.is_some());
    }

    #[test]
    fn fly_small_run_matches_dense_and_reports_structured_metrics() {
        let spec = ExperimentSpec::new(RepresentationKind::FlyPhi664Virtualized, 2, 3, 2);
        let receipt = run_experiment(&spec).unwrap();

        assert!(receipt.body.outcome.is_success());
        assert_eq!(receipt.body.operation_support, OperationSupportClass::Exact);
        assert!(matches!(
            receipt.body.oracle_agreement,
            OracleAgreement::Matched {
                tolerance: 0.0,
                max_error: 0.0
            }
        ));
        assert_eq!(receipt.body.memory.logical_bytes, Some(8 * 16));
        assert!(receipt.body.memory.materialized_payload_bytes.is_some());
        assert!(receipt.body.memory.resident_working_set_bytes.is_some());

        let candidate = receipt.body.structured_candidate.as_ref().unwrap();
        assert_eq!(candidate.macro_node_count, 2);
        assert_eq!(candidate.logical_namespace_addresses, 2 * 664);
        assert!(candidate.materialized_address_count > 0);
        assert!(candidate.materialized_page_count > 0);
        assert_eq!(candidate.recomputed_generations, candidate.cache_misses);
        assert_eq!(
            candidate.reused_generations,
            candidate.cache_hits + candidate.invariant_reuses
        );
        assert!(candidate.peak_tracked_active_bytes >= candidate.worker_scratch_capacity_bytes);
    }

    #[test]
    fn fly_namespace_exhaustion_is_a_structured_terminal_point() {
        let spec = ExperimentSpec::new(RepresentationKind::FlyPhi664Virtualized, 2, 11, 1);
        let receipt = run_experiment(&spec).unwrap();

        assert!(matches!(
            receipt.body.outcome,
            RunOutcome::LogicalNamespaceInsufficient {
                required_addresses: 2048,
                available_addresses: 1328
            }
        ));
        assert!(receipt.body.final_state_digest.is_none());
    }

    #[test]
    fn fly_body_id_order_does_not_change_experiment_identity() {
        let first = ExperimentSpec::new(RepresentationKind::FlyPhi664Virtualized, 2, 2, 1);
        let mut second = first.clone();
        second.fly.body_ids.reverse();

        let first_receipt = run_experiment(&first).unwrap();
        let second_receipt = run_experiment(&second).unwrap();

        assert_eq!(
            first_receipt.body.experiment_id,
            second_receipt.body.experiment_id
        );
        assert_eq!(
            first_receipt
                .body
                .structured_candidate
                .as_ref()
                .unwrap()
                .body_ids_digest,
            second_receipt
                .body
                .structured_candidate
                .as_ref()
                .unwrap()
                .body_ids_digest
        );
    }

    #[test]
    fn composite_dimension_is_reported_unsupported() {
        let spec = ExperimentSpec::new(RepresentationKind::PrimeStabilizer, 4, 2, 1);
        let receipt = run_experiment(&spec).unwrap();
        assert!(matches!(
            receipt.body.outcome,
            RunOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn parses_linux_kib_fields() {
        let sample = "Name:\ttest\nVmRSS:\t1234 kB\nVmHWM:\t5678 kB\n";
        assert_eq!(parse_kib_field(sample, "VmRSS:"), Some(1234));
        assert_eq!(parse_kib_field(sample, "VmHWM:"), Some(5678));
    }

    #[test]
    fn receipts_bind_to_build_source_revision_and_public_locator() {
        let revision = source_revision();
        assert_eq!(revision.len(), 40);
        assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(
            source_revision_url(),
            format!("{SOURCE_REPOSITORY}/commit/{revision}")
        );
    }

    #[test]
    fn successful_receipts_record_exact_operation_support() {
        let dense =
            run_experiment(&ExperimentSpec::new(RepresentationKind::Dense, 2, 2, 1)).unwrap();
        assert_eq!(dense.body.operation_support, OperationSupportClass::Exact);

        let stabilizer = run_experiment(&ExperimentSpec::new(
            RepresentationKind::PrimeStabilizer,
            3,
            2,
            1,
        ))
        .unwrap();
        assert_eq!(
            stabilizer.body.operation_support,
            OperationSupportClass::Exact
        );

        let fly = run_experiment(&ExperimentSpec::new(
            RepresentationKind::FlyPhi664Virtualized,
            2,
            2,
            1,
        ))
        .unwrap();
        assert_eq!(fly.body.operation_support, OperationSupportClass::Exact);
    }

    #[test]
    fn unsupported_receipt_records_unsupported_operation_support() {
        let receipt = run_experiment(&ExperimentSpec::new(
            RepresentationKind::PrimeStabilizer,
            4,
            2,
            1,
        ))
        .unwrap();
        assert_eq!(
            receipt.body.operation_support,
            OperationSupportClass::Unsupported
        );
        assert!(matches!(
            receipt.body.outcome,
            RunOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn post_materialization_failure_receipt_preserves_wall_measurements() {
        let spec = ExperimentSpec::new(RepresentationKind::Dense, 2, 3, 1);
        let system = spec.system().unwrap();
        let operations = workload_operations(system, spec.rounds).unwrap();
        let workload = workload_identity(system, spec.rounds, &operations);
        let receipt = failed_receipt(
            &spec,
            workload,
            OperationSupportClass::Exact,
            "test-experiment".into(),
            probe_host(),
            source_revision(),
            source_revision_url(),
            Some(128),
            linux_current_rss_bytes(),
            linux_peak_rss_bytes(),
            FailureEvidence::materialized(11, Some(22), Some(128)),
            RunOutcome::AllocationFailed {
                reason: "synthetic post-construction failure".into(),
            },
        )
        .unwrap();

        assert_eq!(receipt.body.memory.logical_bytes, Some(128));
        assert_eq!(receipt.body.memory.materialized_payload_bytes, Some(128));
        assert_eq!(receipt.body.memory.materialization_count, Some(1));
        assert_eq!(receipt.body.timings.construction_ns, Some(11));
        assert_eq!(receipt.body.timings.execution_ns, Some(22));
    }

    #[test]
    fn successful_receipt_serializes_resource_limits() {
        let mut spec = ExperimentSpec::new(RepresentationKind::Dense, 2, 2, 1);
        spec.max_logical_bytes = Some(64 * 1024 * 1024);
        spec.oracle_logical_limit_bytes = 8 * 1024 * 1024;

        let receipt = run_experiment(&spec).unwrap();

        assert_eq!(receipt.body.max_logical_bytes, spec.max_logical_bytes);
        assert_eq!(
            receipt.body.oracle_logical_limit_bytes,
            spec.oracle_logical_limit_bytes
        );
    }

    #[test]
    fn receipt_round_trips_json() {
        let spec = ExperimentSpec::new(RepresentationKind::Dense, 2, 2, 1);
        let receipt = run_experiment(&spec).unwrap();
        let json = serde_json::to_string(&receipt).unwrap();
        let decoded: MemoryWallReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.body.experiment_id, receipt.body.experiment_id);
        assert_eq!(decoded.receipt_id, receipt.receipt_id);
        assert_eq!(decoded.schema, RECEIPT_SCHEMA);
        assert_eq!(decoded.schema, "qsolqec.memorywall.receipt.v2");
    }
}

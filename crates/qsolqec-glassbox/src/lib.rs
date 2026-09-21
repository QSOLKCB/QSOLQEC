//! Glass Box observation boundary for QSOLQEC.
//!
//! The observer wraps execution. It does not live inside the mathematical
//! kernels and does not become correctness authority.

use core::fmt;
use std::time::Instant;

use qsolqec_core::SystemSpec;
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_ops::Operation;
use sha2::{Digest, Sha256};

pub const RECEIPT_SCHEMA: &str = "qsolqec.glassbox.operation-receipt.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepresentationIdentity {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApproximationDeclaration {
    Exact,
    Approximate(ApproximationSpec),
}

/// Validated approximation metadata.
///
/// Fields are intentionally private so external representations cannot forge an
/// unchecked approximation declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ApproximationSpec {
    method: String,
    declared_absolute_error: Option<f64>,
}

impl ApproximationSpec {
    pub fn method(&self) -> &str {
        &self.method
    }

    pub const fn declared_absolute_error(&self) -> Option<f64> {
        self.declared_absolute_error
    }
}

impl ApproximationDeclaration {
    pub fn approximate(
        method: impl Into<String>,
        declared_absolute_error: Option<f64>,
    ) -> Result<Self, ApproximationError> {
        let method = method.into();
        if method.trim().is_empty() {
            return Err(ApproximationError::EmptyMethod);
        }

        let declared_absolute_error = match declared_absolute_error {
            Some(bound) => Some(canonical_nonnegative(
                bound,
                |bound| ApproximationError::InvalidAbsoluteError { bound },
            )?),
            None => None,
        };

        Ok(Self::Approximate(ApproximationSpec {
            method,
            declared_absolute_error,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarModel {
    Ieee754F64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComparisonRule {
    ExactBits,
    AbsoluteAmplitudeTolerance { tolerance: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizationPolicy {
    ObserveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumericalContract {
    scalar: ScalarModel,
    comparison: ComparisonRule,
    normalization: NormalizationPolicy,
}

impl NumericalContract {
    pub const fn exact_bits_f64() -> Self {
        Self {
            scalar: ScalarModel::Ieee754F64,
            comparison: ComparisonRule::ExactBits,
            normalization: NormalizationPolicy::ObserveOnly,
        }
    }

    pub fn absolute_amplitude_f64(tolerance: f64) -> Result<Self, NumericalContractError> {
        let tolerance = canonical_nonnegative(tolerance, |tolerance| {
            NumericalContractError::InvalidTolerance { tolerance }
        })?;

        Ok(Self {
            scalar: ScalarModel::Ieee754F64,
            comparison: ComparisonRule::AbsoluteAmplitudeTolerance { tolerance },
            normalization: NormalizationPolicy::ObserveOnly,
        })
    }

    pub const fn scalar(self) -> ScalarModel {
        self.scalar
    }

    pub const fn comparison(self) -> ComparisonRule {
        self.comparison
    }

    pub const fn normalization(self) -> NormalizationPolicy {
        self.normalization
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateSnapshot {
    pub representation: RepresentationIdentity,
    pub system: SystemSpec,
    pub approximation: ApproximationDeclaration,
    pub state_digest: String,
    pub norm_squared: f64,
    /// Logical bytes owned by the representation itself, not process RSS.
    pub logical_bytes: u128,
}

/// State surface that can be observed without the Glass Box knowing the
/// representation's internal storage.
pub trait ObservableState {
    fn observation_snapshot(&self) -> StateSnapshot;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationPhase {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservationEvent {
    pub sequence: u64,
    pub phase: ObservationPhase,
    pub snapshot: StateSnapshot,
    /// Wall-clock observation. Excluded from artifact identity.
    pub elapsed_ns: Option<u128>,
    pub outcome: Option<ExecutionOutcome>,
    /// Human diagnostic only. Excluded from artifact identity.
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservationReceipt {
    pub schema: &'static str,
    pub artifact_id: String,
    pub operation_id: String,
    pub operation_kind: &'static str,
    pub numerical_contract: NumericalContract,
    pub before: ObservationEvent,
    pub after: ObservationEvent,
}

pub struct ObservedExecution<E> {
    pub receipt: ObservationReceipt,
    pub result: Result<(), E>,
}

#[derive(Debug, Clone)]
pub struct GlassBox {
    numerical_contract: NumericalContract,
    next_sequence: u64,
}

impl GlassBox {
    pub const fn new(numerical_contract: NumericalContract) -> Self {
        Self {
            numerical_contract,
            next_sequence: 0,
        }
    }

    pub const fn numerical_contract(&self) -> NumericalContract {
        self.numerical_contract
    }

    pub fn observe_operation<S, E, F>(
        &mut self,
        state: &mut S,
        operation: &Operation,
        execute: F,
    ) -> Result<ObservedExecution<E>, GlassBoxError>
    where
        S: ObservableState,
        E: fmt::Display,
        F: FnOnce(&mut S) -> Result<(), E>,
    {
        let before_sequence = self.next_sequence;
        let after_sequence = before_sequence
            .checked_add(1)
            .ok_or(GlassBoxError::EventSequenceOverflow)?;
        let next_sequence = before_sequence
            .checked_add(2)
            .ok_or(GlassBoxError::EventSequenceOverflow)?;

        let before_snapshot = state.observation_snapshot();
        let operation_bytes = operation.canonical_bytes();
        let operation_id = format!("sha256:{}", sha256_hex(&operation_bytes));

        let start = Instant::now();
        let result = execute(state);
        let elapsed_ns = start.elapsed().as_nanos();
        let after_snapshot = state.observation_snapshot();

        let (outcome, diagnostic) = match &result {
            Ok(()) => (ExecutionOutcome::Success, None),
            Err(error) => (ExecutionOutcome::Failure, Some(error.to_string())),
        };

        let before = ObservationEvent {
            sequence: before_sequence,
            phase: ObservationPhase::Before,
            snapshot: before_snapshot,
            elapsed_ns: None,
            outcome: None,
            diagnostic: None,
        };
        let after = ObservationEvent {
            sequence: after_sequence,
            phase: ObservationPhase::After,
            snapshot: after_snapshot,
            elapsed_ns: Some(elapsed_ns),
            outcome: Some(outcome),
            diagnostic,
        };

        let artifact_id = artifact_identity(
            operation,
            self.numerical_contract,
            &before.snapshot,
            &after.snapshot,
            outcome,
        );

        self.next_sequence = next_sequence;

        Ok(ObservedExecution {
            receipt: ObservationReceipt {
                schema: RECEIPT_SCHEMA,
                artifact_id,
                operation_id,
                operation_kind: operation.kind(),
                numerical_contract: self.numerical_contract,
                before,
                after,
            },
            result,
        })
    }
}

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: "glassbox".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![Capability::Observer],
        consumes: vec![
            DataKind::QuditState,
            DataKind::OperationStream,
            DataKind::StateTransition,
        ],
        produces: vec![DataKind::Observation, DataKind::Artifact],
        experimental: true,
        maturity: Maturity::E2DeterministicFixture,
    }
}

/// Incremental SHA-256 helper for semantic state digests.
pub struct SemanticHasher {
    inner: Sha256,
}

impl SemanticHasher {
    pub fn new() -> Self {
        Self {
            inner: Sha256::new(),
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    pub fn finalize_hex(self) -> String {
        digest_to_hex(self.inner.finalize())
    }
}

impl Default for SemanticHasher {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable SHA-256 helper used for small canonical payloads.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = SemanticHasher::new();
    hasher.update(bytes);
    hasher.finalize_hex()
}

fn digest_to_hex(digest: impl AsRef<[u8]>) -> String {
    let bytes = digest.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn artifact_identity(
    operation: &Operation,
    numerical_contract: NumericalContract,
    before: &StateSnapshot,
    after: &StateSnapshot,
    outcome: ExecutionOutcome,
) -> String {
    let mut canonical = Vec::new();
    push_bytes(&mut canonical, RECEIPT_SCHEMA.as_bytes());
    push_bytes(&mut canonical, &operation.canonical_bytes());
    append_numerical_contract(&mut canonical, numerical_contract);
    append_snapshot(&mut canonical, before);
    append_snapshot(&mut canonical, after);
    canonical.push(match outcome {
        ExecutionOutcome::Success => 1,
        ExecutionOutcome::Failure => 2,
    });

    format!("sha256:{}", sha256_hex(&canonical))
}

fn append_numerical_contract(output: &mut Vec<u8>, contract: NumericalContract) {
    output.push(match contract.scalar {
        ScalarModel::Ieee754F64 => 1,
    });
    match contract.comparison {
        ComparisonRule::ExactBits => output.push(1),
        ComparisonRule::AbsoluteAmplitudeTolerance { tolerance } => {
            output.push(2);
            output.extend_from_slice(&tolerance.to_bits().to_be_bytes());
        }
    }
    output.push(match contract.normalization {
        NormalizationPolicy::ObserveOnly => 1,
    });
}

fn append_snapshot(output: &mut Vec<u8>, snapshot: &StateSnapshot) {
    push_bytes(output, snapshot.representation.id.as_bytes());
    push_bytes(output, snapshot.representation.version.as_bytes());
    output.extend_from_slice(&(snapshot.system.dimension() as u128).to_be_bytes());
    output.extend_from_slice(&(snapshot.system.subsystems() as u128).to_be_bytes());
    append_approximation(output, &snapshot.approximation);
    push_bytes(output, snapshot.state_digest.as_bytes());
    output.extend_from_slice(&snapshot.norm_squared.to_bits().to_be_bytes());
    output.extend_from_slice(&snapshot.logical_bytes.to_be_bytes());
}

fn append_approximation(output: &mut Vec<u8>, approximation: &ApproximationDeclaration) {
    match approximation {
        ApproximationDeclaration::Exact => output.push(1),
        ApproximationDeclaration::Approximate(spec) => {
            output.push(2);
            push_bytes(output, spec.method.as_bytes());
            match spec.declared_absolute_error {
                Some(bound) => {
                    output.push(1);
                    output.extend_from_slice(&bound.to_bits().to_be_bytes());
                }
                None => output.push(0),
            }
        }
    }
}

fn canonical_nonnegative<E>(value: f64, error: impl FnOnce(f64) -> E) -> Result<f64, E> {
    if !value.is_finite() || value < 0.0 {
        return Err(error(value));
    }

    Ok(if value == 0.0 { 0.0 } else { value })
}

fn push_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u128).to_be_bytes());
    output.extend_from_slice(bytes);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumericalContractError {
    InvalidTolerance { tolerance: f64 },
}

impl fmt::Display for NumericalContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTolerance { tolerance } => {
                write!(
                    f,
                    "numerical tolerance must be finite and non-negative, got {tolerance}"
                )
            }
        }
    }
}

impl std::error::Error for NumericalContractError {}

#[derive(Debug, Clone, PartialEq)]
pub enum ApproximationError {
    EmptyMethod,
    InvalidAbsoluteError { bound: f64 },
}

impl fmt::Display for ApproximationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMethod => f.write_str("approximation method must not be empty"),
            Self::InvalidAbsoluteError { bound } => write!(
                f,
                "declared approximation error must be finite and non-negative, got {bound}"
            ),
        }
    }
}

impl std::error::Error for ApproximationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassBoxError {
    EventSequenceOverflow,
}

impl fmt::Display for GlassBoxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventSequenceOverflow => f.write_str("Glass Box event sequence overflowed"),
        }
    }
}

impl std::error::Error for GlassBoxError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    #[derive(Debug, Clone)]
    struct DummyState {
        spec: SystemSpec,
        value: u64,
    }

    impl DummyState {
        fn new() -> Self {
            Self {
                spec: SystemSpec::new(3, 1).unwrap(),
                value: 0,
            }
        }
    }

    impl ObservableState for DummyState {
        fn observation_snapshot(&self) -> StateSnapshot {
            StateSnapshot {
                representation: RepresentationIdentity {
                    id: "dummy".into(),
                    version: "1".into(),
                },
                system: self.spec,
                approximation: ApproximationDeclaration::Exact,
                state_digest: sha256_hex(&self.value.to_be_bytes()),
                norm_squared: 1.0,
                logical_bytes: 8,
            }
        }
    }

    #[derive(Debug)]
    struct DummyError;

    impl fmt::Display for DummyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("dummy failure")
        }
    }

    #[test]
    fn numerical_contract_rejects_invalid_tolerance() {
        assert!(NumericalContract::absolute_amplitude_f64(f64::NAN).is_err());
        assert!(NumericalContract::absolute_amplitude_f64(-1.0).is_err());
    }

    #[test]
    fn numerical_contract_canonicalizes_signed_zero() {
        let positive = NumericalContract::absolute_amplitude_f64(0.0).unwrap();
        let negative = NumericalContract::absolute_amplitude_f64(-0.0).unwrap();

        assert_eq!(positive, negative);
        match negative.comparison() {
            ComparisonRule::AbsoluteAmplitudeTolerance { tolerance } => {
                assert_eq!(tolerance.to_bits(), 0.0f64.to_bits());
            }
            ComparisonRule::ExactBits => panic!("expected tolerance contract"),
        }
    }

    #[test]
    fn emits_before_and_after_events() {
        let contract = NumericalContract::absolute_amplitude_f64(1.0e-12).unwrap();
        let mut glassbox = GlassBox::new(contract);
        let mut state = DummyState::new();
        let operation = Operation::WeylX {
            target: 0,
            shift: 1,
        };

        let observed = glassbox
            .observe_operation(&mut state, &operation, |state| {
                state.value = 1;
                Ok::<(), Infallible>(())
            })
            .unwrap();

        assert!(observed.result.is_ok());
        assert_eq!(observed.receipt.schema, RECEIPT_SCHEMA);
        assert_eq!(observed.receipt.before.phase, ObservationPhase::Before);
        assert_eq!(observed.receipt.after.phase, ObservationPhase::After);
        assert_eq!(observed.receipt.before.sequence, 0);
        assert_eq!(observed.receipt.after.sequence, 1);
        assert_eq!(
            observed.receipt.after.outcome,
            Some(ExecutionOutcome::Success)
        );
        assert!(observed.receipt.after.elapsed_ns.is_some());
        assert_ne!(
            observed.receipt.before.snapshot.state_digest,
            observed.receipt.after.snapshot.state_digest
        );
    }

    #[test]
    fn artifact_identity_canonicalizes_signed_zero_tolerance() {
        let operation = Operation::WeylX {
            target: 0,
            shift: 1,
        };

        let mut positive_box =
            GlassBox::new(NumericalContract::absolute_amplitude_f64(0.0).unwrap());
        let mut positive_state = DummyState::new();
        let positive = positive_box
            .observe_operation(&mut positive_state, &operation, |state| {
                state.value = 1;
                Ok::<(), Infallible>(())
            })
            .unwrap()
            .receipt;

        let mut negative_box =
            GlassBox::new(NumericalContract::absolute_amplitude_f64(-0.0).unwrap());
        let mut negative_state = DummyState::new();
        let negative = negative_box
            .observe_operation(&mut negative_state, &operation, |state| {
                state.value = 1;
                Ok::<(), Infallible>(())
            })
            .unwrap()
            .receipt;

        assert_eq!(positive.artifact_id, negative.artifact_id);
    }

    #[test]
    fn artifact_identity_excludes_timing_and_event_sequence() {
        let contract = NumericalContract::absolute_amplitude_f64(1.0e-12).unwrap();
        let operation = Operation::WeylX {
            target: 0,
            shift: 1,
        };
        let mut glassbox = GlassBox::new(contract);

        let mut first = DummyState::new();
        let first_receipt = glassbox
            .observe_operation(&mut first, &operation, |state| {
                state.value = 1;
                Ok::<(), Infallible>(())
            })
            .unwrap()
            .receipt;

        let mut second = DummyState::new();
        let second_receipt = glassbox
            .observe_operation(&mut second, &operation, |state| {
                state.value = 1;
                Ok::<(), Infallible>(())
            })
            .unwrap()
            .receipt;

        assert_ne!(
            first_receipt.before.sequence,
            second_receipt.before.sequence
        );
        assert_eq!(first_receipt.artifact_id, second_receipt.artifact_id);
    }

    #[test]
    fn records_failed_execution_without_becoming_authority() {
        let mut glassbox = GlassBox::new(NumericalContract::exact_bits_f64());
        let mut state = DummyState::new();
        let operation = Operation::WeylX {
            target: 0,
            shift: 1,
        };

        let observed = glassbox
            .observe_operation(&mut state, &operation, |_state| {
                Err::<(), DummyError>(DummyError)
            })
            .unwrap();

        assert!(observed.result.is_err());
        assert_eq!(
            observed.receipt.after.outcome,
            Some(ExecutionOutcome::Failure)
        );
        assert_eq!(
            observed.receipt.after.diagnostic.as_deref(),
            Some("dummy failure")
        );
        assert_eq!(
            observed.receipt.before.snapshot.state_digest,
            observed.receipt.after.snapshot.state_digest
        );
    }

    #[test]
    fn approximation_constructor_rejects_bad_declarations() {
        assert!(ApproximationDeclaration::approximate("", None).is_err());
        assert!(ApproximationDeclaration::approximate("mps", Some(-0.1)).is_err());

        let declaration =
            ApproximationDeclaration::approximate("mps", Some(1.0e-6)).unwrap();
        match declaration {
            ApproximationDeclaration::Approximate(spec) => {
                assert_eq!(spec.method(), "mps");
                assert_eq!(spec.declared_absolute_error(), Some(1.0e-6));
            }
            ApproximationDeclaration::Exact => panic!("expected approximation"),
        }
    }

    #[test]
    fn approximation_error_bound_canonicalizes_signed_zero() {
        let declaration =
            ApproximationDeclaration::approximate("mps", Some(-0.0)).unwrap();
        match declaration {
            ApproximationDeclaration::Approximate(spec) => {
                assert_eq!(
                    spec.declared_absolute_error().unwrap().to_bits(),
                    0.0f64.to_bits()
                );
            }
            ApproximationDeclaration::Exact => panic!("expected approximation"),
        }
    }

    #[test]
    fn descriptor_is_observer_only() {
        let descriptor = module_descriptor();
        descriptor.validate().unwrap();

        assert_eq!(descriptor.capabilities, vec![Capability::Observer]);
        assert!(descriptor.consumes.contains(&DataKind::StateTransition));
        assert!(descriptor.produces.contains(&DataKind::Observation));
        assert!(descriptor.produces.contains(&DataKind::Artifact));
    }
}

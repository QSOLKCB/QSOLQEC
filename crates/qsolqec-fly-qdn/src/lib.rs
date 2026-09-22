//! R8 Gate-A Q(d,n) binding for the neutral Fly-Phi664 substrate.
//!
//! This is a correctness-first baseline, not the memory-saving result. The
//! frozen QSOLQEC basis index maps directly to the same-numbered Phi664 packed
//! address. Each finite Complex64 amplitude is encoded as 16 big-endian
//! IEEE-754 bytes; positive zero is implicit, while every other bit pattern is
//! materialized so exact round trips remain possible.
//!
//! Supported operations reconstruct the full amplitude vector, execute the R2
//! scalar semantics without calling DenseState, and atomically re-encode the
//! result. R8 Gate B must replace full reconstruction with virtualized
//! materialization while preserving this contract.

use core::fmt;
use std::f64::consts::TAU;

use num_complex::Complex64;
use qsolqec_core::SystemSpec;
use qsolqec_fly_phi664::{Phi664Address, Phi664Codec, Phi664Error, Phi664Store};
use qsolqec_glassbox::{
    ApproximationDeclaration, ObservableState, RepresentationIdentity, SemanticHasher,
    StateSnapshot,
};
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_ops::{Operation, OperationSupport, OperationValidationError};
use qsolqec_storage::{LogicalAddressCodec, PackedAddress, StorageContractError, StorageSnapshot};

pub const REPRESENTATION_ID: &str = "fly-phi664-qdn-baseline";
pub const ENCODING_ID: &str = "qsolqec.fly-phi664-qdn.complex64-be.v1";
pub const AMPLITUDE_BYTES: u128 = 16;
const STATE_DIGEST_DOMAIN: &[u8] = b"qsolqec.fly-phi664-qdn.semantic-state.v1";

#[derive(Debug, Clone)]
pub struct FlyQdnState {
    spec: SystemSpec,
    store: Phi664Store,
    state_len: usize,
    state_digest: String,
    norm_squared: f64,
}

impl FlyQdnState {
    pub fn from_amplitudes(
        codec: Phi664Codec,
        max_window_addresses: u128,
        spec: SystemSpec,
        amplitudes: Vec<Complex64>,
    ) -> Result<Self, FlyQdnError> {
        let expected = expected_state_len(spec)?;
        if amplitudes.len() != expected {
            return Err(FlyQdnError::AmplitudeCountMismatch {
                expected,
                actual: amplitudes.len(),
            });
        }
        validate_capacity(&codec, expected)?;
        validate_finite(&amplitudes)?;
        Self::encode(codec, max_window_addresses, spec, &amplitudes)
    }

    pub fn basis(
        codec: Phi664Codec,
        max_window_addresses: u128,
        spec: SystemSpec,
        basis_index: usize,
    ) -> Result<Self, FlyQdnError> {
        let state_len = expected_state_len(spec)?;
        validate_capacity(&codec, state_len)?;
        if basis_index >= state_len {
            return Err(FlyQdnError::BasisIndexOutOfRange {
                index: basis_index,
                state_len,
            });
        }
        let mut amplitudes = allocate_zeroed(state_len)?;
        amplitudes[basis_index] = Complex64::new(1.0, 0.0);
        Self::encode(codec, max_window_addresses, spec, &amplitudes)
    }

    pub fn zero(
        codec: Phi664Codec,
        max_window_addresses: u128,
        spec: SystemSpec,
    ) -> Result<Self, FlyQdnError> {
        Self::basis(codec, max_window_addresses, spec, 0)
    }

    fn encode(
        codec: Phi664Codec,
        max_window_addresses: u128,
        spec: SystemSpec,
        amplitudes: &[Complex64],
    ) -> Result<Self, FlyQdnError> {
        let state_len = expected_state_len(spec)?;
        if amplitudes.len() != state_len {
            return Err(FlyQdnError::AmplitudeCountMismatch {
                expected: state_len,
                actual: amplitudes.len(),
            });
        }
        validate_capacity(&codec, state_len)?;
        validate_finite(amplitudes)?;

        let mut store = Phi664Store::new(codec, max_window_addresses)?;
        for (basis_index, amplitude) in amplitudes.iter().copied().enumerate() {
            if is_implicit_positive_zero(amplitude) {
                continue;
            }
            let address = address_for_basis_index(store.codec(), basis_index)?;
            store.write_payload(&address, encode_amplitude(amplitude).to_vec())?;
        }

        let norm_squared = amplitudes
            .iter()
            .fold(0.0, |sum, amplitude| sum + amplitude.norm_sqr());
        Ok(Self {
            spec,
            store,
            state_len,
            state_digest: semantic_state_digest(spec, amplitudes),
            norm_squared,
        })
    }

    pub const fn spec(&self) -> SystemSpec {
        self.spec
    }

    pub const fn state_len(&self) -> usize {
        self.state_len
    }

    pub fn storage(&self) -> &Phi664Store {
        &self.store
    }

    pub fn storage_snapshot(&self) -> StorageSnapshot {
        self.store.snapshot()
    }

    pub fn basis_address(&self, basis_index: usize) -> Result<Phi664Address, FlyQdnError> {
        if basis_index >= self.state_len {
            return Err(FlyQdnError::BasisIndexOutOfRange {
                index: basis_index,
                state_len: self.state_len,
            });
        }
        address_for_basis_index(self.store.codec(), basis_index)
    }

    pub fn reconstruct(&self) -> Result<Vec<Complex64>, FlyQdnError> {
        let mut amplitudes = allocate_zeroed(self.state_len)?;
        for (basis_index, slot) in amplitudes.iter_mut().enumerate() {
            let address = address_for_basis_index(self.store.codec(), basis_index)?;
            if let Some(payload) = self.store.payload(&address)? {
                *slot = decode_amplitude(basis_index, payload)?;
            }
        }
        Ok(amplitudes)
    }

    pub fn support_for_spec(
        spec: SystemSpec,
        operation: &Operation,
    ) -> Result<OperationSupport, FlyQdnError> {
        operation
            .validate_for(spec)
            .map_err(FlyQdnError::InvalidOperation)?;
        Ok(match operation {
            Operation::WeylX { .. }
            | Operation::WeylZ { .. }
            | Operation::Fourier { .. }
            | Operation::ControlledShift { .. }
            | Operation::Swap { .. } => OperationSupport::Exact,
            Operation::LocalPermutation { .. } | Operation::LocalUnitary(_) => {
                OperationSupport::Unsupported
            }
        })
    }

    pub fn support_for(&self, operation: &Operation) -> Result<OperationSupport, FlyQdnError> {
        Self::support_for_spec(self.spec, operation)
    }

    pub fn apply_operation(&mut self, operation: &Operation) -> Result<(), FlyQdnError> {
        self.apply_operations(std::slice::from_ref(operation))
    }

    pub fn apply_operations(&mut self, operations: &[Operation]) -> Result<(), FlyQdnError> {
        for operation in operations {
            match Self::support_for_spec(self.spec, operation)? {
                OperationSupport::Exact => {}
                OperationSupport::Approximate => return Err(FlyQdnError::UnexpectedApproximate),
                OperationSupport::Unsupported => {
                    return Err(FlyQdnError::UnsupportedOperation {
                        kind: operation.kind(),
                    })
                }
            }
        }

        let mut amplitudes = self.reconstruct()?;
        for operation in operations {
            apply_supported(self.spec, &mut amplitudes, operation)?;
        }
        let next = Self::encode(
            self.store.codec().clone(),
            self.store.max_window_addresses(),
            self.spec,
            &amplitudes,
        )?;
        *self = next;
        Ok(())
    }

    pub fn compare_amplitudes(
        &self,
        reference: &[Complex64],
        tolerance: f64,
    ) -> Result<AmplitudeComparison, FlyQdnError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(FlyQdnError::InvalidComparisonTolerance { tolerance });
        }
        if reference.len() != self.state_len {
            return Err(FlyQdnError::AmplitudeCountMismatch {
                expected: self.state_len,
                actual: reference.len(),
            });
        }
        validate_finite(reference)?;

        let actual = self.reconstruct()?;
        let mut exact_bits = true;
        let mut max_absolute_error = 0.0f64;
        for (left, right) in actual.iter().zip(reference) {
            exact_bits &=
                left.re.to_bits() == right.re.to_bits() && left.im.to_bits() == right.im.to_bits();
            max_absolute_error = max_absolute_error.max((*left - *right).norm());
        }
        Ok(AmplitudeComparison {
            exact_bits,
            max_absolute_error,
            within_tolerance: max_absolute_error <= tolerance,
        })
    }

    pub fn logical_state_bytes(&self) -> u128 {
        self.state_len as u128 * AMPLITUDE_BYTES
    }
}

impl ObservableState for FlyQdnState {
    fn observation_snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            representation: RepresentationIdentity {
                id: REPRESENTATION_ID.into(),
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmplitudeComparison {
    pub exact_bits: bool,
    pub max_absolute_error: f64,
    pub within_tolerance: bool,
}

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: REPRESENTATION_ID.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            Capability::StateRepresentation,
            Capability::OperationExecution,
        ],
        consumes: vec![DataKind::QuditState, DataKind::OperationStream],
        produces: vec![
            DataKind::EncodedState,
            DataKind::QuditState,
            DataKind::StateTransition,
        ],
        experimental: true,
        maturity: Maturity::E3OracleCompared,
    }
}

fn expected_state_len(spec: SystemSpec) -> Result<usize, FlyQdnError> {
    spec.dense_state_len().ok_or(FlyQdnError::StateSizeOverflow)
}

fn validate_capacity(codec: &Phi664Codec, state_len: usize) -> Result<(), FlyQdnError> {
    let required = state_len as u128;
    let available = codec.geometry().logical_address_count();
    if required > available {
        return Err(FlyQdnError::InsufficientLogicalAddresses {
            required,
            available,
        });
    }
    Ok(())
}

fn validate_finite(amplitudes: &[Complex64]) -> Result<(), FlyQdnError> {
    for (index, amplitude) in amplitudes.iter().enumerate() {
        if !amplitude.re.is_finite() || !amplitude.im.is_finite() {
            return Err(FlyQdnError::NonFiniteAmplitude { index });
        }
    }
    Ok(())
}

fn allocate_zeroed(state_len: usize) -> Result<Vec<Complex64>, FlyQdnError> {
    let mut amplitudes = Vec::new();
    amplitudes
        .try_reserve_exact(state_len)
        .map_err(|_| FlyQdnError::AllocationFailed {
            amplitudes: state_len,
        })?;
    amplitudes.resize(state_len, Complex64::new(0.0, 0.0));
    Ok(amplitudes)
}

fn address_for_basis_index(
    codec: &Phi664Codec,
    basis_index: usize,
) -> Result<Phi664Address, FlyQdnError> {
    let packed = PackedAddress::bind(codec.geometry(), basis_index as u128)?;
    Ok(codec.unpack(&packed)?)
}

fn is_implicit_positive_zero(amplitude: Complex64) -> bool {
    amplitude.re.to_bits() == 0.0f64.to_bits() && amplitude.im.to_bits() == 0.0f64.to_bits()
}

fn encode_amplitude(amplitude: Complex64) -> [u8; 16] {
    let mut payload = [0u8; 16];
    payload[..8].copy_from_slice(&amplitude.re.to_bits().to_be_bytes());
    payload[8..].copy_from_slice(&amplitude.im.to_bits().to_be_bytes());
    payload
}

fn decode_amplitude(basis_index: usize, payload: &[u8]) -> Result<Complex64, FlyQdnError> {
    if payload.len() != 16 {
        return Err(FlyQdnError::MalformedAmplitudePayload {
            basis_index,
            bytes: payload.len(),
        });
    }
    let mut re = [0u8; 8];
    let mut im = [0u8; 8];
    re.copy_from_slice(&payload[..8]);
    im.copy_from_slice(&payload[8..]);
    let amplitude = Complex64::new(
        f64::from_bits(u64::from_be_bytes(re)),
        f64::from_bits(u64::from_be_bytes(im)),
    );
    if !amplitude.re.is_finite() || !amplitude.im.is_finite() {
        return Err(FlyQdnError::NonFiniteAmplitude { index: basis_index });
    }
    Ok(amplitude)
}

fn semantic_state_digest(spec: SystemSpec, amplitudes: &[Complex64]) -> String {
    let mut hasher = SemanticHasher::new();
    hasher.update(STATE_DIGEST_DOMAIN);
    hasher.update(&(spec.dimension() as u128).to_be_bytes());
    hasher.update(&(spec.subsystems() as u128).to_be_bytes());
    hasher.update(&[1]);
    hasher.update(ENCODING_ID.as_bytes());
    for amplitude in amplitudes {
        hasher.update(&amplitude.re.to_bits().to_be_bytes());
        hasher.update(&amplitude.im.to_bits().to_be_bytes());
    }
    hasher.finalize_hex()
}

fn apply_supported(
    spec: SystemSpec,
    amplitudes: &mut Vec<Complex64>,
    operation: &Operation,
) -> Result<(), FlyQdnError> {
    match operation {
        Operation::WeylX { target, shift } => apply_weyl_x(spec, amplitudes, *target, *shift),
        Operation::WeylZ { target, power } => apply_weyl_z(spec, amplitudes, *target, *power),
        Operation::Fourier { target } => apply_fourier(spec, amplitudes, *target),
        Operation::ControlledShift {
            control,
            target,
            shift,
        } => apply_controlled_shift(spec, amplitudes, *control, *target, *shift),
        Operation::Swap { a, b } => apply_swap(spec, amplitudes, *a, *b),
        Operation::LocalPermutation { .. } | Operation::LocalUnitary(_) => {
            Err(FlyQdnError::UnsupportedOperation {
                kind: operation.kind(),
            })
        }
    }
}

fn apply_weyl_x(
    spec: SystemSpec,
    amplitudes: &mut [Complex64],
    target: usize,
    shift: usize,
) -> Result<(), FlyQdnError> {
    let dimension = spec.dimension();
    let stride = subsystem_stride(spec, target)?;
    let block = stride
        .checked_mul(dimension)
        .ok_or(FlyQdnError::IndexArithmeticOverflow)?;
    let shift = shift % dimension;
    let mut lane = allocate_zeroed(dimension)?;
    for base in (0..amplitudes.len()).step_by(block) {
        for offset in 0..stride {
            for (digit, slot) in lane.iter_mut().enumerate() {
                *slot = amplitudes[base + offset + digit * stride];
            }
            for (digit, value) in lane.iter().copied().enumerate() {
                amplitudes[base + offset + ((digit + shift) % dimension) * stride] = value;
            }
        }
    }
    Ok(())
}

fn apply_weyl_z(
    spec: SystemSpec,
    amplitudes: &mut [Complex64],
    target: usize,
    power: usize,
) -> Result<(), FlyQdnError> {
    let dimension = spec.dimension();
    let stride = subsystem_stride(spec, target)?;
    let reduced_power = power % dimension;
    for (index, amplitude) in amplitudes.iter_mut().enumerate() {
        let digit = (index / stride) % dimension;
        let exponent = mul_mod(reduced_power, digit, dimension);
        let angle = TAU * exponent as f64 / dimension as f64;
        *amplitude *= Complex64::from_polar(1.0, angle);
    }
    Ok(())
}

fn apply_fourier(
    spec: SystemSpec,
    amplitudes: &mut [Complex64],
    target: usize,
) -> Result<(), FlyQdnError> {
    let dimension = spec.dimension();
    let stride = subsystem_stride(spec, target)?;
    let block = stride
        .checked_mul(dimension)
        .ok_or(FlyQdnError::IndexArithmeticOverflow)?;
    let scale = 1.0 / (dimension as f64).sqrt();
    let mut input = allocate_zeroed(dimension)?;
    let mut output = allocate_zeroed(dimension)?;
    for base in (0..amplitudes.len()).step_by(block) {
        for offset in 0..stride {
            for (digit, slot) in input.iter_mut().enumerate() {
                *slot = amplitudes[base + offset + digit * stride];
            }
            for (output_digit, output_slot) in output.iter_mut().enumerate() {
                let mut sum = Complex64::new(0.0, 0.0);
                for (input_digit, input_value) in input.iter().copied().enumerate() {
                    let exponent = mul_mod(input_digit, output_digit, dimension);
                    let angle = TAU * exponent as f64 / dimension as f64;
                    sum += input_value * Complex64::from_polar(1.0, angle);
                }
                *output_slot = sum * scale;
            }
            for (digit, value) in output.iter().copied().enumerate() {
                amplitudes[base + offset + digit * stride] = value;
            }
        }
    }
    Ok(())
}

fn apply_controlled_shift(
    spec: SystemSpec,
    amplitudes: &mut Vec<Complex64>,
    control: usize,
    target: usize,
    shift: usize,
) -> Result<(), FlyQdnError> {
    let dimension = spec.dimension();
    let control_stride = subsystem_stride(spec, control)?;
    let target_stride = subsystem_stride(spec, target)?;
    let reduced_shift = shift % dimension;
    let mut output = allocate_zeroed(amplitudes.len())?;
    for (index, amplitude) in amplitudes.iter().copied().enumerate() {
        let control_digit = (index / control_stride) % dimension;
        let target_digit = (index / target_stride) % dimension;
        let delta = mul_mod(control_digit, reduced_shift, dimension);
        let new_target = (target_digit + delta) % dimension;
        let destination = replace_digit(index, target_digit, new_target, target_stride)?;
        output[destination] = amplitude;
    }
    *amplitudes = output;
    Ok(())
}

fn apply_swap(
    spec: SystemSpec,
    amplitudes: &mut Vec<Complex64>,
    a: usize,
    b: usize,
) -> Result<(), FlyQdnError> {
    let dimension = spec.dimension();
    let stride_a = subsystem_stride(spec, a)?;
    let stride_b = subsystem_stride(spec, b)?;
    let mut output = allocate_zeroed(amplitudes.len())?;
    for (index, amplitude) in amplitudes.iter().copied().enumerate() {
        let digit_a = (index / stride_a) % dimension;
        let digit_b = (index / stride_b) % dimension;
        let first = replace_digit(index, digit_a, digit_b, stride_a)?;
        let destination = replace_digit(first, digit_b, digit_a, stride_b)?;
        output[destination] = amplitude;
    }
    *amplitudes = output;
    Ok(())
}

fn subsystem_stride(spec: SystemSpec, subsystem: usize) -> Result<usize, FlyQdnError> {
    let exponent = u32::try_from(subsystem).map_err(|_| FlyQdnError::IndexArithmeticOverflow)?;
    spec.dimension()
        .checked_pow(exponent)
        .ok_or(FlyQdnError::IndexArithmeticOverflow)
}

fn replace_digit(
    index: usize,
    old_digit: usize,
    new_digit: usize,
    stride: usize,
) -> Result<usize, FlyQdnError> {
    let old_term = old_digit
        .checked_mul(stride)
        .ok_or(FlyQdnError::IndexArithmeticOverflow)?;
    let new_term = new_digit
        .checked_mul(stride)
        .ok_or(FlyQdnError::IndexArithmeticOverflow)?;
    index
        .checked_sub(old_term)
        .and_then(|value| value.checked_add(new_term))
        .ok_or(FlyQdnError::IndexArithmeticOverflow)
}

fn mul_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 * b as u128) % modulus as u128) as usize
}

#[derive(Debug)]
pub enum FlyQdnError {
    StateSizeOverflow,
    AllocationFailed { amplitudes: usize },
    BasisIndexOutOfRange { index: usize, state_len: usize },
    AmplitudeCountMismatch { expected: usize, actual: usize },
    NonFiniteAmplitude { index: usize },
    InsufficientLogicalAddresses { required: u128, available: u128 },
    MalformedAmplitudePayload { basis_index: usize, bytes: usize },
    InvalidComparisonTolerance { tolerance: f64 },
    InvalidOperation(OperationValidationError),
    UnsupportedOperation { kind: &'static str },
    UnexpectedApproximate,
    IndexArithmeticOverflow,
    Phi664(Phi664Error),
    Storage(StorageContractError),
}

impl fmt::Display for FlyQdnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StateSizeOverflow => f.write_str("Q(d,n) state size overflows the platform address space"),
            Self::AllocationFailed { amplitudes } => write!(f, "failed to allocate {amplitudes} reconstructed amplitudes"),
            Self::BasisIndexOutOfRange { index, state_len } => write!(f, "basis index {index} is outside state length {state_len}"),
            Self::AmplitudeCountMismatch { expected, actual } => write!(f, "expected {expected} amplitudes, got {actual}"),
            Self::NonFiniteAmplitude { index } => write!(f, "amplitude {index} contains a non-finite component"),
            Self::InsufficientLogicalAddresses { required, available } => write!(f, "Q(d,n) requires {required} basis addresses but Phi664 geometry exposes only {available}"),
            Self::MalformedAmplitudePayload { basis_index, bytes } => write!(f, "basis amplitude {basis_index} has payload length {bytes}, expected 16"),
            Self::InvalidComparisonTolerance { tolerance } => write!(f, "comparison tolerance must be finite and non-negative, got {tolerance}"),
            Self::InvalidOperation(source) => write!(f, "invalid operation: {source}"),
            Self::UnsupportedOperation { kind } => write!(f, "operation {kind} is unsupported by the R8A baseline"),
            Self::UnexpectedApproximate => f.write_str("R8A baseline unexpectedly reported approximate support"),
            Self::IndexArithmeticOverflow => f.write_str("Q(d,n) operation index arithmetic overflowed"),
            Self::Phi664(source) => write!(f, "Fly-Phi664 storage error: {source}"),
            Self::Storage(source) => write!(f, "structured-storage error: {source}"),
        }
    }
}

impl std::error::Error for FlyQdnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidOperation(source) => Some(source),
            Self::Phi664(source) => Some(source),
            Self::Storage(source) => Some(source),
            _ => None,
        }
    }
}

impl From<Phi664Error> for FlyQdnError {
    fn from(value: Phi664Error) -> Self {
        Self::Phi664(value)
    }
}

impl From<StorageContractError> for FlyQdnError {
    fn from(value: StorageContractError) -> Self {
        Self::Storage(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsolqec_dense::DenseState;
    use qsolqec_fly_phi664::{FibreId, MacroSourceSpec};
    use qsolqec_glassbox::{GlassBox, NumericalContract};

    fn codec(body_ids: Vec<u64>) -> Phi664Codec {
        Phi664Codec::from_body_ids(
            MacroSourceSpec {
                dataset_id: "r8a-fixture:v1".into(),
                release_date: "2026-09-22".into(),
                license: "CC0".into(),
                node_source_uri: "fixture://nodes".into(),
                edge_source_uri: "fixture://edges".into(),
                node_identity_field: "bodyId".into(),
                node_projection: "fixture bodyId set".into(),
                edge_interpretation: "fixture directed edges".into(),
                graph_projection: "fixture identity only".into(),
            },
            body_ids,
        )
        .unwrap()
    }

    #[test]
    fn exact_round_trip_preserves_signed_zero_and_sparse_positive_zero() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let amplitudes = vec![
            Complex64::new(0.0, 0.0),
            Complex64::new(-0.0, 0.0),
            Complex64::new(0.25, -0.5),
            Complex64::new(0.0, 0.0),
            Complex64::new(0.0, 0.0),
            Complex64::new(0.0, 0.0),
            Complex64::new(0.0, 0.0),
            Complex64::new(0.0, 0.0),
            Complex64::new(1.0, 0.0),
        ];
        let state =
            FlyQdnState::from_amplitudes(codec(vec![10]), 64, spec, amplitudes.clone()).unwrap();
        assert_eq!(state.storage().materialized_address_count(), 3);
        let round_trip = state.reconstruct().unwrap();
        for (actual, expected) in round_trip.iter().zip(&amplitudes) {
            assert_eq!(actual.re.to_bits(), expected.re.to_bits());
            assert_eq!(actual.im.to_bits(), expected.im.to_bits());
        }
    }

    #[test]
    fn basis_mapping_crosses_phi664_fibre_boundaries() {
        let spec = SystemSpec::new(2, 8).unwrap();
        let state = FlyQdnState::zero(codec(vec![10]), 64, spec).unwrap();
        let a26 = state.basis_address(26).unwrap();
        let a27 = state.basis_address(27).unwrap();
        let a151 = state.basis_address(151).unwrap();
        let a152 = state.basis_address(152).unwrap();
        assert_eq!(a26.fibre(), FibreId::F27);
        assert_eq!((a26.x(), a26.y(), a26.z()), (2, 2, 2));
        assert_eq!(a27.fibre(), FibreId::N125);
        assert_eq!((a27.x(), a27.y(), a27.z()), (0, 0, 0));
        assert_eq!(a151.fibre(), FibreId::N125);
        assert_eq!((a151.x(), a151.y(), a151.z()), (4, 4, 4));
        assert_eq!(a152.fibre(), FibreId::R512);
    }

    #[test]
    fn rejects_state_larger_than_phi664_namespace() {
        let spec = SystemSpec::new(3, 6).unwrap();
        let error = FlyQdnState::zero(codec(vec![10]), 64, spec).unwrap_err();
        assert!(matches!(
            error,
            FlyQdnError::InsufficientLogicalAddresses {
                required: 729,
                available: 664
            }
        ));
    }

    #[test]
    fn semantic_digest_is_separate_from_macrograph_identity() {
        let spec = SystemSpec::new(2, 2).unwrap();
        let amplitudes = vec![
            Complex64::new(0.5, 0.0),
            Complex64::new(0.0, 0.5),
            Complex64::new(-0.5, 0.0),
            Complex64::new(0.0, -0.5),
        ];
        let first =
            FlyQdnState::from_amplitudes(codec(vec![10]), 64, spec, amplitudes.clone()).unwrap();
        let second = FlyQdnState::from_amplitudes(codec(vec![99]), 64, spec, amplitudes).unwrap();
        assert_eq!(
            first.observation_snapshot().state_digest,
            second.observation_snapshot().state_digest
        );
        assert_ne!(
            first.storage_snapshot().facts().geometry.digest(),
            second.storage_snapshot().facts().geometry.digest()
        );
    }

    #[test]
    fn supported_clifford_style_contract_matches_dense_oracle() {
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
        let mut fly = FlyQdnState::from_amplitudes(codec(vec![10]), 64, spec, amplitudes).unwrap();
        dense.apply_operations(&operations).unwrap();
        fly.apply_operations(&operations).unwrap();
        let comparison = fly.compare_amplitudes(dense.amplitudes(), 1.0e-12).unwrap();
        assert!(comparison.exact_bits);
        assert!(comparison.within_tolerance);
        assert_eq!(comparison.max_absolute_error, 0.0);
    }

    #[test]
    fn unsupported_operation_is_rejected_before_mutation() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let mut state = FlyQdnState::zero(codec(vec![10]), 64, spec).unwrap();
        let before = state.observation_snapshot().state_digest;
        let operation = Operation::LocalPermutation {
            target: 0,
            map: vec![1, 2, 0],
        };
        assert_eq!(
            state.support_for(&operation).unwrap(),
            OperationSupport::Unsupported
        );
        assert!(matches!(
            state.apply_operation(&operation),
            Err(FlyQdnError::UnsupportedOperation { .. })
        ));
        assert_eq!(before, state.observation_snapshot().state_digest);
    }

    #[test]
    fn glass_box_observes_qdn_bound_state() {
        let spec = SystemSpec::new(2, 2).unwrap();
        let mut state = FlyQdnState::zero(codec(vec![10]), 64, spec).unwrap();
        let operation = Operation::WeylX {
            target: 0,
            shift: 1,
        };
        let mut glassbox = GlassBox::new(NumericalContract::exact_bits_f64());
        let observed = glassbox
            .observe_operation(&mut state, &operation, |state| {
                state.apply_operation(&operation)
            })
            .unwrap();
        assert!(observed.result.is_ok());
        assert_ne!(
            observed.receipt.before.snapshot.state_digest,
            observed.receipt.after.snapshot.state_digest
        );
        assert_eq!(
            observed.receipt.after.snapshot.representation.id,
            REPRESENTATION_ID
        );
    }

    #[test]
    fn descriptor_is_oracle_compared_bound_representation() {
        let descriptor = module_descriptor();
        descriptor.validate().unwrap();
        assert_eq!(descriptor.maturity, Maturity::E3OracleCompared);
        assert!(descriptor
            .capabilities
            .contains(&Capability::StateRepresentation));
        assert!(descriptor
            .capabilities
            .contains(&Capability::OperationExecution));
        assert!(descriptor.produces.contains(&DataKind::EncodedState));
    }
}

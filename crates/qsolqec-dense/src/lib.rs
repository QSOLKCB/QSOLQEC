//! Exact dense qudit reference oracle and scalar operation executor.
//!
//! R1 introduced the dense state. R2 adds exact scalar execution for the
//! generalized qudit operation contract without constructing global d^n x d^n
//! matrices.

use core::fmt;
use std::f64::consts::TAU;

use num_complex::Complex64;
use qsolqec_core::SystemSpec;
use qsolqec_glassbox::{
    ApproximationDeclaration, ObservableState, RepresentationIdentity, SemanticHasher,
    StateSnapshot,
};
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_ops::{LocalUnitary, Operation, OperationSupport, OperationValidationError};

/// Dense reference state for Q(d,n).
#[derive(Debug, Clone, PartialEq)]
pub struct DenseState {
    spec: SystemSpec,
    amplitudes: Vec<Complex64>,
}

impl DenseState {
    /// Construct the all-zero basis state.
    pub fn zero(spec: SystemSpec) -> Result<Self, DenseStateError> {
        Self::basis(spec, 0)
    }

    /// Construct an exact computational-basis state.
    pub fn basis(spec: SystemSpec, basis_index: usize) -> Result<Self, DenseStateError> {
        let state_len = spec
            .dense_state_len()
            .ok_or(DenseStateError::StateSizeOverflow)?;

        if basis_index >= state_len {
            return Err(DenseStateError::BasisIndexOutOfRange {
                index: basis_index,
                state_len,
            });
        }

        let mut amplitudes = allocate_zeroed(state_len)?;
        amplitudes[basis_index] = Complex64::new(1.0, 0.0);

        Ok(Self { spec, amplitudes })
    }

    /// Construct a dense state from explicitly supplied amplitudes.
    ///
    /// The constructor checks shape and finiteness, but intentionally does not
    /// normalize the vector. Silent normalization would hide upstream errors
    /// and would make the reference oracle less useful.
    pub fn from_amplitudes(
        spec: SystemSpec,
        amplitudes: Vec<Complex64>,
    ) -> Result<Self, DenseStateError> {
        let expected = spec
            .dense_state_len()
            .ok_or(DenseStateError::StateSizeOverflow)?;

        if amplitudes.len() != expected {
            return Err(DenseStateError::AmplitudeCountMismatch {
                expected,
                actual: amplitudes.len(),
            });
        }

        for (index, amplitude) in amplitudes.iter().enumerate() {
            if !amplitude.re.is_finite() || !amplitude.im.is_finite() {
                return Err(DenseStateError::NonFiniteAmplitude { index });
            }
        }

        Ok(Self { spec, amplitudes })
    }

    pub const fn spec(&self) -> SystemSpec {
        self.spec
    }

    pub fn amplitudes(&self) -> &[Complex64] {
        &self.amplitudes
    }

    /// Serial, fixed-order norm calculation for the reference implementation.
    pub fn norm_squared(&self) -> f64 {
        self.amplitudes
            .iter()
            .fold(0.0, |sum, amplitude| sum + amplitude.norm_sqr())
    }

    /// Raw Born weights for each computational-basis outcome.
    ///
    /// These values are not renormalized. For a normalized state they sum to
    /// one; for a malformed/non-normalized state their sum exposes that fact.
    pub fn probabilities(&self) -> Vec<f64> {
        self.amplitudes.iter().map(Complex64::norm_sqr).collect()
    }

    pub fn basis_probability(&self, basis_index: usize) -> Result<f64, DenseStateError> {
        let amplitude =
            self.amplitudes
                .get(basis_index)
                .ok_or(DenseStateError::BasisIndexOutOfRange {
                    index: basis_index,
                    state_len: self.amplitudes.len(),
                })?;

        Ok(amplitude.norm_sqr())
    }

    /// Report operation support for a declared system without constructing a state.
    pub fn support_for_spec(
        spec: SystemSpec,
        operation: &Operation,
    ) -> Result<OperationSupport, DenseOperationError> {
        operation
            .validate_for(spec)
            .map_err(DenseOperationError::InvalidOperation)?;
        Ok(OperationSupport::Exact)
    }

    /// Report support for this state's declared system.
    pub fn support_for(
        &self,
        operation: &Operation,
    ) -> Result<OperationSupport, DenseOperationError> {
        Self::support_for_spec(self.spec, operation)
    }

    /// Apply one validated generalized-qudit operation.
    pub fn apply_operation(&mut self, operation: &Operation) -> Result<(), DenseOperationError> {
        Self::support_for_spec(self.spec, operation)?;
        self.apply_validated(operation)
    }

    /// Apply a sequence atomically with respect to structural validation.
    ///
    /// Every operation is validated before the first mutation. Numerical
    /// execution errors can still fail during application, but an invalid
    /// later operation cannot leave a partially executed sequence.
    pub fn apply_operations(
        &mut self,
        operations: &[Operation],
    ) -> Result<(), DenseOperationError> {
        for operation in operations {
            Self::support_for_spec(self.spec, operation)?;
        }

        for operation in operations {
            self.apply_validated(operation)?;
        }

        Ok(())
    }

    fn apply_validated(&mut self, operation: &Operation) -> Result<(), DenseOperationError> {
        match operation {
            Operation::WeylX { target, shift } => self.apply_weyl_x(*target, *shift),
            Operation::WeylZ { target, power } => self.apply_weyl_z(*target, *power),
            Operation::Fourier { target } => self.apply_fourier(*target),
            Operation::ControlledShift {
                control,
                target,
                shift,
            } => self.apply_controlled_shift(*control, *target, *shift),
            Operation::Swap { a, b } => self.apply_swap(*a, *b),
            Operation::LocalPermutation { target, map } => {
                self.apply_local_permutation(*target, map)
            }
            Operation::LocalUnitary(unitary) => self.apply_local_unitary(unitary),
        }
    }

    fn apply_weyl_x(&mut self, target: usize, shift: usize) -> Result<(), DenseOperationError> {
        let dimension = self.spec.dimension();
        let stride = subsystem_stride(self.spec, target)?;
        let block = stride
            .checked_mul(dimension)
            .ok_or(DenseOperationError::IndexArithmeticOverflow)?;
        let shift = shift % dimension;
        let mut lane = allocate_zeroed(dimension)?;

        for base in (0..self.amplitudes.len()).step_by(block) {
            for offset in 0..stride {
                for (digit, slot) in lane.iter_mut().enumerate() {
                    *slot = self.amplitudes[base + offset + digit * stride];
                }
                for (digit, value) in lane.iter().copied().enumerate() {
                    let output = (digit + shift) % dimension;
                    self.amplitudes[base + offset + output * stride] = value;
                }
            }
        }

        Ok(())
    }

    fn apply_weyl_z(&mut self, target: usize, power: usize) -> Result<(), DenseOperationError> {
        let dimension = self.spec.dimension();
        let stride = subsystem_stride(self.spec, target)?;
        let reduced_power = power % dimension;

        for (index, amplitude) in self.amplitudes.iter_mut().enumerate() {
            let digit = (index / stride) % dimension;
            let exponent = mul_mod(reduced_power, digit, dimension);
            let angle = TAU * exponent as f64 / dimension as f64;
            *amplitude *= Complex64::from_polar(1.0, angle);
        }

        Ok(())
    }

    fn apply_fourier(&mut self, target: usize) -> Result<(), DenseOperationError> {
        let dimension = self.spec.dimension();
        let stride = subsystem_stride(self.spec, target)?;
        let block = stride
            .checked_mul(dimension)
            .ok_or(DenseOperationError::IndexArithmeticOverflow)?;
        let scale = 1.0 / (dimension as f64).sqrt();
        let mut input = allocate_zeroed(dimension)?;
        let mut output = allocate_zeroed(dimension)?;

        for base in (0..self.amplitudes.len()).step_by(block) {
            for offset in 0..stride {
                for (digit, slot) in input.iter_mut().enumerate() {
                    *slot = self.amplitudes[base + offset + digit * stride];
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
                    self.amplitudes[base + offset + digit * stride] = value;
                }
            }
        }

        Ok(())
    }

    fn apply_controlled_shift(
        &mut self,
        control: usize,
        target: usize,
        shift: usize,
    ) -> Result<(), DenseOperationError> {
        let dimension = self.spec.dimension();
        let control_stride = subsystem_stride(self.spec, control)?;
        let target_stride = subsystem_stride(self.spec, target)?;
        let reduced_shift = shift % dimension;
        let mut output = allocate_zeroed(self.amplitudes.len())?;

        for (index, amplitude) in self.amplitudes.iter().copied().enumerate() {
            let control_digit = (index / control_stride) % dimension;
            let target_digit = (index / target_stride) % dimension;
            let delta = mul_mod(control_digit, reduced_shift, dimension);
            let new_target = (target_digit + delta) % dimension;
            let destination = replace_digit(index, target_digit, new_target, target_stride)?;
            output[destination] = amplitude;
        }

        self.amplitudes = output;
        Ok(())
    }

    fn apply_swap(&mut self, a: usize, b: usize) -> Result<(), DenseOperationError> {
        let dimension = self.spec.dimension();
        let stride_a = subsystem_stride(self.spec, a)?;
        let stride_b = subsystem_stride(self.spec, b)?;
        let mut output = allocate_zeroed(self.amplitudes.len())?;

        for (index, amplitude) in self.amplitudes.iter().copied().enumerate() {
            let digit_a = (index / stride_a) % dimension;
            let digit_b = (index / stride_b) % dimension;
            let first = replace_digit(index, digit_a, digit_b, stride_a)?;
            let destination = replace_digit(first, digit_b, digit_a, stride_b)?;
            output[destination] = amplitude;
        }

        self.amplitudes = output;
        Ok(())
    }

    fn apply_local_permutation(
        &mut self,
        target: usize,
        map: &[usize],
    ) -> Result<(), DenseOperationError> {
        let dimension = self.spec.dimension();
        let stride = subsystem_stride(self.spec, target)?;
        let mut output = allocate_zeroed(self.amplitudes.len())?;

        for (index, amplitude) in self.amplitudes.iter().copied().enumerate() {
            let digit = (index / stride) % dimension;
            let destination = replace_digit(index, digit, map[digit], stride)?;
            output[destination] = amplitude;
        }

        self.amplitudes = output;
        Ok(())
    }

    fn apply_local_unitary(&mut self, unitary: &LocalUnitary) -> Result<(), DenseOperationError> {
        let dimension = self.spec.dimension();
        let stride = subsystem_stride(self.spec, unitary.target())?;
        let block = stride
            .checked_mul(dimension)
            .ok_or(DenseOperationError::IndexArithmeticOverflow)?;
        let matrix = unitary.matrix();
        let mut input = allocate_zeroed(dimension)?;
        let mut output = allocate_zeroed(dimension)?;

        for base in (0..self.amplitudes.len()).step_by(block) {
            for offset in 0..stride {
                for (digit, slot) in input.iter_mut().enumerate() {
                    *slot = self.amplitudes[base + offset + digit * stride];
                }

                for (row, output_slot) in output.iter_mut().enumerate() {
                    let mut sum = Complex64::new(0.0, 0.0);
                    for (column, input_value) in input.iter().copied().enumerate() {
                        sum += matrix[row * dimension + column] * input_value;
                    }
                    *output_slot = sum;
                }

                for (digit, value) in output.iter().copied().enumerate() {
                    self.amplitudes[base + offset + digit * stride] = value;
                }
            }
        }

        Ok(())
    }
}

impl ObservableState for DenseState {
    fn observation_snapshot(&self) -> StateSnapshot {
        let mut hasher = SemanticHasher::new();
        hasher.update(b"qsolqec.dense-state.v1");
        hasher.update(&(self.spec.dimension() as u128).to_be_bytes());
        hasher.update(&(self.spec.subsystems() as u128).to_be_bytes());
        hasher.update(&[1]); // SubsystemZeroLeastSignificant

        for amplitude in &self.amplitudes {
            hasher.update(&amplitude.re.to_bits().to_be_bytes());
            hasher.update(&amplitude.im.to_bits().to_be_bytes());
        }

        StateSnapshot {
            representation: RepresentationIdentity {
                id: "dense-reference".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            system: self.spec,
            approximation: ApproximationDeclaration::Exact,
            state_digest: hasher.finalize_hex(),
            norm_squared: self.norm_squared(),
            logical_bytes: self.amplitudes.len() as u128 * std::mem::size_of::<Complex64>() as u128,
        }
    }
}

fn allocate_zeroed(state_len: usize) -> Result<Vec<Complex64>, DenseStateError> {
    let mut amplitudes = Vec::new();
    amplitudes
        .try_reserve_exact(state_len)
        .map_err(|_| DenseStateError::AllocationFailed {
            amplitudes: state_len,
        })?;
    amplitudes.resize(state_len, Complex64::new(0.0, 0.0));
    Ok(amplitudes)
}

fn subsystem_stride(spec: SystemSpec, subsystem: usize) -> Result<usize, DenseOperationError> {
    let exponent =
        u32::try_from(subsystem).map_err(|_| DenseOperationError::IndexArithmeticOverflow)?;
    spec.dimension()
        .checked_pow(exponent)
        .ok_or(DenseOperationError::IndexArithmeticOverflow)
}

fn replace_digit(
    index: usize,
    old_digit: usize,
    new_digit: usize,
    stride: usize,
) -> Result<usize, DenseOperationError> {
    let old_term = old_digit
        .checked_mul(stride)
        .ok_or(DenseOperationError::IndexArithmeticOverflow)?;
    let new_term = new_digit
        .checked_mul(stride)
        .ok_or(DenseOperationError::IndexArithmeticOverflow)?;
    let without_old = index
        .checked_sub(old_term)
        .ok_or(DenseOperationError::IndexArithmeticOverflow)?;
    without_old
        .checked_add(new_term)
        .ok_or(DenseOperationError::IndexArithmeticOverflow)
}

fn mul_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 * b as u128) % modulus as u128) as usize
}

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: "dense-reference".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            Capability::StateRepresentation,
            Capability::OperationExecution,
            Capability::Oracle,
        ],
        consumes: vec![DataKind::QuditState, DataKind::OperationStream],
        produces: vec![
            DataKind::QuditState,
            DataKind::StateTransition,
            DataKind::ProbabilityDistribution,
        ],
        experimental: true,
        maturity: Maturity::E2DeterministicFixture,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenseStateError {
    StateSizeOverflow,
    AllocationFailed { amplitudes: usize },
    BasisIndexOutOfRange { index: usize, state_len: usize },
    AmplitudeCountMismatch { expected: usize, actual: usize },
    NonFiniteAmplitude { index: usize },
}

impl fmt::Display for DenseStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StateSizeOverflow => {
                f.write_str("dense state size overflows the platform address space")
            }
            Self::AllocationFailed { amplitudes } => {
                write!(f, "failed to allocate {amplitudes} dense amplitudes")
            }
            Self::BasisIndexOutOfRange { index, state_len } => {
                write!(f, "basis index {index} is outside state length {state_len}")
            }
            Self::AmplitudeCountMismatch { expected, actual } => {
                write!(f, "expected {expected} amplitudes, got {actual}")
            }
            Self::NonFiniteAmplitude { index } => {
                write!(f, "amplitude {index} contains a non-finite component")
            }
        }
    }
}

impl std::error::Error for DenseStateError {}

#[derive(Debug)]
pub enum DenseOperationError {
    InvalidOperation(OperationValidationError),
    State(DenseStateError),
    IndexArithmeticOverflow,
}

impl fmt::Display for DenseOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOperation(source) => write!(f, "invalid operation: {source}"),
            Self::State(source) => write!(f, "dense-state error: {source}"),
            Self::IndexArithmeticOverflow => {
                f.write_str("dense operation index arithmetic overflowed")
            }
        }
    }
}

impl std::error::Error for DenseOperationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidOperation(source) => Some(source),
            Self::State(source) => Some(source),
            Self::IndexArithmeticOverflow => None,
        }
    }
}

impl From<DenseStateError> for DenseOperationError {
    fn from(value: DenseStateError) -> Self {
        Self::State(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsolqec_ops::{LocalUnitaryError, OperationValidationError};
    use serde_json::Value;

    const EPSILON: f64 = 1.0e-12;

    fn assert_basis_fixture(raw: &str) {
        let fixture: Value = serde_json::from_str(raw).unwrap();

        assert_eq!(
            fixture["schema"].as_str(),
            Some("qsolqec.dense-basis-fixture.v1")
        );

        let dimension = fixture["dimension"].as_u64().unwrap() as usize;
        let subsystems = fixture["subsystems"].as_u64().unwrap() as usize;
        let digits: Vec<usize> = fixture["digits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() as usize)
            .collect();
        let expected_index = fixture["basis_index"].as_u64().unwrap() as usize;
        let expected_len = fixture["state_len"].as_u64().unwrap() as usize;
        let expected_probabilities: Vec<f64> = fixture["probabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_f64().unwrap())
            .collect();

        let spec = SystemSpec::new(dimension, subsystems).unwrap();
        assert_eq!(spec.dense_state_len(), Some(expected_len));
        assert_eq!(spec.basis_index(&digits), Ok(expected_index));
        assert_eq!(spec.basis_digits(expected_index), Ok(digits));

        let state = DenseState::basis(spec, expected_index).unwrap();
        assert_eq!(state.norm_squared(), 1.0);
        assert_eq!(state.probabilities(), expected_probabilities);
    }

    fn assert_states_close(left: &DenseState, right: &DenseState) {
        assert_eq!(left.spec(), right.spec());
        assert_eq!(left.amplitudes().len(), right.amplitudes().len());

        for (index, (lhs, rhs)) in left.amplitudes().iter().zip(right.amplitudes()).enumerate() {
            assert!(
                (*lhs - *rhs).norm() <= EPSILON,
                "amplitude {index} differs: {lhs:?} vs {rhs:?}"
            );
        }
    }

    fn basis_from_digits(spec: SystemSpec, digits: &[usize]) -> DenseState {
        DenseState::basis(spec, spec.basis_index(digits).unwrap()).unwrap()
    }

    #[test]
    fn zero_state_is_first_basis_state() {
        let state = DenseState::zero(SystemSpec::new(3, 2).unwrap()).unwrap();

        assert_eq!(state.amplitudes().len(), 9);
        assert_eq!(state.amplitudes()[0], Complex64::new(1.0, 0.0));
        assert_eq!(state.norm_squared(), 1.0);
    }

    #[test]
    fn supplied_amplitudes_are_not_silently_normalized() {
        let state = DenseState::from_amplitudes(
            SystemSpec::new(2, 1).unwrap(),
            vec![Complex64::new(1.0, 0.0), Complex64::new(1.0, 0.0)],
        )
        .unwrap();

        assert_eq!(state.norm_squared(), 2.0);
        assert_eq!(state.probabilities(), vec![1.0, 1.0]);
    }

    #[test]
    fn rejects_wrong_amplitude_count() {
        let error = DenseState::from_amplitudes(
            SystemSpec::new(2, 2).unwrap(),
            vec![Complex64::new(1.0, 0.0)],
        )
        .unwrap_err();

        assert_eq!(
            error,
            DenseStateError::AmplitudeCountMismatch {
                expected: 4,
                actual: 1
            }
        );
    }

    #[test]
    fn rejects_non_finite_amplitude() {
        let error = DenseState::from_amplitudes(
            SystemSpec::new(2, 1).unwrap(),
            vec![Complex64::new(1.0, 0.0), Complex64::new(f64::NAN, 0.0)],
        )
        .unwrap_err();

        assert_eq!(error, DenseStateError::NonFiniteAmplitude { index: 1 });
    }

    #[test]
    fn qubit_fixture_is_exact() {
        assert_basis_fixture(include_str!("../../../fixtures/dense/q2-n2-basis.json"));
    }

    #[test]
    fn qutrit_fixture_is_exact() {
        assert_basis_fixture(include_str!("../../../fixtures/dense/q3-n2-basis.json"));
    }

    #[test]
    fn ququart_fixture_is_exact() {
        assert_basis_fixture(include_str!("../../../fixtures/dense/q4-n2-basis.json"));
    }

    #[test]
    fn weyl_x_to_the_d_is_identity_for_qubit_qutrit_and_ququart() {
        for dimension in 2..=4 {
            let spec = SystemSpec::new(dimension, 1).unwrap();
            let original = basis_from_digits(spec, &[0]);
            let mut state = original.clone();

            for _ in 0..dimension {
                state
                    .apply_operation(&Operation::WeylX {
                        target: 0,
                        shift: 1,
                    })
                    .unwrap();
            }

            assert_states_close(&state, &original);
        }
    }

    #[test]
    fn weyl_z_to_the_d_is_identity_for_qubit_qutrit_and_ququart() {
        for dimension in 2..=4 {
            let spec = SystemSpec::new(dimension, 1).unwrap();
            let original = basis_from_digits(spec, &[1]);
            let mut state = original.clone();

            for _ in 0..dimension {
                state
                    .apply_operation(&Operation::WeylZ {
                        target: 0,
                        power: 1,
                    })
                    .unwrap();
            }

            assert_states_close(&state, &original);
        }
    }

    #[test]
    fn weyl_commutation_relation_holds() {
        for dimension in 2..=4 {
            let spec = SystemSpec::new(dimension, 1).unwrap();
            let initial = basis_from_digits(spec, &[0]);

            let mut zx = initial.clone();
            zx.apply_operation(&Operation::WeylX {
                target: 0,
                shift: 1,
            })
            .unwrap();
            zx.apply_operation(&Operation::WeylZ {
                target: 0,
                power: 1,
            })
            .unwrap();

            let mut xz = initial.clone();
            xz.apply_operation(&Operation::WeylZ {
                target: 0,
                power: 1,
            })
            .unwrap();
            xz.apply_operation(&Operation::WeylX {
                target: 0,
                shift: 1,
            })
            .unwrap();

            let omega = Complex64::from_polar(1.0, TAU / dimension as f64);
            for (left, right) in zx.amplitudes().iter().zip(xz.amplitudes()) {
                assert!((*left - omega * *right).norm() <= EPSILON);
            }
        }
    }

    #[test]
    fn fourier_fourth_power_is_identity() {
        for dimension in 2..=4 {
            let spec = SystemSpec::new(dimension, 1).unwrap();
            let original = basis_from_digits(spec, &[1]);
            let mut state = original.clone();

            for _ in 0..4 {
                state
                    .apply_operation(&Operation::Fourier { target: 0 })
                    .unwrap();
            }

            assert_states_close(&state, &original);
        }
    }

    #[test]
    fn controlled_shift_uses_control_digit_as_multiplier() {
        let spec = SystemSpec::new(4, 2).unwrap();
        let mut state = basis_from_digits(spec, &[3, 2]);

        state
            .apply_operation(&Operation::ControlledShift {
                control: 0,
                target: 1,
                shift: 1,
            })
            .unwrap();

        assert_states_close(&state, &basis_from_digits(spec, &[3, 1]));
    }

    #[test]
    fn swap_exchanges_subsystem_digits_and_squares_to_identity() {
        let spec = SystemSpec::new(4, 2).unwrap();
        let original = basis_from_digits(spec, &[3, 1]);
        let mut state = original.clone();

        state
            .apply_operation(&Operation::Swap { a: 0, b: 1 })
            .unwrap();
        assert_states_close(&state, &basis_from_digits(spec, &[1, 3]));

        state
            .apply_operation(&Operation::Swap { a: 0, b: 1 })
            .unwrap();
        assert_states_close(&state, &original);
    }

    #[test]
    fn local_permutation_uses_input_to_output_mapping() {
        let spec = SystemSpec::new(4, 2).unwrap();
        let mut state = basis_from_digits(spec, &[1, 2]);

        state
            .apply_operation(&Operation::LocalPermutation {
                target: 0,
                map: vec![2, 0, 3, 1],
            })
            .unwrap();

        assert_states_close(&state, &basis_from_digits(spec, &[0, 2]));
    }

    #[test]
    fn generic_local_unitary_applies_without_global_matrix() {
        let spec = SystemSpec::new(2, 1).unwrap();
        let scale = 1.0 / 2.0_f64.sqrt();
        let h = LocalUnitary::new(
            0,
            2,
            vec![
                Complex64::new(scale, 0.0),
                Complex64::new(scale, 0.0),
                Complex64::new(scale, 0.0),
                Complex64::new(-scale, 0.0),
            ],
        )
        .unwrap();

        let original = DenseState::zero(spec).unwrap();
        let mut state = original.clone();

        state
            .apply_operation(&Operation::LocalUnitary(h.clone()))
            .unwrap();
        let probabilities = state.probabilities();
        assert!((probabilities[0] - 0.5).abs() <= EPSILON);
        assert!((probabilities[1] - 0.5).abs() <= EPSILON);

        state.apply_operation(&Operation::LocalUnitary(h)).unwrap();
        assert_states_close(&state, &original);
    }

    #[test]
    fn representative_operation_sequence_preserves_norm() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let mut state = DenseState::zero(spec).unwrap();
        let operations = vec![
            Operation::Fourier { target: 0 },
            Operation::ControlledShift {
                control: 0,
                target: 1,
                shift: 1,
            },
            Operation::WeylZ {
                target: 1,
                power: 2,
            },
            Operation::WeylX {
                target: 0,
                shift: 2,
            },
            Operation::Swap { a: 0, b: 1 },
            Operation::LocalPermutation {
                target: 1,
                map: vec![1, 2, 0],
            },
        ];

        state.apply_operations(&operations).unwrap();
        assert!((state.norm_squared() - 1.0).abs() <= EPSILON);
    }

    #[test]
    fn invalid_batch_is_rejected_before_first_mutation() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let original = DenseState::zero(spec).unwrap();
        let mut state = original.clone();

        let operations = vec![
            Operation::WeylX {
                target: 0,
                shift: 1,
            },
            Operation::Swap { a: 1, b: 1 },
        ];

        let error = state.apply_operations(&operations).unwrap_err();
        assert!(matches!(
            error,
            DenseOperationError::InvalidOperation(
                OperationValidationError::DistinctSubsystemsRequired { .. }
            )
        ));
        assert_eq!(state, original);
    }

    #[test]
    fn rejects_non_unitary_fallback_before_execution() {
        let error = LocalUnitary::new(0, 2, vec![Complex64::new(1.0, 0.0); 4]).unwrap_err();

        assert_eq!(error, LocalUnitaryError::NotUnitary);
    }

    #[test]
    fn observation_snapshot_is_stable_and_exact() {
        let state = DenseState::basis(SystemSpec::new(4, 2).unwrap(), 11).unwrap();

        let first = state.observation_snapshot();
        let second = state.observation_snapshot();

        assert_eq!(first, second);
        assert_eq!(first.representation.id, "dense-reference");
        assert_eq!(first.approximation, ApproximationDeclaration::Exact);
        assert_eq!(first.logical_bytes, 16 * 16);
        assert_eq!(first.norm_squared, 1.0);
        assert!(first.state_digest.len() == 64);
    }

    #[test]
    fn descriptor_declares_reference_and_operation_roles() {
        let descriptor = module_descriptor();
        descriptor.validate().unwrap();

        assert!(descriptor
            .capabilities
            .contains(&Capability::StateRepresentation));
        assert!(descriptor
            .capabilities
            .contains(&Capability::OperationExecution));
        assert!(descriptor.capabilities.contains(&Capability::Oracle));
        assert!(descriptor.consumes.contains(&DataKind::OperationStream));
        assert!(descriptor.produces.contains(&DataKind::StateTransition));
        assert_eq!(descriptor.maturity, Maturity::E2DeterministicFixture);
    }
}

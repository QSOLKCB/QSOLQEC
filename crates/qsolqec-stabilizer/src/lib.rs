//! Exact prime-dimensional stabilizer representation.
//!
//! R4 introduces the first non-dense QSOLQEC state representation. Candidate
//! execution never calls the dense oracle; dense is used only by this crate's
//! dev-tests to validate tractable fixtures.

use core::fmt;

use qsolqec_core::SystemSpec;
use qsolqec_glassbox::{
    ApproximationDeclaration, ObservableState, RepresentationIdentity, SemanticHasher,
    StateSnapshot,
};
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_ops::{Operation, OperationSupport, OperationValidationError};

const STABILIZER_SCHEMA: &[u8] = b"qsolqec.prime-stabilizer-state.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauliGenerator {
    x: Vec<usize>,
    z: Vec<usize>,
    phase: usize,
}

impl PauliGenerator {
    pub fn x(&self) -> &[usize] {
        &self.x
    }

    pub fn z(&self) -> &[usize] {
        &self.z
    }

    pub const fn phase(&self) -> usize {
        self.phase
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimeStabilizerState {
    spec: SystemSpec,
    generators: Vec<PauliGenerator>,
}

impl PrimeStabilizerState {
    /// Construct |0...0> for a prime local dimension.
    pub fn zero(spec: SystemSpec) -> Result<Self, StabilizerError> {
        if !is_prime(spec.dimension()) {
            return Err(StabilizerError::NonPrimeDimension {
                dimension: spec.dimension(),
            });
        }

        let mut generators = Vec::with_capacity(spec.subsystems());
        for stabilized_subsystem in 0..spec.subsystems() {
            let x = vec![0; spec.subsystems()];
            let mut z = vec![0; spec.subsystems()];
            z[stabilized_subsystem] = 1;

            generators.push(PauliGenerator { x, z, phase: 0 });
        }

        Ok(Self { spec, generators })
    }

    /// Construct a computational-basis state without materializing amplitudes.
    pub fn basis(spec: SystemSpec, digits: &[usize]) -> Result<Self, StabilizerError> {
        if digits.len() != spec.subsystems() {
            return Err(StabilizerError::WrongDigitCount {
                expected: spec.subsystems(),
                actual: digits.len(),
            });
        }

        let mut state = Self::zero(spec)?;

        for (target, &digit) in digits.iter().enumerate() {
            if digit >= spec.dimension() {
                return Err(StabilizerError::DigitOutOfRange {
                    target,
                    digit,
                    dimension: spec.dimension(),
                });
            }
            if digit != 0 {
                state.apply_operation(&Operation::WeylX {
                    target,
                    shift: digit,
                })?;
            }
        }

        Ok(state)
    }

    pub const fn spec(&self) -> SystemSpec {
        self.spec
    }

    pub fn generators(&self) -> &[PauliGenerator] {
        &self.generators
    }

    pub fn logical_bytes(&self) -> u128 {
        let scalars_per_generator = (2 * self.spec.subsystems() + 1) as u128;
        self.generators.len() as u128 * scalars_per_generator * std::mem::size_of::<usize>() as u128
    }

    /// Report support without silently falling back to another representation.
    pub fn support_for(&self, operation: &Operation) -> Result<OperationSupport, StabilizerError> {
        operation
            .validate_for(self.spec)
            .map_err(StabilizerError::InvalidOperation)?;

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

    pub fn apply_operation(&mut self, operation: &Operation) -> Result<(), StabilizerError> {
        match self.support_for(operation)? {
            OperationSupport::Exact => self.apply_supported(operation),
            OperationSupport::Approximate => Err(StabilizerError::UnexpectedApproximateSupport),
            OperationSupport::Unsupported => Err(StabilizerError::UnsupportedOperation {
                kind: operation.kind(),
            }),
        }
    }

    /// Validate support for the entire sequence before the first mutation.
    pub fn apply_operations(&mut self, operations: &[Operation]) -> Result<(), StabilizerError> {
        for operation in operations {
            match self.support_for(operation)? {
                OperationSupport::Exact => {}
                OperationSupport::Approximate => {
                    return Err(StabilizerError::UnexpectedApproximateSupport);
                }
                OperationSupport::Unsupported => {
                    return Err(StabilizerError::UnsupportedOperation {
                        kind: operation.kind(),
                    });
                }
            }
        }

        for operation in operations {
            self.apply_supported(operation)?;
        }

        Ok(())
    }

    pub fn generators_commute(&self) -> bool {
        let d = self.spec.dimension();
        for left in 0..self.generators.len() {
            for right in (left + 1)..self.generators.len() {
                if symplectic_product(&self.generators[left], &self.generators[right], d) != 0 {
                    return false;
                }
            }
        }
        true
    }

    fn apply_supported(&mut self, operation: &Operation) -> Result<(), StabilizerError> {
        let d = self.spec.dimension();

        for generator in &mut self.generators {
            match operation {
                Operation::WeylX { target, shift } => {
                    let delta = mul_mod(*shift % d, generator.z[*target], d);
                    generator.phase = sub_mod(generator.phase, delta, d);
                }
                Operation::WeylZ { target, power } => {
                    let delta = mul_mod(*power % d, generator.x[*target], d);
                    generator.phase = add_mod(generator.phase, delta, d);
                }
                Operation::Fourier { target } => {
                    let old_x = generator.x[*target];
                    let old_z = generator.z[*target];
                    generator.phase = sub_mod(generator.phase, mul_mod(old_x, old_z, d), d);
                    generator.x[*target] = neg_mod(old_z, d);
                    generator.z[*target] = old_x;
                }
                Operation::ControlledShift {
                    control,
                    target,
                    shift,
                } => {
                    let shift = *shift % d;
                    let old_control_x = generator.x[*control];
                    let old_target_z = generator.z[*target];

                    generator.x[*target] =
                        add_mod(generator.x[*target], mul_mod(shift, old_control_x, d), d);
                    generator.z[*control] =
                        sub_mod(generator.z[*control], mul_mod(shift, old_target_z, d), d);
                }
                Operation::Swap { a, b } => {
                    generator.x.swap(*a, *b);
                    generator.z.swap(*a, *b);
                }
                Operation::LocalPermutation { .. } | Operation::LocalUnitary(_) => {
                    return Err(StabilizerError::UnsupportedOperation {
                        kind: operation.kind(),
                    });
                }
            }
        }

        debug_assert!(self.generators_commute());
        Ok(())
    }
}

impl ObservableState for PrimeStabilizerState {
    fn observation_snapshot(&self) -> StateSnapshot {
        let mut hasher = SemanticHasher::new();
        hasher.update(STABILIZER_SCHEMA);
        hash_usize(&mut hasher, self.spec.dimension());
        hash_usize(&mut hasher, self.spec.subsystems());
        hash_usize(&mut hasher, self.generators.len());

        for generator in &self.generators {
            hash_usize(&mut hasher, generator.phase);
            for &value in &generator.x {
                hash_usize(&mut hasher, value);
            }
            for &value in &generator.z {
                hash_usize(&mut hasher, value);
            }
        }

        StateSnapshot {
            representation: RepresentationIdentity {
                id: "prime-stabilizer".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            system: self.spec,
            approximation: ApproximationDeclaration::Exact,
            state_digest: hasher.finalize_hex(),
            norm_squared: 1.0,
            logical_bytes: self.logical_bytes(),
        }
    }
}

fn hash_usize(hasher: &mut SemanticHasher, value: usize) {
    hasher.update(&(value as u128).to_be_bytes());
}

fn is_prime(value: usize) -> bool {
    if value < 2 {
        return false;
    }
    if value == 2 {
        return true;
    }
    if value.is_multiple_of(2) {
        return false;
    }

    let mut divisor = 3usize;
    while divisor <= value / divisor {
        if value.is_multiple_of(divisor) {
            return false;
        }
        divisor += 2;
    }
    true
}

fn add_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 + b as u128) % modulus as u128) as usize
}

fn sub_mod(a: usize, b: usize, modulus: usize) -> usize {
    let a = a % modulus;
    let b = b % modulus;
    if a >= b {
        a - b
    } else {
        modulus - (b - a)
    }
}

fn neg_mod(value: usize, modulus: usize) -> usize {
    let value = value % modulus;
    if value == 0 {
        0
    } else {
        modulus - value
    }
}

fn mul_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 * b as u128) % modulus as u128) as usize
}

fn symplectic_product(left: &PauliGenerator, right: &PauliGenerator, modulus: usize) -> usize {
    let mut product = 0usize;
    for index in 0..left.x.len() {
        product = add_mod(
            product,
            mul_mod(left.z[index], right.x[index], modulus),
            modulus,
        );
        product = sub_mod(
            product,
            mul_mod(left.x[index], right.z[index], modulus),
            modulus,
        );
    }
    product
}

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: "prime-stabilizer".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            Capability::StateRepresentation,
            Capability::OperationExecution,
        ],
        consumes: vec![DataKind::QuditState, DataKind::OperationStream],
        produces: vec![DataKind::QuditState, DataKind::StateTransition],
        experimental: true,
        maturity: Maturity::E3OracleCompared,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StabilizerError {
    NonPrimeDimension {
        dimension: usize,
    },
    WrongDigitCount {
        expected: usize,
        actual: usize,
    },
    DigitOutOfRange {
        target: usize,
        digit: usize,
        dimension: usize,
    },
    InvalidOperation(OperationValidationError),
    UnsupportedOperation {
        kind: &'static str,
    },
    UnexpectedApproximateSupport,
}

impl fmt::Display for StabilizerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPrimeDimension { dimension } => write!(
                f,
                "prime stabilizer representation does not support local dimension {dimension}"
            ),
            Self::WrongDigitCount { expected, actual } => {
                write!(f, "expected {expected} basis digits, got {actual}")
            }
            Self::DigitOutOfRange {
                target,
                digit,
                dimension,
            } => write!(
                f,
                "basis digit {digit} at subsystem {target} is outside 0..{dimension}"
            ),
            Self::InvalidOperation(source) => write!(f, "invalid operation: {source}"),
            Self::UnsupportedOperation { kind } => {
                write!(f, "operation {kind} is unsupported by prime stabilizer")
            }
            Self::UnexpectedApproximateSupport => {
                f.write_str("prime stabilizer unexpectedly declared approximate support")
            }
        }
    }
}

impl std::error::Error for StabilizerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidOperation(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    use num_complex::Complex64;
    use qsolqec_dense::DenseState;
    use qsolqec_glassbox::{GlassBox, NumericalContract};

    const EPSILON: f64 = 1.0e-10;

    fn assert_generator_stabilizes_dense(generator: &PauliGenerator, dense: &DenseState) {
        let spec = dense.spec();
        let d = spec.dimension();
        let mut transformed = vec![Complex64::new(0.0, 0.0); dense.amplitudes().len()];

        for (source_index, amplitude) in dense.amplitudes().iter().copied().enumerate() {
            let mut digits = spec.basis_digits(source_index).unwrap();
            let mut exponent = generator.phase;

            for (subsystem, digit) in digits.iter_mut().enumerate() {
                exponent = add_mod(exponent, mul_mod(generator.z[subsystem], *digit, d), d);
                *digit = add_mod(*digit, generator.x[subsystem], d);
            }

            let destination = spec.basis_index(&digits).unwrap();
            let phase = Complex64::from_polar(1.0, TAU * exponent as f64 / d as f64);
            transformed[destination] += amplitude * phase;
        }

        for (index, (actual, expected)) in transformed.iter().zip(dense.amplitudes()).enumerate() {
            assert!(
                (*actual - *expected).norm() <= EPSILON,
                "generator does not stabilize dense amplitude {index}: {actual:?} vs {expected:?}"
            );
        }
    }

    fn assert_oracle_agreement(dimension: usize, operations: &[Operation]) {
        let spec = SystemSpec::new(dimension, 2).unwrap();
        let mut dense = DenseState::zero(spec).unwrap();
        let mut stabilizer = PrimeStabilizerState::zero(spec).unwrap();

        dense.apply_operations(operations).unwrap();
        stabilizer.apply_operations(operations).unwrap();

        assert!(stabilizer.generators_commute());
        for generator in stabilizer.generators() {
            assert_generator_stabilizes_dense(generator, &dense);
        }
    }

    #[test]
    fn rejects_composite_ququart_dimension() {
        assert_eq!(
            PrimeStabilizerState::zero(SystemSpec::new(4, 2).unwrap()),
            Err(StabilizerError::NonPrimeDimension { dimension: 4 })
        );
    }

    #[test]
    fn supports_qubits_qutrits_and_higher_prime_dimensions() {
        for dimension in [2, 3, 5, 7] {
            let state = PrimeStabilizerState::zero(SystemSpec::new(dimension, 2).unwrap()).unwrap();
            assert_eq!(state.generators().len(), 2);
            assert!(state.generators_commute());
        }
    }

    #[test]
    fn reports_support_without_fallback() {
        let state = PrimeStabilizerState::zero(SystemSpec::new(3, 2).unwrap()).unwrap();

        assert_eq!(
            state
                .support_for(&Operation::Fourier { target: 0 })
                .unwrap(),
            OperationSupport::Exact
        );
        assert_eq!(
            state
                .support_for(&Operation::LocalPermutation {
                    target: 0,
                    map: vec![1, 2, 0],
                })
                .unwrap(),
            OperationSupport::Unsupported
        );
    }

    #[test]
    fn unsupported_batch_is_rejected_before_mutation() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let original = PrimeStabilizerState::zero(spec).unwrap();
        let mut state = original.clone();

        let operations = [
            Operation::WeylX {
                target: 0,
                shift: 1,
            },
            Operation::LocalPermutation {
                target: 1,
                map: vec![1, 2, 0],
            },
        ];

        assert_eq!(
            state.apply_operations(&operations),
            Err(StabilizerError::UnsupportedOperation {
                kind: "local-permutation"
            })
        );
        assert_eq!(state, original);
    }

    #[test]
    fn computational_basis_state_matches_dense_oracle() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let stabilizer = PrimeStabilizerState::basis(spec, &[2, 1]).unwrap();
        let dense = DenseState::basis(spec, spec.basis_index(&[2, 1]).unwrap()).unwrap();

        for generator in stabilizer.generators() {
            assert_generator_stabilizes_dense(generator, &dense);
        }
    }

    #[test]
    fn clifford_sequences_match_dense_oracle_for_prime_dimensions() {
        for dimension in [2, 3, 5] {
            let operations = [
                Operation::Fourier { target: 0 },
                Operation::ControlledShift {
                    control: 0,
                    target: 1,
                    shift: 1,
                },
                Operation::WeylZ {
                    target: 1,
                    power: 1,
                },
                Operation::WeylX {
                    target: 0,
                    shift: 1,
                },
                Operation::Swap { a: 0, b: 1 },
                Operation::Fourier { target: 1 },
            ];

            assert_oracle_agreement(dimension, &operations);
        }
    }

    #[test]
    fn tableau_storage_beats_dense_growth_for_twelve_qubits() {
        let spec = SystemSpec::new(2, 12).unwrap();
        let stabilizer = PrimeStabilizerState::zero(spec).unwrap();
        let dense_bytes =
            spec.dense_state_len().unwrap() as u128 * std::mem::size_of::<Complex64>() as u128;

        assert!(stabilizer.logical_bytes() < dense_bytes);
        assert_eq!(
            stabilizer.logical_bytes(),
            12u128 * 25u128 * std::mem::size_of::<usize>() as u128
        );
    }

    #[test]
    fn glassbox_observes_exact_stabilizer_state() {
        let spec = SystemSpec::new(3, 2).unwrap();
        let mut state = PrimeStabilizerState::zero(spec).unwrap();
        let operation = Operation::Fourier { target: 0 };
        let mut glassbox =
            GlassBox::new(NumericalContract::absolute_amplitude_f64(1.0e-12).unwrap());

        let before = state.observation_snapshot();
        let observed = glassbox
            .observe_operation(&mut state, &operation, |state| {
                state.apply_operation(&operation)
            })
            .unwrap();

        observed.result.unwrap();
        assert_eq!(before.approximation, ApproximationDeclaration::Exact);
        assert_eq!(
            observed.receipt.after.snapshot.approximation,
            ApproximationDeclaration::Exact
        );
        assert_ne!(
            observed.receipt.before.snapshot.state_digest,
            observed.receipt.after.snapshot.state_digest
        );
    }

    #[test]
    fn descriptor_records_oracle_compared_maturity() {
        let descriptor = module_descriptor();
        descriptor.validate().unwrap();

        assert!(descriptor
            .capabilities
            .contains(&Capability::StateRepresentation));
        assert!(descriptor
            .capabilities
            .contains(&Capability::OperationExecution));
        assert!(!descriptor.capabilities.contains(&Capability::Oracle));
        assert_eq!(descriptor.maturity, Maturity::E3OracleCompared);
    }
}

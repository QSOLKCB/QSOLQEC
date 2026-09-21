//! Exact dense qudit reference oracle.
//!
//! R1 deliberately implements no gates and no implicit normalization. The
//! dense state is a small-system reference representation against which later
//! modules can be compared.

use core::fmt;

use num_complex::Complex64;
use qsolqec_core::SystemSpec;
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};

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

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: "dense-reference".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![Capability::StateRepresentation, Capability::Oracle],
        consumes: vec![],
        produces: vec![DataKind::QuditState, DataKind::ProbabilityDistribution],
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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
    fn descriptor_declares_reference_role() {
        let descriptor = module_descriptor();
        descriptor.validate().unwrap();

        assert!(descriptor
            .capabilities
            .contains(&Capability::StateRepresentation));
        assert!(descriptor.capabilities.contains(&Capability::Oracle));
        assert_eq!(descriptor.maturity, Maturity::E2DeterministicFixture);
    }
}

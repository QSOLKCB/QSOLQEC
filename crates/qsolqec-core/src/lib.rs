//! Core experiment primitives for QSOLQEC.
//!
//! This crate owns representation-independent experiment geometry. R1 freezes
//! the basis convention used by the dense reference oracle.

use core::fmt;

/// QSOLQEC's frozen basis ordering.
///
/// Subsystem 0 is the least-significant base-d digit. For basis digits
/// `[q0, q1, ..., q(n-1)]`, the flat basis index is:
///
/// `q0 + q1*d + q2*d^2 + ...`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BasisOrder {
    SubsystemZeroLeastSignificant,
}

pub const BASIS_ORDER: BasisOrder = BasisOrder::SubsystemZeroLeastSignificant;

/// A homogeneous system of `subsystems` local systems, each with
/// `dimension` basis states.
///
/// Examples:
/// - Q(2, n): n qubits
/// - Q(3, n): n qutrits
/// - Q(4, n): n ququarts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemSpec {
    dimension: usize,
    subsystems: usize,
}

impl SystemSpec {
    /// Construct a system specification.
    ///
    /// QSOLQEC treats local dimensions below 2 and an empty subsystem set as
    /// invalid experiment specifications.
    pub fn new(dimension: usize, subsystems: usize) -> Result<Self, SystemSpecError> {
        if dimension < 2 {
            return Err(SystemSpecError::DimensionTooSmall { dimension });
        }
        if subsystems == 0 {
            return Err(SystemSpecError::NoSubsystems);
        }

        Ok(Self {
            dimension,
            subsystems,
        })
    }

    pub const fn dimension(self) -> usize {
        self.dimension
    }

    pub const fn subsystems(self) -> usize {
        self.subsystems
    }

    pub const fn basis_order(self) -> BasisOrder {
        BASIS_ORDER
    }

    /// Number of amplitudes required by the naive dense representation.
    ///
    /// Returning `None` is deliberate: a requested experiment can exceed the
    /// addressable size before any allocation is attempted.
    pub fn dense_state_len(self) -> Option<usize> {
        let exponent = u32::try_from(self.subsystems).ok()?;
        self.dimension.checked_pow(exponent)
    }

    /// Convert subsystem basis digits to the frozen flat basis index.
    pub fn basis_index(self, digits: &[usize]) -> Result<usize, BasisIndexError> {
        if digits.len() != self.subsystems {
            return Err(BasisIndexError::WrongDigitCount {
                expected: self.subsystems,
                actual: digits.len(),
            });
        }

        let mut index = 0usize;
        let mut place = 1usize;

        for (subsystem, &digit) in digits.iter().enumerate() {
            if digit >= self.dimension {
                return Err(BasisIndexError::DigitOutOfRange {
                    subsystem,
                    digit,
                    dimension: self.dimension,
                });
            }

            let term = digit
                .checked_mul(place)
                .ok_or(BasisIndexError::ArithmeticOverflow)?;
            index = index
                .checked_add(term)
                .ok_or(BasisIndexError::ArithmeticOverflow)?;

            if subsystem + 1 < self.subsystems {
                place = place
                    .checked_mul(self.dimension)
                    .ok_or(BasisIndexError::ArithmeticOverflow)?;
            }
        }

        Ok(index)
    }

    /// Convert a flat basis index back to subsystem basis digits.
    pub fn basis_digits(self, index: usize) -> Result<Vec<usize>, BasisIndexError> {
        let state_len = self
            .dense_state_len()
            .ok_or(BasisIndexError::StateSizeOverflow)?;

        if index >= state_len {
            return Err(BasisIndexError::IndexOutOfRange { index, state_len });
        }

        let mut remainder = index;
        let mut digits = Vec::with_capacity(self.subsystems);

        for _ in 0..self.subsystems {
            digits.push(remainder % self.dimension);
            remainder /= self.dimension;
        }

        Ok(digits)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemSpecError {
    DimensionTooSmall { dimension: usize },
    NoSubsystems,
}

impl fmt::Display for SystemSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionTooSmall { dimension } => {
                write!(f, "local dimension must be at least 2, got {dimension}")
            }
            Self::NoSubsystems => write!(f, "system must contain at least one subsystem"),
        }
    }
}

impl std::error::Error for SystemSpecError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasisIndexError {
    WrongDigitCount {
        expected: usize,
        actual: usize,
    },
    DigitOutOfRange {
        subsystem: usize,
        digit: usize,
        dimension: usize,
    },
    IndexOutOfRange {
        index: usize,
        state_len: usize,
    },
    StateSizeOverflow,
    ArithmeticOverflow,
}

impl fmt::Display for BasisIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongDigitCount { expected, actual } => {
                write!(f, "expected {expected} basis digits, got {actual}")
            }
            Self::DigitOutOfRange {
                subsystem,
                digit,
                dimension,
            } => write!(
                f,
                "basis digit {digit} at subsystem {subsystem} is outside 0..{dimension}"
            ),
            Self::IndexOutOfRange { index, state_len } => {
                write!(f, "basis index {index} is outside state length {state_len}")
            }
            Self::StateSizeOverflow => {
                f.write_str("dense state size overflows the platform address space")
            }
            Self::ArithmeticOverflow => f.write_str("basis-index arithmetic overflowed"),
        }
    }
}

impl std::error::Error for BasisIndexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ququart_dense_size_is_four_to_n() {
        let spec = SystemSpec::new(4, 5).unwrap();
        assert_eq!(spec.dense_state_len(), Some(1024));
    }

    #[test]
    fn basis_order_is_frozen_to_subsystem_zero_least_significant() {
        let spec = SystemSpec::new(4, 2).unwrap();
        assert_eq!(
            spec.basis_order(),
            BasisOrder::SubsystemZeroLeastSignificant
        );
        assert_eq!(spec.basis_index(&[3, 2]), Ok(11));
        assert_eq!(spec.basis_digits(11), Ok(vec![3, 2]));
    }

    #[test]
    fn qutrit_basis_index_round_trips() {
        let spec = SystemSpec::new(3, 3).unwrap();

        for index in 0..spec.dense_state_len().unwrap() {
            let digits = spec.basis_digits(index).unwrap();
            assert_eq!(spec.basis_index(&digits), Ok(index));
        }
    }

    #[test]
    fn basis_index_rejects_wrong_digit_count() {
        let spec = SystemSpec::new(3, 2).unwrap();
        assert_eq!(
            spec.basis_index(&[1]),
            Err(BasisIndexError::WrongDigitCount {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn basis_index_rejects_out_of_range_digit() {
        let spec = SystemSpec::new(3, 2).unwrap();
        assert_eq!(
            spec.basis_index(&[0, 3]),
            Err(BasisIndexError::DigitOutOfRange {
                subsystem: 1,
                digit: 3,
                dimension: 3
            })
        );
    }

    #[test]
    fn rejects_non_qudit_dimensions() {
        assert_eq!(
            SystemSpec::new(1, 1),
            Err(SystemSpecError::DimensionTooSmall { dimension: 1 })
        );
    }

    #[test]
    fn rejects_empty_system() {
        assert_eq!(SystemSpec::new(2, 0), Err(SystemSpecError::NoSubsystems));
    }

    #[test]
    fn dense_size_fails_closed_on_overflow() {
        let spec = SystemSpec::new(usize::MAX, 2).unwrap();
        assert_eq!(spec.dense_state_len(), None);
        assert_eq!(
            spec.basis_digits(0),
            Err(BasisIndexError::StateSizeOverflow)
        );
    }
}

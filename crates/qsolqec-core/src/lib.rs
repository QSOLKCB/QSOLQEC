//! Core experiment primitives for QSOLQEC.
//!
//! This crate intentionally does not implement quantum-state evolution yet.
//! PR #1 only freezes the generic Q(d,n) system specification.

use core::fmt;

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

    /// Number of amplitudes required by the naive dense representation.
    ///
    /// Returning `None` is deliberate: a requested experiment can exceed the
    /// addressable size before any allocation is attempted.
    pub fn dense_state_len(self) -> Option<usize> {
        let exponent = u32::try_from(self.subsystems).ok()?;
        self.dimension.checked_pow(exponent)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ququart_dense_size_is_four_to_n() {
        let spec = SystemSpec::new(4, 5).unwrap();
        assert_eq!(spec.dense_state_len(), Some(1024));
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
    }
}

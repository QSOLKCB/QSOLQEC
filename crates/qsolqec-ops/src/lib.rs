//! Generalized qudit operation specifications.
//!
//! R2 freezes the operation conventions independently of any optimized compute
//! backend. Dense execution lives in qsolqec-dense; future representations can
//! implement the same operation semantics without depending on dense storage.

use core::fmt;

use num_complex::Complex64;
use qsolqec_core::SystemSpec;

/// Tolerance used only to validate user-supplied local unitary matrices.
///
/// This is not a general experiment comparison tolerance.
pub const LOCAL_UNITARY_TOLERANCE: f64 = 1.0e-12;

/// Generalized qudit operations supported by R2.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Generalized Weyl shift:
    ///
    /// |j> -> |j + shift mod d>
    WeylX { target: usize, shift: usize },

    /// Generalized Weyl phase:
    ///
    /// |j> -> omega^(power*j) |j>
    /// where omega = exp(2*pi*i/d).
    WeylZ { target: usize, power: usize },

    /// Positive-exponent normalized discrete Fourier transform:
    ///
    /// |j> -> 1/sqrt(d) * sum_k omega^(j*k) |k>.
    Fourier { target: usize },

    /// Generalized SUM-style controlled shift:
    ///
    /// |c,t> -> |c, t + c*shift mod d>.
    ControlledShift {
        control: usize,
        target: usize,
        shift: usize,
    },

    /// Swap two local subsystems.
    Swap { a: usize, b: usize },

    /// Permute the local basis of one subsystem.
    ///
    /// map[input_digit] = output_digit.
    LocalPermutation { target: usize, map: Vec<usize> },

    /// Explicit one-subsystem unitary fallback.
    LocalUnitary(LocalUnitary),
}

impl Operation {
    /// Validate an operation against a concrete Q(d,n) system.
    pub fn validate_for(&self, spec: SystemSpec) -> Result<(), OperationValidationError> {
        match self {
            Self::WeylX { target, .. }
            | Self::WeylZ { target, .. }
            | Self::Fourier { target } => validate_target(spec, *target),
            Self::ControlledShift {
                control, target, ..
            } => {
                validate_target(spec, *control)?;
                validate_target(spec, *target)?;
                if control == target {
                    return Err(OperationValidationError::DistinctSubsystemsRequired {
                        first: *control,
                        second: *target,
                    });
                }
                Ok(())
            }
            Self::Swap { a, b } => {
                validate_target(spec, *a)?;
                validate_target(spec, *b)?;
                if a == b {
                    return Err(OperationValidationError::DistinctSubsystemsRequired {
                        first: *a,
                        second: *b,
                    });
                }
                Ok(())
            }
            Self::LocalPermutation { target, map } => {
                validate_target(spec, *target)?;
                validate_permutation(spec.dimension(), map)
            }
            Self::LocalUnitary(unitary) => {
                validate_target(spec, unitary.target)?;
                if unitary.dimension != spec.dimension() {
                    return Err(OperationValidationError::LocalDimensionMismatch {
                        operation_dimension: unitary.dimension,
                        system_dimension: spec.dimension(),
                    });
                }
                Ok(())
            }
        }
    }
}

/// A validated row-major d x d unitary acting on one local subsystem.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalUnitary {
    target: usize,
    dimension: usize,
    matrix: Vec<Complex64>,
}

impl LocalUnitary {
    pub fn new(
        target: usize,
        dimension: usize,
        matrix: Vec<Complex64>,
    ) -> Result<Self, LocalUnitaryError> {
        if dimension < 2 {
            return Err(LocalUnitaryError::DimensionTooSmall { dimension });
        }

        let expected = dimension
            .checked_mul(dimension)
            .ok_or(LocalUnitaryError::MatrixSizeOverflow { dimension })?;

        if matrix.len() != expected {
            return Err(LocalUnitaryError::MatrixLengthMismatch {
                expected,
                actual: matrix.len(),
            });
        }

        for (index, value) in matrix.iter().enumerate() {
            if !value.re.is_finite() || !value.im.is_finite() {
                return Err(LocalUnitaryError::NonFiniteElement { index });
            }
        }

        if !is_unitary(&matrix, dimension, LOCAL_UNITARY_TOLERANCE) {
            return Err(LocalUnitaryError::NotUnitary);
        }

        Ok(Self {
            target,
            dimension,
            matrix,
        })
    }

    pub const fn target(&self) -> usize {
        self.target
    }

    pub const fn dimension(&self) -> usize {
        self.dimension
    }

    pub fn matrix(&self) -> &[Complex64] {
        &self.matrix
    }
}

fn validate_target(spec: SystemSpec, target: usize) -> Result<(), OperationValidationError> {
    if target >= spec.subsystems() {
        return Err(OperationValidationError::TargetOutOfRange {
            target,
            subsystems: spec.subsystems(),
        });
    }
    Ok(())
}

fn validate_permutation(
    dimension: usize,
    map: &[usize],
) -> Result<(), OperationValidationError> {
    if map.len() != dimension {
        return Err(OperationValidationError::PermutationLengthMismatch {
            expected: dimension,
            actual: map.len(),
        });
    }

    let mut seen = vec![false; dimension];
    for (input, &output) in map.iter().enumerate() {
        if output >= dimension {
            return Err(OperationValidationError::PermutationOutputOutOfRange {
                input,
                output,
                dimension,
            });
        }
        if seen[output] {
            return Err(OperationValidationError::PermutationNotBijective { output });
        }
        seen[output] = true;
    }

    Ok(())
}

fn is_unitary(matrix: &[Complex64], dimension: usize, tolerance: f64) -> bool {
    for left_column in 0..dimension {
        for right_column in 0..dimension {
            let mut inner = Complex64::new(0.0, 0.0);

            for row in 0..dimension {
                let left = matrix[row * dimension + left_column];
                let right = matrix[row * dimension + right_column];
                inner += left.conj() * right;
            }

            let expected = if left_column == right_column {
                Complex64::new(1.0, 0.0)
            } else {
                Complex64::new(0.0, 0.0)
            };

            if (inner - expected).norm() > tolerance {
                return false;
            }
        }
    }

    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationValidationError {
    TargetOutOfRange {
        target: usize,
        subsystems: usize,
    },
    DistinctSubsystemsRequired {
        first: usize,
        second: usize,
    },
    LocalDimensionMismatch {
        operation_dimension: usize,
        system_dimension: usize,
    },
    PermutationLengthMismatch {
        expected: usize,
        actual: usize,
    },
    PermutationOutputOutOfRange {
        input: usize,
        output: usize,
        dimension: usize,
    },
    PermutationNotBijective {
        output: usize,
    },
}

impl fmt::Display for OperationValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetOutOfRange { target, subsystems } => write!(
                f,
                "target subsystem {target} is outside subsystem count {subsystems}"
            ),
            Self::DistinctSubsystemsRequired { first, second } => write!(
                f,
                "operation requires distinct subsystems, got {first} and {second}"
            ),
            Self::LocalDimensionMismatch {
                operation_dimension,
                system_dimension,
            } => write!(
                f,
                "local operation dimension {operation_dimension} does not match system dimension {system_dimension}"
            ),
            Self::PermutationLengthMismatch { expected, actual } => {
                write!(f, "expected permutation length {expected}, got {actual}")
            }
            Self::PermutationOutputOutOfRange {
                input,
                output,
                dimension,
            } => write!(
                f,
                "permutation maps input {input} to invalid output {output} for dimension {dimension}"
            ),
            Self::PermutationNotBijective { output } => {
                write!(f, "permutation output {output} is repeated")
            }
        }
    }
}

impl std::error::Error for OperationValidationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalUnitaryError {
    DimensionTooSmall {
        dimension: usize,
    },
    MatrixSizeOverflow {
        dimension: usize,
    },
    MatrixLengthMismatch {
        expected: usize,
        actual: usize,
    },
    NonFiniteElement {
        index: usize,
    },
    NotUnitary,
}

impl fmt::Display for LocalUnitaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionTooSmall { dimension } => {
                write!(f, "local unitary dimension must be at least 2, got {dimension}")
            }
            Self::MatrixSizeOverflow { dimension } => {
                write!(f, "matrix size overflows for local dimension {dimension}")
            }
            Self::MatrixLengthMismatch { expected, actual } => {
                write!(f, "expected {expected} matrix elements, got {actual}")
            }
            Self::NonFiniteElement { index } => {
                write!(f, "matrix element {index} contains a non-finite component")
            }
            Self::NotUnitary => f.write_str("matrix is not unitary within the fixed tolerance"),
        }
    }
}

impl std::error::Error for LocalUnitaryError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_hadamard_as_local_unitary() {
        let scale = 1.0 / 2.0_f64.sqrt();
        let h = vec![
            Complex64::new(scale, 0.0),
            Complex64::new(scale, 0.0),
            Complex64::new(scale, 0.0),
            Complex64::new(-scale, 0.0),
        ];

        let unitary = LocalUnitary::new(0, 2, h).unwrap();
        assert_eq!(unitary.dimension(), 2);
    }

    #[test]
    fn rejects_non_unitary_matrix() {
        let matrix = vec![Complex64::new(1.0, 0.0); 4];
        assert_eq!(
            LocalUnitary::new(0, 2, matrix),
            Err(LocalUnitaryError::NotUnitary)
        );
    }

    #[test]
    fn rejects_duplicate_permutation_outputs() {
        let op = Operation::LocalPermutation {
            target: 0,
            map: vec![0, 0, 2],
        };

        assert_eq!(
            op.validate_for(SystemSpec::new(3, 1).unwrap()),
            Err(OperationValidationError::PermutationNotBijective { output: 0 })
        );
    }

    #[test]
    fn rejects_same_control_and_target() {
        let op = Operation::ControlledShift {
            control: 0,
            target: 0,
            shift: 1,
        };

        assert_eq!(
            op.validate_for(SystemSpec::new(4, 2).unwrap()),
            Err(OperationValidationError::DistinctSubsystemsRequired {
                first: 0,
                second: 0
            })
        );
    }
}

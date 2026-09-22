//! Compile-time module contract for QSOLQEC.
//!
//! The runtime should reason from declared capabilities and typed data kinds,
//! not from module names or repository provenance.

use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    StateRepresentation,
    OperationExecution,
    NoiseModel,
    Measurement,
    Decoder,
    Observer,
    Sonifier,
    Analyzer,
    ComputeBackend,
    Compressor,
    Oracle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataKind {
    ExperimentSpec,
    SystemSpec,
    QuditState,
    OperationStream,
    EncodedState,
    StateTransition,
    ProbabilityDistribution,
    ErrorPattern,
    Syndrome,
    Correction,
    Observation,
    Analysis,
    AudioEvent,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Maturity {
    E0Sketch,
    E1Executes,
    E2DeterministicFixture,
    E3OracleCompared,
    E4Benchmarked,
    E5IndependentlyReplicated,
    E6BridgeCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDescriptor {
    pub id: String,
    pub version: String,
    pub capabilities: Vec<Capability>,
    pub consumes: Vec<DataKind>,
    pub produces: Vec<DataKind>,
    pub experimental: bool,
    pub maturity: Maturity,
}

impl ModuleDescriptor {
    pub fn validate(&self) -> Result<(), DescriptorError> {
        if self.id.trim().is_empty() {
            return Err(DescriptorError::EmptyId);
        }
        if self.version.trim().is_empty() {
            return Err(DescriptorError::EmptyVersion);
        }
        if self.capabilities.is_empty() {
            return Err(DescriptorError::NoCapabilities);
        }
        Ok(())
    }

    pub fn can_consume(&self, kind: DataKind) -> bool {
        self.consumes.contains(&kind)
    }

    pub fn can_produce(&self, kind: DataKind) -> bool {
        self.produces.contains(&kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorError {
    EmptyId,
    EmptyVersion,
    NoCapabilities,
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyId => "module id must not be empty",
            Self::EmptyVersion => "module version must not be empty",
            Self::NoCapabilities => "module must declare at least one capability",
        };
        f.write_str(message)
    }
}

impl std::error::Error for DescriptorError {}

/// Minimal future execution surface.
///
/// PR #1 intentionally does not define configuration serialization, dynamic
/// loading, async execution, or shared runtime state. Those contracts should
/// be added only when a concrete module requires them.
pub trait ResearchModule {
    type Input;
    type Output;
    type Error;

    fn descriptor(&self) -> ModuleDescriptor;

    fn execute(&mut self, input: Self::Input) -> Result<Self::Output, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_rejects_hidden_capability() {
        let descriptor = ModuleDescriptor {
            id: "example".into(),
            version: "0.0.1".into(),
            capabilities: vec![],
            consumes: vec![],
            produces: vec![],
            experimental: true,
            maturity: Maturity::E0Sketch,
        };

        assert_eq!(descriptor.validate(), Err(DescriptorError::NoCapabilities));
    }
}

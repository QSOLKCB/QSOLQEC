//! Experiment-plan validation for QSOLQEC.
//!
//! R0 validates declared module composition. It deliberately does not execute
//! arbitrary modules yet.

use std::collections::{HashMap, HashSet};
use std::fmt;

use qsolqec_module_api::{DataKind, DescriptorError, ModuleDescriptor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleBinding {
    pub instance_id: String,
    pub descriptor: ModuleDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineEdge {
    pub from: String,
    pub to: String,
    pub kind: DataKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExperimentPlan {
    pub modules: Vec<ModuleBinding>,
    pub edges: Vec<PipelineEdge>,
}

impl ExperimentPlan {
    pub fn validate(&self) -> Result<(), PlanError> {
        let mut ids = HashSet::with_capacity(self.modules.len());
        let mut modules = HashMap::with_capacity(self.modules.len());

        for binding in &self.modules {
            if binding.instance_id.trim().is_empty() {
                return Err(PlanError::EmptyInstanceId);
            }
            if !ids.insert(binding.instance_id.as_str()) {
                return Err(PlanError::DuplicateInstanceId(binding.instance_id.clone()));
            }

            binding
                .descriptor
                .validate()
                .map_err(|source| PlanError::InvalidDescriptor {
                    instance_id: binding.instance_id.clone(),
                    source,
                })?;

            modules.insert(binding.instance_id.as_str(), &binding.descriptor);
        }

        for edge in &self.edges {
            let producer = modules
                .get(edge.from.as_str())
                .ok_or_else(|| PlanError::UnknownModule(edge.from.clone()))?;
            let consumer = modules
                .get(edge.to.as_str())
                .ok_or_else(|| PlanError::UnknownModule(edge.to.clone()))?;

            if !producer.can_produce(edge.kind) {
                return Err(PlanError::ProducerDoesNotDeclare {
                    instance_id: edge.from.clone(),
                    kind: edge.kind,
                });
            }

            if !consumer.can_consume(edge.kind) {
                return Err(PlanError::ConsumerDoesNotDeclare {
                    instance_id: edge.to.clone(),
                    kind: edge.kind,
                });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    EmptyInstanceId,
    DuplicateInstanceId(String),
    InvalidDescriptor {
        instance_id: String,
        source: DescriptorError,
    },
    UnknownModule(String),
    ProducerDoesNotDeclare {
        instance_id: String,
        kind: DataKind,
    },
    ConsumerDoesNotDeclare {
        instance_id: String,
        kind: DataKind,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInstanceId => f.write_str("module instance id must not be empty"),
            Self::DuplicateInstanceId(id) => write!(f, "duplicate module instance id: {id}"),
            Self::InvalidDescriptor {
                instance_id,
                source,
            } => write!(f, "invalid descriptor for {instance_id}: {source}"),
            Self::UnknownModule(id) => write!(f, "pipeline edge references unknown module: {id}"),
            Self::ProducerDoesNotDeclare { instance_id, kind } => {
                write!(f, "{instance_id} does not declare output {kind:?}")
            }
            Self::ConsumerDoesNotDeclare { instance_id, kind } => {
                write!(f, "{instance_id} does not declare input {kind:?}")
            }
        }
    }
}

impl std::error::Error for PlanError {}

#[cfg(test)]
mod tests {
    use super::*;
    use qsolqec_module_api::{Capability, Maturity};

    fn source() -> ModuleBinding {
        ModuleBinding {
            instance_id: "state".into(),
            descriptor: ModuleDescriptor {
                id: "dense-reference".into(),
                version: "0.0.1".into(),
                capabilities: vec![Capability::StateRepresentation],
                consumes: vec![],
                produces: vec![DataKind::QuditState],
                experimental: true,
                maturity: Maturity::E0Sketch,
            },
        }
    }

    fn observer() -> ModuleBinding {
        ModuleBinding {
            instance_id: "glassbox".into(),
            descriptor: ModuleDescriptor {
                id: "glassbox".into(),
                version: "0.0.1".into(),
                capabilities: vec![Capability::Observer],
                consumes: vec![DataKind::QuditState],
                produces: vec![DataKind::Observation],
                experimental: true,
                maturity: Maturity::E0Sketch,
            },
        }
    }

    #[test]
    fn accepts_declared_pipeline() {
        let plan = ExperimentPlan {
            modules: vec![source(), observer()],
            edges: vec![PipelineEdge {
                from: "state".into(),
                to: "glassbox".into(),
                kind: DataKind::QuditState,
            }],
        };

        assert_eq!(plan.validate(), Ok(()));
    }

    #[test]
    fn rejects_undeclared_edge_type() {
        let plan = ExperimentPlan {
            modules: vec![source(), observer()],
            edges: vec![PipelineEdge {
                from: "state".into(),
                to: "glassbox".into(),
                kind: DataKind::Syndrome,
            }],
        };

        assert!(matches!(
            plan.validate(),
            Err(PlanError::ProducerDoesNotDeclare { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_instance_ids() {
        let plan = ExperimentPlan {
            modules: vec![source(), source()],
            edges: vec![],
        };

        assert!(matches!(
            plan.validate(),
            Err(PlanError::DuplicateInstanceId(_))
        ));
    }
}

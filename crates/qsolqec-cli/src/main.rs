use qsolqec_core::SystemSpec;
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_runtime::{ExperimentPlan, ModuleBinding, PipelineEdge};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = SystemSpec::new(4, 2)?;

    let plan = ExperimentPlan {
        modules: vec![
            ModuleBinding {
                instance_id: "state".into(),
                descriptor: ModuleDescriptor {
                    id: "dense-reference-placeholder".into(),
                    version: "0.0.1".into(),
                    capabilities: vec![Capability::StateRepresentation],
                    consumes: vec![],
                    produces: vec![DataKind::QuditState],
                    experimental: true,
                    maturity: Maturity::E0Sketch,
                },
            },
            ModuleBinding {
                instance_id: "glassbox".into(),
                descriptor: ModuleDescriptor {
                    id: "glassbox-placeholder".into(),
                    version: "0.0.1".into(),
                    capabilities: vec![Capability::Observer],
                    consumes: vec![DataKind::QuditState],
                    produces: vec![DataKind::Observation],
                    experimental: true,
                    maturity: Maturity::E0Sketch,
                },
            },
        ],
        edges: vec![PipelineEdge {
            from: "state".into(),
            to: "glassbox".into(),
            kind: DataKind::QuditState,
        }],
    };

    plan.validate()?;

    println!(
        "QSOLQEC R0: Q({}, {}) dense-reference-size={} plan=valid",
        system.dimension(),
        system.subsystems(),
        system
            .dense_state_len()
            .map_or_else(|| "overflow".to_owned(), |value| value.to_string())
    );

    Ok(())
}

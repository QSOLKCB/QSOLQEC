use qsolqec_core::SystemSpec;
use qsolqec_dense::{module_descriptor as dense_descriptor, DenseState};
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_runtime::{ExperimentPlan, ModuleBinding, PipelineEdge};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = SystemSpec::new(4, 2)?;
    let basis_digits = [3, 2];
    let basis_index = system.basis_index(&basis_digits)?;
    let state = DenseState::basis(system, basis_index)?;

    let plan = ExperimentPlan {
        modules: vec![
            ModuleBinding {
                instance_id: "state".into(),
                descriptor: dense_descriptor(),
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
        "QSOLQEC R1: Q({}, {}) basis={:?}->{} amplitudes={} norm_squared={} p_basis={} plan=valid",
        system.dimension(),
        system.subsystems(),
        basis_digits,
        basis_index,
        state.amplitudes().len(),
        state.norm_squared(),
        state.basis_probability(basis_index)?
    );

    Ok(())
}

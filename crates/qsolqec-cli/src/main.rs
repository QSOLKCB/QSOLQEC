use qsolqec_core::SystemSpec;
use qsolqec_dense::{module_descriptor as dense_descriptor, DenseState};
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_ops::Operation;
use qsolqec_runtime::{ExperimentPlan, ModuleBinding, PipelineEdge};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = SystemSpec::new(4, 2)?;
    let mut state = DenseState::zero(system)?;

    state.apply_operations(&[
        Operation::Fourier { target: 0 },
        Operation::ControlledShift {
            control: 0,
            target: 1,
            shift: 1,
        },
    ])?;

    let nonzero: Vec<(usize, f64)> = state
        .probabilities()
        .into_iter()
        .enumerate()
        .filter(|(_, probability)| *probability > 1.0e-12)
        .collect();

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
        "QSOLQEC R2: Q({}, {}) F_4(0)+CS(0->1) norm_squared={} nonzero={nonzero:?} plan=valid",
        system.dimension(),
        system.subsystems(),
        state.norm_squared()
    );

    Ok(())
}

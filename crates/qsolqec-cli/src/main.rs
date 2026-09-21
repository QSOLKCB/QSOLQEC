use qsolqec_core::SystemSpec;
use qsolqec_dense::{module_descriptor as dense_descriptor, DenseState};
use qsolqec_glassbox::{module_descriptor as glassbox_descriptor, GlassBox, NumericalContract};
use qsolqec_module_api::DataKind;
use qsolqec_ops::Operation;
use qsolqec_runtime::{ExperimentPlan, ModuleBinding, PipelineEdge};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = SystemSpec::new(4, 2)?;
    let mut state = DenseState::zero(system)?;
    let contract = NumericalContract::absolute_amplitude_f64(1.0e-12)?;
    let mut glassbox = GlassBox::new(contract);

    let operations = [
        Operation::Fourier { target: 0 },
        Operation::ControlledShift {
            control: 0,
            target: 1,
            shift: 1,
        },
    ];

    let mut artifact_ids = Vec::new();
    for operation in &operations {
        let observed = glassbox.observe_operation(&mut state, operation, |state| {
            state.apply_operation(operation)
        })?;
        observed.result?;
        artifact_ids.push(observed.receipt.artifact_id);
    }

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
                descriptor: glassbox_descriptor(),
            },
        ],
        edges: vec![PipelineEdge {
            from: "state".into(),
            to: "glassbox".into(),
            kind: DataKind::StateTransition,
        }],
    };

    plan.validate()?;

    println!(
        "QSOLQEC R3: Q({}, {}) receipts={} norm_squared={} nonzero={nonzero:?} artifacts={artifact_ids:?} plan=valid",
        system.dimension(),
        system.subsystems(),
        artifact_ids.len(),
        state.norm_squared()
    );

    Ok(())
}

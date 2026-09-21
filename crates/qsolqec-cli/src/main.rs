use qsolqec_core::SystemSpec;
use qsolqec_dense::{module_descriptor as dense_descriptor, DenseState};
use qsolqec_glassbox::{
    module_descriptor as glassbox_descriptor, GlassBox, NumericalContract, ObservableState,
};
use qsolqec_module_api::DataKind;
use qsolqec_ops::{Operation, OperationSupport};
use qsolqec_runtime::{ExperimentPlan, ModuleBinding, PipelineEdge};
use qsolqec_stabilizer::{
    module_descriptor as stabilizer_descriptor, PrimeStabilizerState,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = SystemSpec::new(3, 2)?;
    let operations = [
        Operation::Fourier { target: 0 },
        Operation::ControlledShift {
            control: 0,
            target: 1,
            shift: 1,
        },
        Operation::WeylZ {
            target: 1,
            power: 1,
        },
    ];

    let mut dense = DenseState::zero(system)?;
    let mut stabilizer = PrimeStabilizerState::zero(system)?;

    for operation in &operations {
        if stabilizer.support_for(operation)? != OperationSupport::Exact {
            return Err(format!("unexpected non-exact support for {}", operation.kind()).into());
        }
    }

    let contract = NumericalContract::absolute_amplitude_f64(1.0e-12)?;
    let mut dense_box = GlassBox::new(contract);
    let mut stabilizer_box = GlassBox::new(contract);

    let mut dense_artifacts = Vec::new();
    let mut stabilizer_artifacts = Vec::new();

    for operation in &operations {
        let dense_observed = dense_box.observe_operation(&mut dense, operation, |state| {
            state.apply_operation(operation)
        })?;
        dense_observed.result?;
        dense_artifacts.push(dense_observed.receipt.artifact_id);

        let stabilizer_observed =
            stabilizer_box.observe_operation(&mut stabilizer, operation, |state| {
                state.apply_operation(operation)
            })?;
        stabilizer_observed.result?;
        stabilizer_artifacts.push(stabilizer_observed.receipt.artifact_id);
    }

    let dense_snapshot = dense.observation_snapshot();
    let stabilizer_snapshot = stabilizer.observation_snapshot();

    let plan = ExperimentPlan {
        modules: vec![
            ModuleBinding {
                instance_id: "dense".into(),
                descriptor: dense_descriptor(),
            },
            ModuleBinding {
                instance_id: "stabilizer".into(),
                descriptor: stabilizer_descriptor(),
            },
            ModuleBinding {
                instance_id: "glassbox".into(),
                descriptor: glassbox_descriptor(),
            },
        ],
        edges: vec![
            PipelineEdge {
                from: "dense".into(),
                to: "glassbox".into(),
                kind: DataKind::StateTransition,
            },
            PipelineEdge {
                from: "stabilizer".into(),
                to: "glassbox".into(),
                kind: DataKind::StateTransition,
            },
        ],
    };

    plan.validate()?;

    println!(
        "QSOLQEC R4: Q({}, {}) dense_bytes={} stabilizer_bytes={} generators={} dense_artifacts={} stabilizer_artifacts={} plan=valid",
        system.dimension(),
        system.subsystems(),
        dense_snapshot.logical_bytes,
        stabilizer_snapshot.logical_bytes,
        stabilizer.generators().len(),
        dense_artifacts.len(),
        stabilizer_artifacts.len()
    );

    Ok(())
}

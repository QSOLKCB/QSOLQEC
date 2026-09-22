use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor};
use qsolqec_qec::{
    Correction, Decoder, DecoderError, RepetitionCodeSpec, Syndrome,
};

struct ExternalCandidate {
    code: RepetitionCodeSpec,
}

impl ExternalCandidate {
    fn descriptor_value(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "external-candidate".into(),
            version: "integration-v1".into(),
            capabilities: vec![Capability::Decoder],
            consumes: vec![DataKind::Syndrome],
            produces: vec![DataKind::Correction],
            experimental: true,
            maturity: Maturity::E2DeterministicFixture,
        }
    }
}

impl Decoder for ExternalCandidate {
    fn descriptor(&self) -> ModuleDescriptor {
        self.descriptor_value()
    }

    fn code(&self) -> RepetitionCodeSpec {
        self.code
    }

    fn decode(&self, syndrome: &Syndrome) -> Result<Correction, DecoderError> {
        let shifts = vec![0; self.code.length()];
        Correction::for_decoder(
            self.code,
            shifts,
            syndrome,
            &self.descriptor_value(),
        )
    }
}

#[test]
fn downstream_decoder_can_construct_validated_correction() {
    let code = RepetitionCodeSpec::new(2, 3).unwrap();
    let syndrome = Syndrome::from_x_error_shifts(code, &[0, 0, 0]).unwrap();
    let candidate = ExternalCandidate { code };

    let correction = candidate.decode(&syndrome).unwrap();

    assert_eq!(correction.code(), code);
    assert_eq!(correction.x_shifts(), &[0, 0, 0]);
    assert_eq!(correction.source_syndrome_digest(), syndrome.digest());
    assert_eq!(correction.decoder_id(), "external-candidate");
    assert_eq!(correction.decoder_version(), "integration-v1");
}

//! QSOLQEC R10 replayable noise, syndrome, and decoder modules.
//!
//! This crate is deliberately representation-independent. Decoder execution
//! consumes typed syndrome data only; it does not accept Dense, Stabilizer,
//! Fly-Phi664, or any other state representation and therefore has no runtime
//! oracle-rescue path.

use core::fmt;
use std::collections::BTreeMap;

use qsolqec_core::SystemSpec;
use qsolqec_glassbox::SemanticHasher;
use qsolqec_module_api::{Capability, DataKind, Maturity, ModuleDescriptor, ResearchModule};
use qsolqec_ops::Operation;

const NOISE_SPEC_SCHEMA: &[u8] = b"qsolqec.weyl-noise-spec.v1";
const ERROR_PATTERN_SCHEMA: &[u8] = b"qsolqec.error-pattern.v1";
const SYNDROME_SCHEMA: &[u8] = b"qsolqec.repetition-x-syndrome.v1";
const CORRECTION_SCHEMA: &[u8] = b"qsolqec.repetition-x-correction.v1";
const COMPARISON_SCHEMA: &[u8] = b"qsolqec.decoder-comparison.v1";
const PPM_SCALE: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WeylError {
    target: usize,
    x_shift: usize,
    z_power: usize,
}

impl WeylError {
    pub fn new(
        system: SystemSpec,
        target: usize,
        x_shift: usize,
        z_power: usize,
    ) -> Result<Self, QecError> {
        if target >= system.subsystems() {
            return Err(QecError::ErrorTargetOutOfRange {
                target,
                subsystems: system.subsystems(),
            });
        }
        if x_shift >= system.dimension() {
            return Err(QecError::ErrorExponentOutOfRange {
                target,
                field: "x_shift",
                value: x_shift,
                dimension: system.dimension(),
            });
        }
        if z_power >= system.dimension() {
            return Err(QecError::ErrorExponentOutOfRange {
                target,
                field: "z_power",
                value: z_power,
                dimension: system.dimension(),
            });
        }
        if x_shift == 0 && z_power == 0 {
            return Err(QecError::TrivialErrorEvent { target });
        }

        Ok(Self {
            target,
            x_shift,
            z_power,
        })
    }

    pub const fn target(self) -> usize {
        self.target
    }

    pub const fn x_shift(self) -> usize {
        self.x_shift
    }

    pub const fn z_power(self) -> usize {
        self.z_power
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeylNoiseSpec {
    seed: u64,
    x_error_ppm: u32,
    z_error_ppm: u32,
    digest: String,
}

impl WeylNoiseSpec {
    pub fn new(seed: u64, x_error_ppm: u32, z_error_ppm: u32) -> Result<Self, QecError> {
        for (field, value) in [("x_error_ppm", x_error_ppm), ("z_error_ppm", z_error_ppm)] {
            if value > PPM_SCALE {
                return Err(QecError::NoiseRateOutOfRange {
                    field,
                    value,
                    max: PPM_SCALE,
                });
            }
        }

        let mut hasher = SemanticHasher::new();
        hasher.update(NOISE_SPEC_SCHEMA);
        hasher.update(&seed.to_be_bytes());
        hasher.update(&x_error_ppm.to_be_bytes());
        hasher.update(&z_error_ppm.to_be_bytes());

        Ok(Self {
            seed,
            x_error_ppm,
            z_error_ppm,
            digest: hasher.finalize_hex(),
        })
    }

    pub const fn seed(&self) -> u64 {
        self.seed
    }

    pub const fn x_error_ppm(&self) -> u32 {
        self.x_error_ppm
    }

    pub const fn z_error_ppm(&self) -> u32 {
        self.z_error_ppm
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorPattern {
    system: SystemSpec,
    source_noise_digest: String,
    events: Vec<WeylError>,
    digest: String,
}

impl ErrorPattern {
    pub fn from_events(
        system: SystemSpec,
        source_noise_digest: impl Into<String>,
        mut events: Vec<WeylError>,
    ) -> Result<Self, QecError> {
        let source_noise_digest = source_noise_digest.into();
        if source_noise_digest.trim().is_empty() {
            return Err(QecError::EmptySourceNoiseDigest);
        }

        events.sort_by_key(|event| event.target);
        for event in &events {
            WeylError::new(system, event.target, event.x_shift, event.z_power)?;
        }
        for pair in events.windows(2) {
            if pair[0].target == pair[1].target {
                return Err(QecError::DuplicateErrorTarget {
                    target: pair[0].target,
                });
            }
        }

        let digest = error_pattern_digest(system, &source_noise_digest, &events);
        Ok(Self {
            system,
            source_noise_digest,
            events,
            digest,
        })
    }

    pub const fn system(&self) -> SystemSpec {
        self.system
    }

    pub fn source_noise_digest(&self) -> &str {
        &self.source_noise_digest
    }

    pub fn events(&self) -> &[WeylError] {
        &self.events
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Freeze the R10 replay order as X then Z on each subsystem.
    pub fn operation_stream(&self) -> Vec<Operation> {
        let mut operations = Vec::with_capacity(self.events.len().saturating_mul(2));
        for event in &self.events {
            if event.x_shift != 0 {
                operations.push(Operation::WeylX {
                    target: event.target,
                    shift: event.x_shift,
                });
            }
            if event.z_power != 0 {
                operations.push(Operation::WeylZ {
                    target: event.target,
                    power: event.z_power,
                });
            }
        }
        operations
    }
}

fn error_pattern_digest(
    system: SystemSpec,
    source_noise_digest: &str,
    events: &[WeylError],
) -> String {
    let mut hasher = SemanticHasher::new();
    hasher.update(ERROR_PATTERN_SCHEMA);
    hash_system(&mut hasher, system);
    hash_bytes(&mut hasher, source_noise_digest.as_bytes());
    hash_usize(&mut hasher, events.len());
    for event in events {
        hash_usize(&mut hasher, event.target);
        hash_usize(&mut hasher, event.x_shift);
        hash_usize(&mut hasher, event.z_power);
    }
    hasher.finalize_hex()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayableWeylNoise {
    spec: WeylNoiseSpec,
}

impl ReplayableWeylNoise {
    pub fn new(spec: WeylNoiseSpec) -> Self {
        Self { spec }
    }

    pub fn spec(&self) -> &WeylNoiseSpec {
        &self.spec
    }

    pub fn sample(&self, system: SystemSpec) -> Result<ErrorPattern, QecError> {
        let mut rng = SplitMix64::new(self.spec.seed);
        let mut events = Vec::new();
        events
            .try_reserve_exact(system.subsystems())
            .map_err(|_| QecError::AllocationFailed {
                kind: "noise-events",
                elements: system.subsystems(),
            })?;

        let nonzero = system.dimension() - 1;
        let nonzero_u64 =
            u64::try_from(nonzero).expect("supported Rust targets represent usize within u64");
        for target in 0..system.subsystems() {
            // Consume a fixed four words per subsystem. Rate changes therefore
            // do not alter the later RNG position.
            let x_decision = rng.next_u64();
            let x_magnitude = rng.next_u64();
            let z_decision = rng.next_u64();
            let z_magnitude = rng.next_u64();

            let x_shift = if (x_decision % u64::from(PPM_SCALE)) < u64::from(self.spec.x_error_ppm)
            {
                1 + usize::try_from(x_magnitude % nonzero_u64).expect("modulo result fits usize")
            } else {
                0
            };
            let z_power = if (z_decision % u64::from(PPM_SCALE)) < u64::from(self.spec.z_error_ppm)
            {
                1 + usize::try_from(z_magnitude % nonzero_u64).expect("modulo result fits usize")
            } else {
                0
            };

            if x_shift != 0 || z_power != 0 {
                events.push(WeylError::new(system, target, x_shift, z_power)?);
            }
        }

        ErrorPattern::from_events(system, self.spec.digest.clone(), events)
    }
}

impl ResearchModule for ReplayableWeylNoise {
    type Input = SystemSpec;
    type Output = ErrorPattern;
    type Error = QecError;

    fn descriptor(&self) -> ModuleDescriptor {
        noise_module_descriptor()
    }

    fn execute(&mut self, input: Self::Input) -> Result<Self::Output, Self::Error> {
        self.sample(input)
    }
}

#[derive(Debug, Clone, Copy)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepetitionCodeSpec {
    dimension: usize,
    length: usize,
}

impl RepetitionCodeSpec {
    pub fn new(dimension: usize, length: usize) -> Result<Self, QecError> {
        if dimension < 2 {
            return Err(QecError::DimensionTooSmall { dimension });
        }
        if !is_prime(dimension) {
            return Err(QecError::NonPrimeDimension { dimension });
        }
        if length < 3 {
            return Err(QecError::RepetitionLengthTooSmall { length });
        }
        if length.is_multiple_of(2) {
            return Err(QecError::RepetitionLengthMustBeOdd { length });
        }

        Ok(Self { dimension, length })
    }

    pub const fn dimension(self) -> usize {
        self.dimension
    }

    pub const fn length(self) -> usize {
        self.length
    }

    pub const fn distance(self) -> usize {
        self.length
    }

    pub const fn correctable_weight(self) -> usize {
        (self.length - 1) / 2
    }

    pub fn system(self) -> SystemSpec {
        SystemSpec::new(self.dimension, self.length)
            .expect("validated repetition-code parameters form a valid SystemSpec")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syndrome {
    code: RepetitionCodeSpec,
    values: Vec<usize>,
    digest: String,
}

impl Syndrome {
    pub fn new(code: RepetitionCodeSpec, values: Vec<usize>) -> Result<Self, QecError> {
        let expected = code.length - 1;
        if values.len() != expected {
            return Err(QecError::SyndromeLengthMismatch {
                expected,
                actual: values.len(),
            });
        }
        for (index, value) in values.iter().copied().enumerate() {
            if value >= code.dimension {
                return Err(QecError::SyndromeValueOutOfRange {
                    index,
                    value,
                    dimension: code.dimension,
                });
            }
        }

        let digest = syndrome_digest(code, &values);
        Ok(Self {
            code,
            values,
            digest,
        })
    }

    pub fn from_x_error_shifts(
        code: RepetitionCodeSpec,
        x_shifts: &[usize],
    ) -> Result<Self, QecError> {
        if x_shifts.len() != code.length {
            return Err(QecError::ErrorShiftLengthMismatch {
                expected: code.length,
                actual: x_shifts.len(),
            });
        }
        for (target, shift) in x_shifts.iter().copied().enumerate() {
            if shift >= code.dimension {
                return Err(QecError::ErrorExponentOutOfRange {
                    target,
                    field: "x_shift",
                    value: shift,
                    dimension: code.dimension,
                });
            }
        }

        let mut values = Vec::with_capacity(code.length - 1);
        for pair in x_shifts.windows(2) {
            values.push(sub_mod(pair[0], pair[1], code.dimension));
        }
        Self::new(code, values)
    }

    pub fn from_error_pattern(
        code: RepetitionCodeSpec,
        pattern: &ErrorPattern,
    ) -> Result<Self, QecError> {
        if pattern.system != code.system() {
            return Err(QecError::ErrorPatternSystemMismatch {
                expected: code.system(),
                actual: pattern.system,
            });
        }

        let mut shifts = vec![0usize; code.length];
        for event in &pattern.events {
            if event.z_power != 0 {
                return Err(QecError::UnsupportedErrorFamily {
                    target: event.target,
                    z_power: event.z_power,
                });
            }
            shifts[event.target] = event.x_shift;
        }

        Self::from_x_error_shifts(code, &shifts)
    }

    pub const fn code(&self) -> RepetitionCodeSpec {
        self.code
    }

    pub fn values(&self) -> &[usize] {
        &self.values
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

fn syndrome_digest(code: RepetitionCodeSpec, values: &[usize]) -> String {
    let mut hasher = SemanticHasher::new();
    hasher.update(SYNDROME_SCHEMA);
    hash_code(&mut hasher, code);
    hash_usize(&mut hasher, values.len());
    for value in values {
        hash_usize(&mut hasher, *value);
    }
    hasher.finalize_hex()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    code: RepetitionCodeSpec,
    x_shifts: Vec<usize>,
    source_syndrome_digest: String,
    decoder_id: String,
    decoder_version: String,
    digest: String,
}

impl Correction {
    pub fn for_decoder(
        code: RepetitionCodeSpec,
        x_shifts: Vec<usize>,
        syndrome: &Syndrome,
        decoder: &ModuleDescriptor,
    ) -> Result<Self, DecoderError> {
        validate_decoder_descriptor(decoder)?;
        if syndrome.code != code {
            return Err(DecoderError::CodeMismatch {
                expected: code,
                actual: syndrome.code,
            });
        }

        Self::new_bound(
            code,
            x_shifts,
            syndrome,
            decoder.id.as_str(),
            decoder.version.as_str(),
        )
    }

    fn new_bound(
        code: RepetitionCodeSpec,
        x_shifts: Vec<usize>,
        syndrome: &Syndrome,
        decoder_id: &str,
        decoder_version: &str,
    ) -> Result<Self, DecoderError> {
        if x_shifts.len() != code.length {
            return Err(DecoderError::Data(QecError::CorrectionLengthMismatch {
                expected: code.length,
                actual: x_shifts.len(),
            }));
        }
        for (target, shift) in x_shifts.iter().copied().enumerate() {
            if shift >= code.dimension {
                return Err(DecoderError::Data(QecError::ErrorExponentOutOfRange {
                    target,
                    field: "correction_x_shift",
                    value: shift,
                    dimension: code.dimension,
                }));
            }
        }

        let mut hasher = SemanticHasher::new();
        hasher.update(CORRECTION_SCHEMA);
        hash_code(&mut hasher, code);
        hash_bytes(&mut hasher, syndrome.digest.as_bytes());
        hash_bytes(&mut hasher, decoder_id.as_bytes());
        hash_bytes(&mut hasher, decoder_version.as_bytes());
        for shift in &x_shifts {
            hash_usize(&mut hasher, *shift);
        }

        Ok(Self {
            code,
            x_shifts,
            source_syndrome_digest: syndrome.digest.clone(),
            decoder_id: decoder_id.into(),
            decoder_version: decoder_version.into(),
            digest: hasher.finalize_hex(),
        })
    }

    pub const fn code(&self) -> RepetitionCodeSpec {
        self.code
    }

    pub fn x_shifts(&self) -> &[usize] {
        &self.x_shifts
    }

    pub fn source_syndrome_digest(&self) -> &str {
        &self.source_syndrome_digest
    }

    pub fn decoder_id(&self) -> &str {
        &self.decoder_id
    }

    pub fn decoder_version(&self) -> &str {
        &self.decoder_version
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn operation_stream(&self) -> Vec<Operation> {
        self.x_shifts
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(target, shift)| {
                (shift != 0).then_some(Operation::WeylX { target, shift })
            })
            .collect()
    }

    pub fn cancels_x_error(&self, error_shifts: &[usize]) -> Result<bool, QecError> {
        if error_shifts.len() != self.code.length {
            return Err(QecError::ErrorShiftLengthMismatch {
                expected: self.code.length,
                actual: error_shifts.len(),
            });
        }
        for (target, error) in error_shifts.iter().copied().enumerate() {
            if error >= self.code.dimension {
                return Err(QecError::ErrorExponentOutOfRange {
                    target,
                    field: "x_shift",
                    value: error,
                    dimension: self.code.dimension,
                });
            }
        }

        Ok(error_shifts
            .iter()
            .copied()
            .zip(self.x_shifts.iter().copied())
            .all(|(error, correction)| add_mod(error, correction, self.code.dimension) == 0))
    }
}

pub trait Decoder {
    fn descriptor(&self) -> ModuleDescriptor;
    fn code(&self) -> RepetitionCodeSpec;
    fn decode(&self, syndrome: &Syndrome) -> Result<Correction, DecoderError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactRepetitionXDecoder {
    code: RepetitionCodeSpec,
}

impl ExactRepetitionXDecoder {
    pub fn new(code: RepetitionCodeSpec) -> Self {
        Self { code }
    }
}

impl Decoder for ExactRepetitionXDecoder {
    fn descriptor(&self) -> ModuleDescriptor {
        exact_decoder_module_descriptor()
    }

    fn code(&self) -> RepetitionCodeSpec {
        self.code
    }

    fn decode(&self, syndrome: &Syndrome) -> Result<Correction, DecoderError> {
        validate_syndrome_code(self.code, syndrome)?;

        let d = self.code.dimension;
        let mut best: Option<(usize, Vec<usize>)> = None;
        let mut tied = false;

        for first in 0..d {
            let mut error = Vec::with_capacity(self.code.length);
            error.push(first);
            for syndrome_value in &syndrome.values {
                let next = sub_mod(
                    *error.last().expect("error vector is nonempty"),
                    *syndrome_value,
                    d,
                );
                error.push(next);
            }
            let weight = error.iter().filter(|value| **value != 0).count();

            match &best {
                None => {
                    best = Some((weight, error));
                    tied = false;
                }
                Some((best_weight, _)) if weight < *best_weight => {
                    best = Some((weight, error));
                    tied = false;
                }
                Some((best_weight, best_error))
                    if weight == *best_weight && error != *best_error =>
                {
                    tied = true;
                }
                _ => {}
            }
        }

        let (min_weight, error) = best.expect("prime dimension is nonzero");
        if tied {
            return Err(DecoderError::AmbiguousSyndrome { min_weight });
        }
        if min_weight > self.code.correctable_weight() {
            return Err(DecoderError::Uncorrectable {
                min_weight,
                correctable_weight: self.code.correctable_weight(),
            });
        }

        let correction = error
            .into_iter()
            .map(|shift| neg_mod(shift, d))
            .collect::<Vec<_>>();
        Correction::for_decoder(self.code, correction, syndrome, &Decoder::descriptor(self))
    }
}

impl ResearchModule for ExactRepetitionXDecoder {
    type Input = Syndrome;
    type Output = Correction;
    type Error = DecoderError;

    fn descriptor(&self) -> ModuleDescriptor {
        Decoder::descriptor(self)
    }

    fn execute(&mut self, input: Self::Input) -> Result<Self::Output, Self::Error> {
        self.decode(&input)
    }
}

#[derive(Debug, Clone)]
pub struct LookupRepetitionXDecoder {
    code: RepetitionCodeSpec,
    table: BTreeMap<Vec<usize>, Vec<usize>>,
}

impl LookupRepetitionXDecoder {
    pub fn build(code: RepetitionCodeSpec, max_entries: usize) -> Result<Self, DecoderError> {
        let required = correctable_pattern_count(code)?;
        if required > max_entries as u128 {
            return Err(DecoderError::LookupBudgetExceeded {
                required,
                max: max_entries,
            });
        }

        let mut table = BTreeMap::new();
        let mut error = vec![0usize; code.length];
        enumerate_correctable_errors(
            code,
            0,
            code.correctable_weight(),
            &mut error,
            &mut |error| {
                let syndrome =
                    Syndrome::from_x_error_shifts(code, error).map_err(DecoderError::Data)?;
                let correction = error
                    .iter()
                    .copied()
                    .map(|shift| neg_mod(shift, code.dimension))
                    .collect::<Vec<_>>();

                if let Some(existing) = table.insert(syndrome.values.clone(), correction.clone()) {
                    if existing != correction {
                        return Err(DecoderError::LookupCollision {
                            syndrome_digest: syndrome.digest,
                        });
                    }
                }
                Ok(())
            },
        )?;

        Ok(Self { code, table })
    }

    pub fn entry_count(&self) -> usize {
        self.table.len()
    }
}

impl Decoder for LookupRepetitionXDecoder {
    fn descriptor(&self) -> ModuleDescriptor {
        lookup_decoder_module_descriptor()
    }

    fn code(&self) -> RepetitionCodeSpec {
        self.code
    }

    fn decode(&self, syndrome: &Syndrome) -> Result<Correction, DecoderError> {
        validate_syndrome_code(self.code, syndrome)?;
        let correction = self.table.get(&syndrome.values).cloned().ok_or_else(|| {
            DecoderError::MissingLookupEntry {
                syndrome_digest: syndrome.digest.clone(),
            }
        })?;
        Correction::for_decoder(self.code, correction, syndrome, &Decoder::descriptor(self))
    }
}

impl ResearchModule for LookupRepetitionXDecoder {
    type Input = Syndrome;
    type Output = Correction;
    type Error = DecoderError;

    fn descriptor(&self) -> ModuleDescriptor {
        Decoder::descriptor(self)
    }

    fn execute(&mut self, input: Self::Input) -> Result<Self::Output, Self::Error> {
        self.decode(&input)
    }
}

fn validate_syndrome_code(
    code: RepetitionCodeSpec,
    syndrome: &Syndrome,
) -> Result<(), DecoderError> {
    if syndrome.code != code {
        return Err(DecoderError::CodeMismatch {
            expected: code,
            actual: syndrome.code,
        });
    }
    Ok(())
}

fn correctable_pattern_count(code: RepetitionCodeSpec) -> Result<u128, DecoderError> {
    let mut total = 0u128;
    let nonzero = (code.dimension - 1) as u128;

    for weight in 0..=code.correctable_weight() {
        let choose = binomial(code.length, weight).ok_or(DecoderError::PatternCountOverflow)?;
        let exponent = u32::try_from(weight).map_err(|_| DecoderError::PatternCountOverflow)?;
        let assignments = nonzero
            .checked_pow(exponent)
            .ok_or(DecoderError::PatternCountOverflow)?;
        total = total
            .checked_add(
                choose
                    .checked_mul(assignments)
                    .ok_or(DecoderError::PatternCountOverflow)?,
            )
            .ok_or(DecoderError::PatternCountOverflow)?;
    }

    Ok(total)
}

fn binomial(n: usize, k: usize) -> Option<u128> {
    let k = k.min(n.saturating_sub(k));
    let mut result = 1u128;
    for i in 0..k {
        result = result.checked_mul((n - i) as u128)?;
        result /= (i + 1) as u128;
    }
    Some(result)
}

fn enumerate_correctable_errors<F>(
    code: RepetitionCodeSpec,
    index: usize,
    remaining_weight: usize,
    error: &mut [usize],
    callback: &mut F,
) -> Result<(), DecoderError>
where
    F: FnMut(&[usize]) -> Result<(), DecoderError>,
{
    if index == code.length {
        return callback(error);
    }

    error[index] = 0;
    enumerate_correctable_errors(code, index + 1, remaining_weight, error, callback)?;

    if remaining_weight > 0 {
        for shift in 1..code.dimension {
            error[index] = shift;
            enumerate_correctable_errors(code, index + 1, remaining_weight - 1, error, callback)?;
        }
    }
    error[index] = 0;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderComparison {
    pub reference_decoder_id: String,
    pub reference_decoder_version: String,
    pub candidate_decoder_id: String,
    pub candidate_decoder_version: String,
    pub cases: u128,
    pub matched: u128,
    pub mismatched: u128,
    pub reference_failures: u128,
    pub candidate_failures: u128,
    pub first_mismatch_syndrome_digest: Option<String>,
    pub digest: String,
}

fn validate_decoder_descriptor(descriptor: &ModuleDescriptor) -> Result<(), DecoderError> {
    descriptor
        .validate()
        .map_err(|error| DecoderError::InvalidDecoderDescriptor {
            reason: error.to_string(),
        })?;
    if !descriptor.capabilities.contains(&Capability::Decoder) {
        return Err(DecoderError::InvalidDecoderDescriptor {
            reason: format!(
                "module {} does not declare Decoder capability",
                descriptor.id
            ),
        });
    }
    if !descriptor.can_consume(DataKind::Syndrome) {
        return Err(DecoderError::InvalidDecoderDescriptor {
            reason: format!("decoder {} does not consume Syndrome", descriptor.id),
        });
    }
    if !descriptor.can_produce(DataKind::Correction) {
        return Err(DecoderError::InvalidDecoderDescriptor {
            reason: format!("decoder {} does not produce Correction", descriptor.id),
        });
    }
    Ok(())
}

fn validate_correction_binding(
    correction: &Correction,
    syndrome: &Syndrome,
    descriptor: &ModuleDescriptor,
    code: RepetitionCodeSpec,
) -> Result<(), DecoderError> {
    if correction.code != code {
        return Err(DecoderError::CorrectionCodeMismatch {
            expected: code,
            actual: correction.code,
        });
    }
    if correction.source_syndrome_digest != syndrome.digest {
        return Err(DecoderError::CorrectionSyndromeMismatch {
            expected: syndrome.digest.clone(),
            actual: correction.source_syndrome_digest.clone(),
        });
    }
    if correction.decoder_id != descriptor.id || correction.decoder_version != descriptor.version {
        return Err(DecoderError::CorrectionDecoderMismatch {
            expected_id: descriptor.id.clone(),
            expected_version: descriptor.version.clone(),
            actual_id: correction.decoder_id.clone(),
            actual_version: correction.decoder_version.clone(),
        });
    }
    Ok(())
}

pub fn compare_decoders_on_correctable_errors(
    reference: &dyn Decoder,
    candidate: &dyn Decoder,
    max_cases: usize,
) -> Result<DecoderComparison, DecoderError> {
    let code = reference.code();
    if candidate.code() != code {
        return Err(DecoderError::CodeMismatch {
            expected: code,
            actual: candidate.code(),
        });
    }

    let required = correctable_pattern_count(code)?;
    if required > max_cases as u128 {
        return Err(DecoderError::ComparisonBudgetExceeded {
            required,
            max: max_cases,
        });
    }

    let reference_descriptor = reference.descriptor();
    let candidate_descriptor = candidate.descriptor();
    validate_decoder_descriptor(&reference_descriptor)?;
    validate_decoder_descriptor(&candidate_descriptor)?;

    let mut cases = 0u128;
    let mut matched = 0u128;
    let mut mismatched = 0u128;
    let mut reference_failures = 0u128;
    let mut candidate_failures = 0u128;
    let mut first_mismatch_syndrome_digest = None;
    let mut error = vec![0usize; code.length];

    enumerate_correctable_errors(
        code,
        0,
        code.correctable_weight(),
        &mut error,
        &mut |error| {
            cases = cases.saturating_add(1);
            let syndrome =
                Syndrome::from_x_error_shifts(code, error).map_err(DecoderError::Data)?;
            let reference_result = reference.decode(&syndrome);
            let candidate_result = candidate.decode(&syndrome);

            match (reference_result, candidate_result) {
                (Ok(reference_correction), Ok(candidate_correction)) => {
                    let reference_binding = validate_correction_binding(
                        &reference_correction,
                        &syndrome,
                        &reference_descriptor,
                        code,
                    );
                    let candidate_binding = validate_correction_binding(
                        &candidate_correction,
                        &syndrome,
                        &candidate_descriptor,
                        code,
                    );

                    match (reference_binding, candidate_binding) {
                        (Ok(()), Ok(())) => {
                            if reference_correction.x_shifts == candidate_correction.x_shifts {
                                matched = matched.saturating_add(1);
                            } else {
                                mismatched = mismatched.saturating_add(1);
                                if first_mismatch_syndrome_digest.is_none() {
                                    first_mismatch_syndrome_digest = Some(syndrome.digest.clone());
                                }
                            }
                        }
                        (Err(_), Ok(())) => {
                            reference_failures = reference_failures.saturating_add(1);
                            if first_mismatch_syndrome_digest.is_none() {
                                first_mismatch_syndrome_digest = Some(syndrome.digest.clone());
                            }
                        }
                        (Ok(()), Err(_)) => {
                            candidate_failures = candidate_failures.saturating_add(1);
                            if first_mismatch_syndrome_digest.is_none() {
                                first_mismatch_syndrome_digest = Some(syndrome.digest.clone());
                            }
                        }
                        (Err(_), Err(_)) => {
                            reference_failures = reference_failures.saturating_add(1);
                            candidate_failures = candidate_failures.saturating_add(1);
                            if first_mismatch_syndrome_digest.is_none() {
                                first_mismatch_syndrome_digest = Some(syndrome.digest.clone());
                            }
                        }
                    }
                }
                (Err(_), Ok(candidate_correction)) => {
                    reference_failures = reference_failures.saturating_add(1);
                    if validate_correction_binding(
                        &candidate_correction,
                        &syndrome,
                        &candidate_descriptor,
                        code,
                    )
                    .is_err()
                    {
                        candidate_failures = candidate_failures.saturating_add(1);
                    }
                    if first_mismatch_syndrome_digest.is_none() {
                        first_mismatch_syndrome_digest = Some(syndrome.digest.clone());
                    }
                }
                (Ok(reference_correction), Err(_)) => {
                    candidate_failures = candidate_failures.saturating_add(1);
                    if validate_correction_binding(
                        &reference_correction,
                        &syndrome,
                        &reference_descriptor,
                        code,
                    )
                    .is_err()
                    {
                        reference_failures = reference_failures.saturating_add(1);
                    }
                    if first_mismatch_syndrome_digest.is_none() {
                        first_mismatch_syndrome_digest = Some(syndrome.digest.clone());
                    }
                }
                (Err(_), Err(_)) => {
                    reference_failures = reference_failures.saturating_add(1);
                    candidate_failures = candidate_failures.saturating_add(1);
                    if first_mismatch_syndrome_digest.is_none() {
                        first_mismatch_syndrome_digest = Some(syndrome.digest.clone());
                    }
                }
            }

            Ok(())
        },
    )?;

    let mut hasher = SemanticHasher::new();
    hasher.update(COMPARISON_SCHEMA);
    hash_code(&mut hasher, code);
    hash_bytes(&mut hasher, reference_descriptor.id.as_bytes());
    hash_bytes(&mut hasher, reference_descriptor.version.as_bytes());
    hash_bytes(&mut hasher, candidate_descriptor.id.as_bytes());
    hash_bytes(&mut hasher, candidate_descriptor.version.as_bytes());
    hasher.update(&cases.to_be_bytes());
    hasher.update(&matched.to_be_bytes());
    hasher.update(&mismatched.to_be_bytes());
    hasher.update(&reference_failures.to_be_bytes());
    hasher.update(&candidate_failures.to_be_bytes());
    match &first_mismatch_syndrome_digest {
        Some(digest) => {
            hasher.update(&[1]);
            hash_bytes(&mut hasher, digest.as_bytes());
        }
        None => hasher.update(&[0]),
    }

    Ok(DecoderComparison {
        reference_decoder_id: reference_descriptor.id,
        reference_decoder_version: reference_descriptor.version,
        candidate_decoder_id: candidate_descriptor.id,
        candidate_decoder_version: candidate_descriptor.version,
        cases,
        matched,
        mismatched,
        reference_failures,
        candidate_failures,
        first_mismatch_syndrome_digest,
        digest: hasher.finalize_hex(),
    })
}

pub fn noise_module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: "replayable-weyl-noise".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![Capability::NoiseModel],
        consumes: vec![DataKind::SystemSpec],
        produces: vec![DataKind::ErrorPattern, DataKind::OperationStream],
        experimental: true,
        maturity: Maturity::E2DeterministicFixture,
    }
}

pub fn exact_decoder_module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: "repetition-x-exact".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![Capability::Decoder],
        consumes: vec![DataKind::Syndrome],
        produces: vec![DataKind::Correction],
        experimental: true,
        maturity: Maturity::E2DeterministicFixture,
    }
}

pub fn lookup_decoder_module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: "repetition-x-lookup".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![Capability::Decoder],
        consumes: vec![DataKind::Syndrome],
        produces: vec![DataKind::Correction],
        experimental: true,
        maturity: Maturity::E3OracleCompared,
    }
}

fn hash_system(hasher: &mut SemanticHasher, system: SystemSpec) {
    hash_usize(hasher, system.dimension());
    hash_usize(hasher, system.subsystems());
}

fn hash_code(hasher: &mut SemanticHasher, code: RepetitionCodeSpec) {
    hash_usize(hasher, code.dimension);
    hash_usize(hasher, code.length);
}

fn hash_usize(hasher: &mut SemanticHasher, value: usize) {
    hasher.update(&(value as u128).to_be_bytes());
}

fn hash_bytes(hasher: &mut SemanticHasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u128).to_be_bytes());
    hasher.update(bytes);
}

fn is_prime(value: usize) -> bool {
    if value < 2 {
        return false;
    }
    if value == 2 {
        return true;
    }
    if value.is_multiple_of(2) {
        return false;
    }

    let mut divisor = 3usize;
    while divisor <= value / divisor {
        if value.is_multiple_of(divisor) {
            return false;
        }
        divisor += 2;
    }
    true
}

fn add_mod(a: usize, b: usize, modulus: usize) -> usize {
    ((a as u128 + b as u128) % modulus as u128) as usize
}

fn sub_mod(a: usize, b: usize, modulus: usize) -> usize {
    let a = a % modulus;
    let b = b % modulus;
    if a >= b {
        a - b
    } else {
        modulus - (b - a)
    }
}

fn neg_mod(value: usize, modulus: usize) -> usize {
    let value = value % modulus;
    if value == 0 {
        0
    } else {
        modulus - value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QecError {
    DimensionTooSmall {
        dimension: usize,
    },
    NonPrimeDimension {
        dimension: usize,
    },
    RepetitionLengthTooSmall {
        length: usize,
    },
    RepetitionLengthMustBeOdd {
        length: usize,
    },
    NoiseRateOutOfRange {
        field: &'static str,
        value: u32,
        max: u32,
    },
    ErrorTargetOutOfRange {
        target: usize,
        subsystems: usize,
    },
    ErrorExponentOutOfRange {
        target: usize,
        field: &'static str,
        value: usize,
        dimension: usize,
    },
    TrivialErrorEvent {
        target: usize,
    },
    DuplicateErrorTarget {
        target: usize,
    },
    EmptySourceNoiseDigest,
    ErrorPatternSystemMismatch {
        expected: SystemSpec,
        actual: SystemSpec,
    },
    UnsupportedErrorFamily {
        target: usize,
        z_power: usize,
    },
    SyndromeLengthMismatch {
        expected: usize,
        actual: usize,
    },
    SyndromeValueOutOfRange {
        index: usize,
        value: usize,
        dimension: usize,
    },
    ErrorShiftLengthMismatch {
        expected: usize,
        actual: usize,
    },
    CorrectionLengthMismatch {
        expected: usize,
        actual: usize,
    },
    AllocationFailed {
        kind: &'static str,
        elements: usize,
    },
}

impl fmt::Display for QecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionTooSmall { dimension } => {
                write!(f, "local dimension must be at least 2, got {dimension}")
            }
            Self::NonPrimeDimension { dimension } => write!(
                f,
                "R10 repetition-X decoder requires a prime local dimension, got {dimension}"
            ),
            Self::RepetitionLengthTooSmall { length } => {
                write!(f, "repetition code length must be at least 3, got {length}")
            }
            Self::RepetitionLengthMustBeOdd { length } => {
                write!(f, "repetition code length must be odd, got {length}")
            }
            Self::NoiseRateOutOfRange { field, value, max } => {
                write!(f, "{field} must be in 0..={max}, got {value}")
            }
            Self::ErrorTargetOutOfRange { target, subsystems } => {
                write!(f, "error target {target} is outside 0..{subsystems}")
            }
            Self::ErrorExponentOutOfRange {
                target,
                field,
                value,
                dimension,
            } => write!(
                f,
                "{field}={value} at target {target} is outside 0..{dimension}"
            ),
            Self::TrivialErrorEvent { target } => {
                write!(
                    f,
                    "error event at target {target} has zero X and Z exponents"
                )
            }
            Self::DuplicateErrorTarget { target } => {
                write!(f, "error pattern contains duplicate target {target}")
            }
            Self::EmptySourceNoiseDigest => f.write_str("source noise digest must not be empty"),
            Self::ErrorPatternSystemMismatch { expected, actual } => write!(
                f,
                "error pattern system Q({},{}) does not match code Q({},{})",
                actual.dimension(),
                actual.subsystems(),
                expected.dimension(),
                expected.subsystems()
            ),
            Self::UnsupportedErrorFamily { target, z_power } => write!(
                f,
                "repetition-X syndrome does not decode Z-power {z_power} at target {target}"
            ),
            Self::SyndromeLengthMismatch { expected, actual } => {
                write!(f, "expected {expected} syndrome values, got {actual}")
            }
            Self::SyndromeValueOutOfRange {
                index,
                value,
                dimension,
            } => write!(
                f,
                "syndrome value {value} at index {index} is outside 0..{dimension}"
            ),
            Self::ErrorShiftLengthMismatch { expected, actual } => {
                write!(f, "expected {expected} X-error shifts, got {actual}")
            }
            Self::CorrectionLengthMismatch { expected, actual } => {
                write!(f, "expected {expected} correction shifts, got {actual}")
            }
            Self::AllocationFailed { kind, elements } => {
                write!(f, "cannot allocate {kind} for {elements} elements")
            }
        }
    }
}

impl std::error::Error for QecError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoderError {
    Data(QecError),
    InvalidDecoderDescriptor {
        reason: String,
    },
    CorrectionCodeMismatch {
        expected: RepetitionCodeSpec,
        actual: RepetitionCodeSpec,
    },
    CorrectionSyndromeMismatch {
        expected: String,
        actual: String,
    },
    CorrectionDecoderMismatch {
        expected_id: String,
        expected_version: String,
        actual_id: String,
        actual_version: String,
    },
    CodeMismatch {
        expected: RepetitionCodeSpec,
        actual: RepetitionCodeSpec,
    },
    AmbiguousSyndrome {
        min_weight: usize,
    },
    Uncorrectable {
        min_weight: usize,
        correctable_weight: usize,
    },
    LookupBudgetExceeded {
        required: u128,
        max: usize,
    },
    ComparisonBudgetExceeded {
        required: u128,
        max: usize,
    },
    PatternCountOverflow,
    LookupCollision {
        syndrome_digest: String,
    },
    MissingLookupEntry {
        syndrome_digest: String,
    },
}

impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(error) => write!(f, "{error}"),
            Self::InvalidDecoderDescriptor { reason } => {
                write!(f, "invalid decoder descriptor: {reason}")
            }
            Self::CorrectionCodeMismatch { expected, actual } => write!(
                f,
                "correction code mismatch: expected prime-d repetition({}, {}), got ({}, {})",
                expected.dimension,
                expected.length,
                actual.dimension,
                actual.length
            ),
            Self::CorrectionSyndromeMismatch { expected, actual } => write!(
                f,
                "correction syndrome provenance mismatch: expected {expected}, got {actual}"
            ),
            Self::CorrectionDecoderMismatch {
                expected_id,
                expected_version,
                actual_id,
                actual_version,
            } => write!(
                f,
                "correction decoder provenance mismatch: expected {expected_id}@{expected_version}, got {actual_id}@{actual_version}"
            ),
            Self::CodeMismatch { expected, actual } => write!(
                f,
                "syndrome/decoder code mismatch: expected prime-d repetition({}, {}), got ({}, {})",
                expected.dimension,
                expected.length,
                actual.dimension,
                actual.length
            ),
            Self::AmbiguousSyndrome { min_weight } => {
                write!(f, "syndrome has multiple minimum-weight representatives at weight {min_weight}")
            }
            Self::Uncorrectable {
                min_weight,
                correctable_weight,
            } => write!(
                f,
                "minimum representative weight {min_weight} exceeds correctable weight {correctable_weight}"
            ),
            Self::LookupBudgetExceeded { required, max } => {
                write!(f, "lookup decoder requires {required} entries, budget is {max}")
            }
            Self::ComparisonBudgetExceeded { required, max } => {
                write!(f, "decoder comparison requires {required} cases, budget is {max}")
            }
            Self::PatternCountOverflow => f.write_str("correctable-pattern count overflowed"),
            Self::LookupCollision { syndrome_digest } => write!(
                f,
                "correctable lookup construction found a syndrome collision at {syndrome_digest}"
            ),
            Self::MissingLookupEntry { syndrome_digest } => {
                write!(f, "lookup decoder has no correction for syndrome {syndrome_digest}")
            }
        }
    }
}

impl std::error::Error for DecoderError {}

impl From<QecError> for DecoderError {
    fn from(value: QecError) -> Self {
        Self::Data(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_sampling_is_replayable_and_canonical() {
        let system = SystemSpec::new(3, 6).unwrap();
        let spec = WeylNoiseSpec::new(42, 350_000, 275_000).unwrap();
        let module = ReplayableWeylNoise::new(spec.clone());

        let first = module.sample(system).unwrap();
        let second = module.sample(system).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.source_noise_digest(), spec.digest());
        assert_eq!(first.digest(), second.digest());
        assert_eq!(first.operation_stream(), second.operation_stream());
    }

    #[test]
    fn noise_consumes_fixed_rng_words_per_subsystem() {
        let system = SystemSpec::new(5, 4).unwrap();
        let none = ReplayableWeylNoise::new(WeylNoiseSpec::new(7, 0, 0).unwrap())
            .sample(system)
            .unwrap();
        let all = ReplayableWeylNoise::new(WeylNoiseSpec::new(7, PPM_SCALE, PPM_SCALE).unwrap())
            .sample(system)
            .unwrap();

        assert!(none.events().is_empty());
        assert_eq!(all.events().len(), 4);
        assert!(all
            .events()
            .iter()
            .all(|event| event.x_shift() > 0 && event.z_power() > 0));
    }

    #[test]
    fn noise_magnitude_selection_is_platform_width_independent() {
        let system = SystemSpec::new(3, 2).unwrap();
        let spec = WeylNoiseSpec::new(0xFFFF_FFFF_0000_0001, PPM_SCALE, PPM_SCALE).unwrap();
        let pattern = ReplayableWeylNoise::new(spec).sample(system).unwrap();

        assert_eq!(pattern.events().len(), 2);
        assert!(pattern.events().iter().all(|event| {
            (1..system.dimension()).contains(&event.x_shift())
                && (1..system.dimension()).contains(&event.z_power())
        }));
    }

    #[test]
    fn error_pattern_canonicalizes_target_order_and_replay_order() {
        let system = SystemSpec::new(3, 3).unwrap();
        let pattern = ErrorPattern::from_events(
            system,
            "manual-fixture",
            vec![
                WeylError::new(system, 2, 1, 2).unwrap(),
                WeylError::new(system, 0, 2, 0).unwrap(),
            ],
        )
        .unwrap();

        assert_eq!(pattern.events()[0].target(), 0);
        assert_eq!(pattern.events()[1].target(), 2);
        assert_eq!(
            pattern.operation_stream(),
            vec![
                Operation::WeylX {
                    target: 0,
                    shift: 2
                },
                Operation::WeylX {
                    target: 2,
                    shift: 1
                },
                Operation::WeylZ {
                    target: 2,
                    power: 2
                },
            ]
        );
    }

    #[test]
    fn exact_qubit_repetition_decoder_corrects_single_shift() {
        let code = RepetitionCodeSpec::new(2, 3).unwrap();
        let error = vec![0, 1, 0];
        let syndrome = Syndrome::from_x_error_shifts(code, &error).unwrap();
        assert_eq!(syndrome.values(), &[1, 1]);

        let decoder = ExactRepetitionXDecoder::new(code);
        let correction = decoder.decode(&syndrome).unwrap();

        assert_eq!(correction.x_shifts(), &[0, 1, 0]);
        assert!(correction.cancels_x_error(&error).unwrap());
    }

    #[test]
    fn exact_qutrit_repetition_decoder_corrects_two_shifts() {
        let code = RepetitionCodeSpec::new(3, 5).unwrap();
        let error = vec![0, 2, 0, 1, 0];
        let syndrome = Syndrome::from_x_error_shifts(code, &error).unwrap();

        let decoder = ExactRepetitionXDecoder::new(code);
        let correction = decoder.decode(&syndrome).unwrap();

        assert!(correction.cancels_x_error(&error).unwrap());
        assert_eq!(
            correction.operation_stream(),
            vec![
                Operation::WeylX {
                    target: 1,
                    shift: 1
                },
                Operation::WeylX {
                    target: 3,
                    shift: 2
                },
            ]
        );
    }

    #[test]
    fn ambiguous_uncorrectable_syndrome_fails_closed() {
        let code = RepetitionCodeSpec::new(3, 3).unwrap();
        let syndrome = Syndrome::from_x_error_shifts(code, &[0, 1, 2]).unwrap();
        let decoder = ExactRepetitionXDecoder::new(code);

        assert_eq!(
            decoder.decode(&syndrome),
            Err(DecoderError::AmbiguousSyndrome { min_weight: 2 })
        );
    }

    #[test]
    fn syndrome_rejects_z_errors_in_x_only_contract() {
        let code = RepetitionCodeSpec::new(3, 3).unwrap();
        let system = code.system();
        let pattern = ErrorPattern::from_events(
            system,
            "manual-fixture",
            vec![WeylError::new(system, 1, 0, 1).unwrap()],
        )
        .unwrap();

        assert_eq!(
            Syndrome::from_error_pattern(code, &pattern),
            Err(QecError::UnsupportedErrorFamily {
                target: 1,
                z_power: 1
            })
        );
    }

    #[test]
    fn code_contract_rejects_even_and_composite_parameters() {
        assert_eq!(
            RepetitionCodeSpec::new(2, 4),
            Err(QecError::RepetitionLengthMustBeOdd { length: 4 })
        );
        assert_eq!(
            RepetitionCodeSpec::new(4, 3),
            Err(QecError::NonPrimeDimension { dimension: 4 })
        );
    }

    #[test]
    fn lookup_candidate_matches_exact_decoder_on_complete_correctable_corpus() {
        let code = RepetitionCodeSpec::new(3, 5).unwrap();
        let reference = ExactRepetitionXDecoder::new(code);
        let candidate = LookupRepetitionXDecoder::build(code, 10_000).unwrap();

        let report =
            compare_decoders_on_correctable_errors(&reference, &candidate, 10_000).unwrap();

        assert_eq!(report.cases, 51);
        assert_eq!(report.matched, report.cases);
        assert_eq!(report.mismatched, 0);
        assert_eq!(report.reference_failures, 0);
        assert_eq!(report.candidate_failures, 0);
        assert!(report.first_mismatch_syndrome_digest.is_none());
        assert_eq!(candidate.entry_count() as u128, report.cases);
    }

    #[derive(Debug, Clone, Copy)]
    struct DelegatingCandidate {
        code: RepetitionCodeSpec,
    }

    impl Decoder for DelegatingCandidate {
        fn descriptor(&self) -> ModuleDescriptor {
            ModuleDescriptor {
                id: "delegating-candidate".into(),
                version: "test-v1".into(),
                capabilities: vec![Capability::Decoder],
                consumes: vec![DataKind::Syndrome],
                produces: vec![DataKind::Correction],
                experimental: true,
                maturity: Maturity::E2DeterministicFixture,
            }
        }

        fn code(&self) -> RepetitionCodeSpec {
            self.code
        }

        fn decode(&self, syndrome: &Syndrome) -> Result<Correction, DecoderError> {
            ExactRepetitionXDecoder::new(self.code).decode(syndrome)
        }
    }

    #[test]
    fn comparison_rejects_delegated_reference_correction_provenance() {
        let code = RepetitionCodeSpec::new(3, 5).unwrap();
        let reference = ExactRepetitionXDecoder::new(code);
        let candidate = DelegatingCandidate { code };

        let report = compare_decoders_on_correctable_errors(&reference, &candidate, 51).unwrap();

        assert_eq!(report.cases, 51);
        assert_eq!(report.matched, 0);
        assert_eq!(report.mismatched, 0);
        assert_eq!(report.reference_failures, 0);
        assert_eq!(report.candidate_failures, 51);
        assert!(report.first_mismatch_syndrome_digest.is_some());
        assert_eq!(report.candidate_decoder_id, "delegating-candidate");
    }

    #[derive(Debug, Clone)]
    struct VersionedZeroCandidate {
        code: RepetitionCodeSpec,
        id: &'static str,
        version: &'static str,
    }

    impl Decoder for VersionedZeroCandidate {
        fn descriptor(&self) -> ModuleDescriptor {
            ModuleDescriptor {
                id: self.id.into(),
                version: self.version.into(),
                capabilities: vec![Capability::Decoder],
                consumes: vec![DataKind::Syndrome],
                produces: vec![DataKind::Correction],
                experimental: true,
                maturity: Maturity::E2DeterministicFixture,
            }
        }

        fn code(&self) -> RepetitionCodeSpec {
            self.code
        }

        fn decode(&self, syndrome: &Syndrome) -> Result<Correction, DecoderError> {
            Correction::for_decoder(
                self.code,
                vec![0; self.code.length()],
                syndrome,
                &self.descriptor(),
            )
        }
    }

    #[test]
    fn comparison_identity_binds_decoder_versions() {
        let code = RepetitionCodeSpec::new(2, 3).unwrap();
        let reference_v1 = VersionedZeroCandidate {
            code,
            id: "reference",
            version: "v1",
        };
        let candidate_v1 = VersionedZeroCandidate {
            code,
            id: "candidate",
            version: "v1",
        };
        let reference_v2 = VersionedZeroCandidate {
            code,
            id: "reference",
            version: "v2",
        };
        let candidate_v2 = VersionedZeroCandidate {
            code,
            id: "candidate",
            version: "v2",
        };

        let v1 = compare_decoders_on_correctable_errors(&reference_v1, &candidate_v1, 4).unwrap();
        let v2 = compare_decoders_on_correctable_errors(&reference_v2, &candidate_v2, 4).unwrap();

        assert_eq!(v1.reference_decoder_id, "reference");
        assert_eq!(v1.reference_decoder_version, "v1");
        assert_eq!(v1.candidate_decoder_id, "candidate");
        assert_eq!(v1.candidate_decoder_version, "v1");
        assert_eq!(v2.reference_decoder_version, "v2");
        assert_eq!(v2.candidate_decoder_version, "v2");
        assert_eq!(v1.cases, v2.cases);
        assert_eq!(v1.matched, v2.matched);
        assert_ne!(v1.digest, v2.digest);
    }

    #[test]
    fn lookup_and_comparison_budgets_fail_before_enumeration() {
        let code = RepetitionCodeSpec::new(3, 5).unwrap();
        assert_eq!(
            LookupRepetitionXDecoder::build(code, 50).unwrap_err(),
            DecoderError::LookupBudgetExceeded {
                required: 51,
                max: 50
            }
        );

        let reference = ExactRepetitionXDecoder::new(code);
        let candidate = LookupRepetitionXDecoder::build(code, 51).unwrap();
        assert_eq!(
            compare_decoders_on_correctable_errors(&reference, &candidate, 50).unwrap_err(),
            DecoderError::ComparisonBudgetExceeded {
                required: 51,
                max: 50
            }
        );
    }

    #[test]
    fn module_descriptors_declare_r10_boundaries() {
        let noise = noise_module_descriptor();
        noise.validate().unwrap();
        assert_eq!(noise.capabilities, vec![Capability::NoiseModel]);
        assert_eq!(noise.consumes, vec![DataKind::SystemSpec]);
        assert!(noise.can_produce(DataKind::ErrorPattern));

        let exact = exact_decoder_module_descriptor();
        exact.validate().unwrap();
        assert_eq!(exact.capabilities, vec![Capability::Decoder]);
        assert_eq!(exact.consumes, vec![DataKind::Syndrome]);
        assert_eq!(exact.produces, vec![DataKind::Correction]);
        assert!(!exact.can_consume(DataKind::QuditState));
        assert!(!exact.can_consume(DataKind::EncodedState));

        let lookup = lookup_decoder_module_descriptor();
        lookup.validate().unwrap();
        assert_eq!(lookup.maturity, Maturity::E3OracleCompared);
    }
}

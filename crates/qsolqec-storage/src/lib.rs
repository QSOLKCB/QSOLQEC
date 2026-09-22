//! Representation-neutral structured logical-storage contract for QSOLQEC.
//!
//! R6 deliberately keeps storage semantics separate from quantum-state
//! semantics. Nothing in this crate requires a `SystemSpec`, norm, fidelity,
//! operation stream, or `ObservableState`.

use core::fmt;

use sha2::{Digest, Sha256};

pub const STORAGE_OBSERVATION_SCHEMA: &str = "qsolqec.storage.observation.v1";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageGeometryIdentity {
    kind: String,
    version: String,
    digest: String,
}

impl StorageGeometryIdentity {
    pub fn from_canonical_bytes(
        kind: impl Into<String>,
        version: impl Into<String>,
        canonical_bytes: &[u8],
    ) -> Result<Self, StorageContractError> {
        let kind = nonempty(kind.into(), "geometry kind")?;
        let version = nonempty(version.into(), "geometry version")?;
        let digest = identity_digest("geometry", &kind, &version, canonical_bytes);
        Ok(Self {
            kind,
            version,
            digest,
        })
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageSourceIdentity {
    id: String,
    version: String,
    digest: String,
}

impl StorageSourceIdentity {
    pub fn from_canonical_bytes(
        id: impl Into<String>,
        version: impl Into<String>,
        canonical_bytes: &[u8],
    ) -> Result<Self, StorageContractError> {
        let id = nonempty(id.into(), "source id")?;
        let version = nonempty(version.into(), "source version")?;
        let digest = identity_digest("source", &id, &version, canonical_bytes);
        Ok(Self {
            id,
            version,
            digest,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Identity of payload bytes, deliberately separate from logical address
/// identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PayloadIdentity {
    codec: String,
    codec_version: String,
    digest: String,
}

impl PayloadIdentity {
    pub fn from_payload_bytes(
        codec: impl Into<String>,
        codec_version: impl Into<String>,
        payload: &[u8],
    ) -> Result<Self, StorageContractError> {
        let codec = nonempty(codec.into(), "payload codec")?;
        let codec_version = nonempty(codec_version.into(), "payload codec version")?;
        let digest = identity_digest("payload", &codec, &codec_version, payload);
        Ok(Self {
            codec,
            codec_version,
            digest,
        })
    }

    pub fn codec(&self) -> &str {
        &self.codec
    }

    pub fn codec_version(&self) -> &str {
        &self.codec_version
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Geometry-bound logical address identity.
///
/// The same numeric index under a different geometry digest is a different
/// logical address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackedAddress {
    geometry_digest: String,
    index: u128,
}

impl PackedAddress {
    pub fn bind(
        geometry: &StorageGeometryIdentity,
        index: u128,
        logical_address_count: u128,
    ) -> Result<Self, StorageContractError> {
        if logical_address_count == 0 {
            return Err(StorageContractError::EmptyAddressSpace);
        }
        if index >= logical_address_count {
            return Err(StorageContractError::AddressOutOfRange {
                index,
                logical_address_count,
            });
        }
        Ok(Self {
            geometry_digest: geometry.digest.clone(),
            index,
        })
    }

    pub fn geometry_digest(&self) -> &str {
        &self.geometry_digest
    }

    pub const fn index(&self) -> u128 {
        self.index
    }

    pub fn validate_geometry(
        &self,
        geometry: &StorageGeometryIdentity,
    ) -> Result<(), StorageContractError> {
        if self.geometry_digest != geometry.digest {
            return Err(StorageContractError::GeometryMismatch);
        }
        Ok(())
    }
}

/// A geometry-specific deterministic address codec.
///
/// Implementations own the structured address type. R6 requires the packed
/// result to be stable, geometry-bound, checked, and round-trippable; it does
/// not impose the later Fly-Phi664 address shape.
pub trait LogicalAddressCodec {
    type Address: Clone + PartialEq + Eq;

    fn geometry_identity(&self) -> &StorageGeometryIdentity;
    fn logical_address_count(&self) -> u128;

    fn pack(&self, address: &Self::Address) -> Result<PackedAddress, StorageContractError>;

    fn unpack(&self, packed: &PackedAddress) -> Result<Self::Address, StorageContractError>;
}

/// Checked mixed-radix helper for deterministic geometry implementations.
///
/// Axis 0 is the most-significant axis.
pub fn pack_mixed_radix(
    coordinates: &[u128],
    extents: &[u128],
) -> Result<u128, StorageContractError> {
    validate_mixed_radix_shape(coordinates.len(), extents)?;
    let mut index = 0u128;

    for (axis, (&coordinate, &extent)) in coordinates.iter().zip(extents).enumerate() {
        if coordinate >= extent {
            return Err(StorageContractError::CoordinateOutOfRange {
                axis,
                coordinate,
                extent,
            });
        }
        index = index
            .checked_mul(extent)
            .and_then(|value| value.checked_add(coordinate))
            .ok_or(StorageContractError::AddressArithmeticOverflow)?;
    }

    Ok(index)
}

pub fn unpack_mixed_radix(
    index: u128,
    extents: &[u128],
) -> Result<Vec<u128>, StorageContractError> {
    validate_mixed_radix_shape(extents.len(), extents)?;
    let logical_address_count = mixed_radix_len(extents)?;
    if index >= logical_address_count {
        return Err(StorageContractError::AddressOutOfRange {
            index,
            logical_address_count,
        });
    }

    let mut coordinates = vec![0u128; extents.len()];
    let mut remainder = index;
    for axis in (0..extents.len()).rev() {
        let extent = extents[axis];
        coordinates[axis] = remainder % extent;
        remainder /= extent;
    }
    Ok(coordinates)
}

pub fn mixed_radix_len(extents: &[u128]) -> Result<u128, StorageContractError> {
    validate_mixed_radix_shape(extents.len(), extents)?;
    extents.iter().try_fold(1u128, |count, &extent| {
        count
            .checked_mul(extent)
            .ok_or(StorageContractError::AddressArithmeticOverflow)
    })
}

fn validate_mixed_radix_shape(
    coordinate_count: usize,
    extents: &[u128],
) -> Result<(), StorageContractError> {
    if extents.is_empty() {
        return Err(StorageContractError::EmptyGeometry);
    }
    if coordinate_count != extents.len() {
        return Err(StorageContractError::GeometryRankMismatch {
            coordinates: coordinate_count,
            extents: extents.len(),
        });
    }
    if let Some((axis, _)) = extents.iter().enumerate().find(|(_, extent)| **extent == 0) {
        return Err(StorageContractError::ZeroExtent { axis });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializationWindow {
    start: PackedAddress,
    len: u128,
}

impl MaterializationWindow {
    pub fn new(
        start: PackedAddress,
        len: u128,
        logical_address_count: u128,
    ) -> Result<Self, StorageContractError> {
        if len == 0 {
            return Err(StorageContractError::EmptyMaterializationWindow);
        }
        if start.index >= logical_address_count {
            return Err(StorageContractError::AddressOutOfRange {
                index: start.index,
                logical_address_count,
            });
        }
        let end = start
            .index
            .checked_add(len)
            .ok_or(StorageContractError::AddressArithmeticOverflow)?;
        if end > logical_address_count {
            return Err(StorageContractError::MaterializationWindowOutOfRange {
                start: start.index,
                len,
                logical_address_count,
            });
        }
        Ok(Self { start, len })
    }

    pub fn start(&self) -> &PackedAddress {
        &self.start
    }

    pub const fn len(&self) -> u128 {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        false
    }

    pub fn end_exclusive(&self) -> u128 {
        self.start.index + self.len
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicalBacking {
    Sparse,
    Dense,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageExactness {
    Exact,
    Approximate {
        method: String,
        error_contract: String,
    },
}

impl StorageExactness {
    pub fn approximate(
        method: impl Into<String>,
        error_contract: impl Into<String>,
    ) -> Result<Self, StorageContractError> {
        let method = nonempty(method.into(), "approximation method")?;
        let error_contract = nonempty(error_contract.into(), "approximation error contract")?;
        Ok(Self::Approximate {
            method,
            error_contract,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceBoundary {
    format: String,
    version: u32,
}

impl PersistenceBoundary {
    pub fn new(
        format: impl Into<String>,
        version: u32,
    ) -> Result<Self, StorageContractError> {
        let format = nonempty(format.into(), "persistence format")?;
        if version == 0 {
            return Err(StorageContractError::ZeroPersistenceVersion);
        }
        Ok(Self { format, version })
    }

    pub fn format(&self) -> &str {
        &self.format
    }

    pub const fn version(&self) -> u32 {
        self.version
    }
}

/// Deterministic partitioning of logical addresses into bounded tiles and
/// stable owner domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileOwnershipPolicy {
    tile_span: u128,
    owner_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileOwnership {
    pub tile_index: u128,
    pub owner: u32,
}

impl TileOwnershipPolicy {
    pub fn new(tile_span: u128, owner_count: u32) -> Result<Self, StorageContractError> {
        if tile_span == 0 {
            return Err(StorageContractError::ZeroTileSpan);
        }
        if owner_count == 0 {
            return Err(StorageContractError::ZeroOwnerCount);
        }
        Ok(Self {
            tile_span,
            owner_count,
        })
    }

    pub const fn tile_span(self) -> u128 {
        self.tile_span
    }

    pub const fn owner_count(self) -> u32 {
        self.owner_count
    }

    pub fn owner_of(self, address: &PackedAddress) -> TileOwnership {
        let tile_index = address.index / self.tile_span;
        let owner = (tile_index % u128::from(self.owner_count)) as u32;
        TileOwnership { tile_index, owner }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSnapshotFacts {
    pub geometry: StorageGeometryIdentity,
    pub source: StorageSourceIdentity,
    pub logical_address_count: u128,
    pub materialized_address_count: u128,
    pub materialized_payload_bytes: u128,
    pub resident_working_set_bytes: u128,
    pub backing: PhysicalBacking,
    pub exactness: StorageExactness,
    pub persistence: PersistenceBoundary,
    pub storage_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSnapshot {
    facts: StorageSnapshotFacts,
}

impl StorageSnapshot {
    pub fn from_facts(facts: StorageSnapshotFacts) -> Result<Self, StorageContractError> {
        if facts.logical_address_count == 0 {
            return Err(StorageContractError::EmptyAddressSpace);
        }
        if facts.materialized_address_count > facts.logical_address_count {
            return Err(StorageContractError::MaterializedCountExceedsLogical {
                materialized: facts.materialized_address_count,
                logical: facts.logical_address_count,
            });
        }
        if facts.materialized_address_count == 0 && facts.materialized_payload_bytes != 0 {
            return Err(StorageContractError::PayloadWithoutMaterializedAddresses);
        }
        validate_digest(&facts.storage_digest, "storage digest")?;
        Ok(Self { facts })
    }

    pub fn facts(&self) -> &StorageSnapshotFacts {
        &self.facts
    }
}

/// Storage observation surface distinct from Glass Box quantum-state
/// observation.
pub trait ObservableStorage {
    fn storage_observation_snapshot(&self) -> StorageSnapshot;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageObservation {
    pub schema: &'static str,
    pub artifact_id: String,
    pub snapshot: StorageSnapshot,
}

pub fn observe_storage<S: ObservableStorage>(storage: &S) -> StorageObservation {
    let snapshot = storage.storage_observation_snapshot();
    let artifact_id = storage_artifact_id(&snapshot);
    StorageObservation {
        schema: STORAGE_OBSERVATION_SCHEMA,
        artifact_id,
        snapshot,
    }
}

pub fn content_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn storage_artifact_id(snapshot: &StorageSnapshot) -> String {
    let facts = snapshot.facts();
    let mut canonical = CanonicalHasher::new();
    canonical.push_str(STORAGE_OBSERVATION_SCHEMA);
    canonical.push_str(facts.geometry.kind());
    canonical.push_str(facts.geometry.version());
    canonical.push_str(facts.geometry.digest());
    canonical.push_str(facts.source.id());
    canonical.push_str(facts.source.version());
    canonical.push_str(facts.source.digest());
    canonical.push_u128(facts.logical_address_count);
    canonical.push_u128(facts.materialized_address_count);
    canonical.push_u128(facts.materialized_payload_bytes);
    canonical.push_u128(facts.resident_working_set_bytes);
    canonical.push_str(match facts.backing {
        PhysicalBacking::Sparse => "sparse",
        PhysicalBacking::Dense => "dense",
    });
    match &facts.exactness {
        StorageExactness::Exact => canonical.push_str("exact"),
        StorageExactness::Approximate {
            method,
            error_contract,
        } => {
            canonical.push_str("approximate");
            canonical.push_str(method);
            canonical.push_str(error_contract);
        }
    }
    canonical.push_str(facts.persistence.format());
    canonical.push_u32(facts.persistence.version());
    canonical.push_str(&facts.storage_digest);
    canonical.finish()
}

fn identity_digest(domain: &str, id: &str, version: &str, bytes: &[u8]) -> String {
    let mut canonical = CanonicalHasher::new();
    canonical.push_str(domain);
    canonical.push_str(id);
    canonical.push_str(version);
    canonical.push_bytes(bytes);
    canonical.finish()
}

fn nonempty(value: String, field: &'static str) -> Result<String, StorageContractError> {
    if value.trim().is_empty() {
        return Err(StorageContractError::EmptyIdentityField { field });
    }
    Ok(value)
}

fn validate_digest(
    digest: &str,
    field: &'static str,
) -> Result<(), StorageContractError> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(StorageContractError::InvalidDigest { field });
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StorageContractError::InvalidDigest { field });
    }
    Ok(())
}

struct CanonicalHasher {
    inner: Sha256,
}

impl CanonicalHasher {
    fn new() -> Self {
        Self {
            inner: Sha256::new(),
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        self.inner.update((bytes.len() as u128).to_be_bytes());
        self.inner.update(bytes);
    }

    fn push_str(&mut self, value: &str) {
        self.push_bytes(value.as_bytes());
    }

    fn push_u128(&mut self, value: u128) {
        self.inner.update(value.to_be_bytes());
    }

    fn push_u32(&mut self, value: u32) {
        self.inner.update(value.to_be_bytes());
    }

    fn finish(self) -> String {
        format!("sha256:{:x}", self.inner.finalize())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageContractError {
    EmptyIdentityField {
        field: &'static str,
    },
    InvalidDigest {
        field: &'static str,
    },
    EmptyGeometry,
    GeometryRankMismatch {
        coordinates: usize,
        extents: usize,
    },
    ZeroExtent {
        axis: usize,
    },
    CoordinateOutOfRange {
        axis: usize,
        coordinate: u128,
        extent: u128,
    },
    EmptyAddressSpace,
    AddressOutOfRange {
        index: u128,
        logical_address_count: u128,
    },
    AddressArithmeticOverflow,
    GeometryMismatch,
    EmptyMaterializationWindow,
    MaterializationWindowOutOfRange {
        start: u128,
        len: u128,
        logical_address_count: u128,
    },
    ZeroTileSpan,
    ZeroOwnerCount,
    ZeroPersistenceVersion,
    MaterializedCountExceedsLogical {
        materialized: u128,
        logical: u128,
    },
    PayloadWithoutMaterializedAddresses,
}

impl fmt::Display for StorageContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentityField { field } => write!(f, "{field} must not be empty"),
            Self::InvalidDigest { field } => {
                write!(f, "{field} must be a sha256: digest with 64 hex digits")
            }
            Self::EmptyGeometry => f.write_str("storage geometry must contain at least one axis"),
            Self::GeometryRankMismatch {
                coordinates,
                extents,
            } => write!(
                f,
                "geometry rank mismatch: {coordinates} coordinates for {extents} extents"
            ),
            Self::ZeroExtent { axis } => write!(f, "geometry extent at axis {axis} is zero"),
            Self::CoordinateOutOfRange {
                axis,
                coordinate,
                extent,
            } => write!(
                f,
                "coordinate {coordinate} at axis {axis} is outside 0..{extent}"
            ),
            Self::EmptyAddressSpace => f.write_str("logical address space must not be empty"),
            Self::AddressOutOfRange {
                index,
                logical_address_count,
            } => write!(
                f,
                "logical address {index} is outside 0..{logical_address_count}"
            ),
            Self::AddressArithmeticOverflow => {
                f.write_str("logical address arithmetic overflowed; address reuse is forbidden")
            }
            Self::GeometryMismatch => {
                f.write_str("packed address belongs to a different storage geometry")
            }
            Self::EmptyMaterializationWindow => {
                f.write_str("materialization window must contain at least one address")
            }
            Self::MaterializationWindowOutOfRange {
                start,
                len,
                logical_address_count,
            } => write!(
                f,
                "materialization window [{start}, {start}+{len}) exceeds logical address count {logical_address_count}"
            ),
            Self::ZeroTileSpan => f.write_str("tile span must be nonzero"),
            Self::ZeroOwnerCount => f.write_str("owner count must be nonzero"),
            Self::ZeroPersistenceVersion => {
                f.write_str("persistence format version must be nonzero")
            }
            Self::MaterializedCountExceedsLogical {
                materialized,
                logical,
            } => write!(
                f,
                "materialized address count {materialized} exceeds logical address count {logical}"
            ),
            Self::PayloadWithoutMaterializedAddresses => f.write_str(
                "materialized payload bytes require at least one materialized address",
            ),
        }
    }
}

impl std::error::Error for StorageContractError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(version: &str) -> StorageGeometryIdentity {
        StorageGeometryIdentity::from_canonical_bytes(
            "fixture-grid",
            version,
            b"axes=2,3,4;axis0-most-significant",
        )
        .unwrap()
    }

    fn source() -> StorageSourceIdentity {
        StorageSourceIdentity::from_canonical_bytes(
            "synthetic-fixture",
            "1",
            b"source-fixture-v1",
        )
        .unwrap()
    }

    fn snapshot(materialized_address_count: u128) -> StorageSnapshot {
        StorageSnapshot::from_facts(StorageSnapshotFacts {
            geometry: geometry("1"),
            source: source(),
            logical_address_count: 24,
            materialized_address_count,
            materialized_payload_bytes: materialized_address_count * 8,
            resident_working_set_bytes: materialized_address_count * 16,
            backing: PhysicalBacking::Sparse,
            exactness: StorageExactness::Exact,
            persistence: PersistenceBoundary::new("fixture-store", 1).unwrap(),
            storage_digest: content_digest(&materialized_address_count.to_be_bytes()),
        })
        .unwrap()
    }

    struct FixtureStore(StorageSnapshot);

    impl ObservableStorage for FixtureStore {
        fn storage_observation_snapshot(&self) -> StorageSnapshot {
            self.0.clone()
        }
    }

    #[test]
    fn identity_binds_kind_version_and_canonical_bytes() {
        let v1 = geometry("1");
        let v1_again = geometry("1");
        let v2 = geometry("2");
        assert_eq!(v1, v1_again);
        assert_ne!(v1.digest(), v2.digest());
    }

    #[test]
    fn mixed_radix_round_trips_without_wrap() {
        let extents = [2, 3, 4];
        assert_eq!(mixed_radix_len(&extents), Ok(24));
        for index in 0..24 {
            let coordinates = unpack_mixed_radix(index, &extents).unwrap();
            assert_eq!(pack_mixed_radix(&coordinates, &extents), Ok(index));
        }
        assert_eq!(
            pack_mixed_radix(&[1, 3, 0], &extents),
            Err(StorageContractError::CoordinateOutOfRange {
                axis: 1,
                coordinate: 3,
                extent: 3,
            })
        );
    }

    #[test]
    fn address_space_overflow_fails_closed() {
        assert_eq!(
            mixed_radix_len(&[u128::MAX, 2]),
            Err(StorageContractError::AddressArithmeticOverflow)
        );
    }

    #[test]
    fn packed_addresses_are_geometry_bound() {
        let address = PackedAddress::bind(&geometry("1"), 7, 24).unwrap();
        assert!(address.validate_geometry(&geometry("1")).is_ok());
        assert_eq!(
            address.validate_geometry(&geometry("2")),
            Err(StorageContractError::GeometryMismatch)
        );
        assert_eq!(
            PackedAddress::bind(&geometry("1"), 24, 24),
            Err(StorageContractError::AddressOutOfRange {
                index: 24,
                logical_address_count: 24,
            })
        );
    }

    #[test]
    fn materialization_windows_are_bounded() {
        let start = PackedAddress::bind(&geometry("1"), 20, 24).unwrap();
        let window = MaterializationWindow::new(start.clone(), 4, 24).unwrap();
        assert_eq!(window.end_exclusive(), 24);
        assert_eq!(
            MaterializationWindow::new(start, 5, 24),
            Err(StorageContractError::MaterializationWindowOutOfRange {
                start: 20,
                len: 5,
                logical_address_count: 24,
            })
        );
    }

    #[test]
    fn materialization_window_overflow_does_not_wrap() {
        let geometry = geometry("1");
        let start = PackedAddress::bind(&geometry, u128::MAX - 1, u128::MAX).unwrap();
        assert_eq!(
            MaterializationWindow::new(start, 2, u128::MAX),
            Err(StorageContractError::AddressArithmeticOverflow)
        );
    }

    #[test]
    fn ownership_is_deterministic_from_tile_and_owner_count() {
        let policy = TileOwnershipPolicy::new(4, 3).unwrap();
        let address = PackedAddress::bind(&geometry("1"), 17, 24).unwrap();
        assert_eq!(
            policy.owner_of(&address),
            TileOwnership {
                tile_index: 4,
                owner: 1,
            }
        );
    }

    #[test]
    fn payload_identity_is_separate_from_address_identity() {
        let address = PackedAddress::bind(&geometry("1"), 3, 24).unwrap();
        let left = PayloadIdentity::from_payload_bytes("raw", "1", b"left").unwrap();
        let right = PayloadIdentity::from_payload_bytes("raw", "1", b"right").unwrap();

        assert_eq!(address.index(), 3);
        assert_ne!(left.digest(), right.digest());
    }

    #[test]
    fn approximate_storage_requires_named_method_and_error_contract() {
        assert!(StorageExactness::approximate("", "abs<=1e-6").is_err());
        assert!(StorageExactness::approximate("quantized", "").is_err());
        assert!(StorageExactness::approximate("quantized", "abs<=1e-6").is_ok());
    }

    #[test]
    fn snapshot_rejects_impossible_materialization_counts() {
        let result = StorageSnapshot::from_facts(StorageSnapshotFacts {
            geometry: geometry("1"),
            source: source(),
            logical_address_count: 24,
            materialized_address_count: 25,
            materialized_payload_bytes: 25,
            resident_working_set_bytes: 25,
            backing: PhysicalBacking::Dense,
            exactness: StorageExactness::Exact,
            persistence: PersistenceBoundary::new("fixture-store", 1).unwrap(),
            storage_digest: content_digest(b"bad"),
        });
        assert!(matches!(
            result,
            Err(StorageContractError::MaterializedCountExceedsLogical { .. })
        ));
    }

    #[test]
    fn storage_observation_identity_is_deterministic_and_semantic() {
        let first = observe_storage(&FixtureStore(snapshot(2)));
        let first_again = observe_storage(&FixtureStore(snapshot(2)));
        let second = observe_storage(&FixtureStore(snapshot(3)));

        assert_eq!(first.schema, STORAGE_OBSERVATION_SCHEMA);
        assert_eq!(first.artifact_id, first_again.artifact_id);
        assert_ne!(first.artifact_id, second.artifact_id);
    }
}

//! Fly-Phi664 structured-memory prototype for QSOLQEC.
//!
//! R7 is a storage substrate, not a Q(d,n) representation. It binds the exact
//! macro-node identity set supplied by a pinned MaleCNS export to three
//! disjoint local fibres (27 + 125 + 512 = 664 logical positions per node).
//! Quantum state semantics remain deliberately absent.

use core::fmt;
use std::collections::BTreeMap;

use qsolqec_storage::{
    content_digest, LogicalAddressCodec, MaterializationWindow, ObservableStorage, PackedAddress,
    PayloadIdentity, PersistenceBoundary, PhysicalBacking, StorageContractError, StorageExactness,
    StorageGeometryIdentity, StorageSnapshot, StorageSnapshotFacts, StorageSourceIdentity,
};

pub const PHI664_POSITIONS_PER_MACRO_NODE: u128 = 664;
pub const F27_POSITIONS: u128 = 27;
pub const N125_POSITIONS: u128 = 125;
pub const R512_POSITIONS: u128 = 512;
pub const N125_OFFSET: u128 = F27_POSITIONS;
pub const R512_OFFSET: u128 = F27_POSITIONS + N125_POSITIONS;

pub const MALE_CNS_V1_DATASET: &str = "male-cns:v1.0";
pub const MALE_CNS_V1_RELEASE_DATE: &str = "2026-06-08";
pub const MALE_CNS_V1_LICENSE: &str = "CC-BY";
pub const MALE_CNS_V1_NODE_SOURCE: &str =
    "https://storage.googleapis.com/flyem-male-cns/v1.0/connectome-data/flat-connectome/body-annotations-male-cns-v1.0-minconf-0.5.feather";
pub const MALE_CNS_V1_EDGE_SOURCE: &str =
    "https://storage.googleapis.com/flyem-male-cns/v1.0/connectome-data/flat-connectome/connectome-weights-male-cns-v1.0-minconf-0.5.feather";
pub const MALE_CNS_V1_DOWNLOAD_PAGE: &str = "https://male-cns.janelia.org/download/";
pub const MALE_CNS_V1_RELEASE_NOTES: &str = "https://male-cns.janelia.org/release/";

const PHI664_GEOMETRY_VERSION: &str = "1";
const PHI664_PERSISTENCE_FORMAT: &str = "qsolqec.fly-phi664.sparse";
const PHI664_PAYLOAD_CODEC: &str = "qsolqec.fly-phi664.opaque-bytes";
const PHI664_PAYLOAD_CODEC_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroSourceSpec {
    pub dataset_id: String,
    pub release_date: String,
    pub license: String,
    pub node_source_uri: String,
    pub edge_source_uri: String,
    pub node_identity_field: String,
    pub node_projection: String,
    pub edge_interpretation: String,
    pub graph_projection: String,
}

impl MacroSourceSpec {
    pub fn male_cns_v1() -> Self {
        Self {
            dataset_id: MALE_CNS_V1_DATASET.into(),
            release_date: MALE_CNS_V1_RELEASE_DATE.into(),
            license: MALE_CNS_V1_LICENSE.into(),
            node_source_uri: MALE_CNS_V1_NODE_SOURCE.into(),
            edge_source_uri: MALE_CNS_V1_EDGE_SOURCE.into(),
            node_identity_field: "bodyId".into(),
            node_projection:
                "caller-supplied bodyId set from the pinned release; canonical order is ascending bodyId"
                    .into(),
            edge_interpretation:
                "directed body_pre -> body_post; weight is published minconf-0.5 synapse count"
                    .into(),
            graph_projection:
                "R7 binds macro-node identity and source topology provenance only; adjacency is not materialized or executed"
                    .into(),
        }
    }

    fn validate(&self) -> Result<(), Phi664Error> {
        for (field, value) in [
            ("dataset_id", self.dataset_id.as_str()),
            ("release_date", self.release_date.as_str()),
            ("license", self.license.as_str()),
            ("node_source_uri", self.node_source_uri.as_str()),
            ("edge_source_uri", self.edge_source_uri.as_str()),
            ("node_identity_field", self.node_identity_field.as_str()),
            ("node_projection", self.node_projection.as_str()),
            ("edge_interpretation", self.edge_interpretation.as_str()),
            ("graph_projection", self.graph_projection.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(Phi664Error::EmptySourceField { field });
            }
        }
        Ok(())
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [
            self.dataset_id.as_str(),
            self.release_date.as_str(),
            self.license.as_str(),
            self.node_source_uri.as_str(),
            self.edge_source_uri.as_str(),
            self.node_identity_field.as_str(),
            self.node_projection.as_str(),
            self.edge_interpretation.as_str(),
            self.graph_projection.as_str(),
        ] {
            push_bytes(&mut bytes, value.as_bytes());
        }
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroGraphManifest {
    source_spec: MacroSourceSpec,
    macro_node_count: u128,
    body_ids_digest: String,
    source_identity: StorageSourceIdentity,
}

impl MacroGraphManifest {
    fn build(source_spec: MacroSourceSpec, sorted_body_ids: &[u64]) -> Result<Self, Phi664Error> {
        source_spec.validate()?;
        if sorted_body_ids.is_empty() {
            return Err(Phi664Error::EmptyMacrograph);
        }

        let body_id_bytes = canonical_body_id_bytes(sorted_body_ids);
        let body_ids_digest = content_digest(&body_id_bytes);
        let macro_node_count =
            u128::try_from(sorted_body_ids.len()).map_err(|_| Phi664Error::AddressSpaceOverflow)?;

        let mut manifest_bytes = source_spec.canonical_bytes();
        manifest_bytes.extend_from_slice(&macro_node_count.to_be_bytes());
        push_bytes(&mut manifest_bytes, body_ids_digest.as_bytes());

        let source_identity = StorageSourceIdentity::from_canonical_bytes(
            source_spec.dataset_id.clone(),
            source_spec.release_date.clone(),
            &manifest_bytes,
        )?;

        Ok(Self {
            source_spec,
            macro_node_count,
            body_ids_digest,
            source_identity,
        })
    }

    pub fn source_spec(&self) -> &MacroSourceSpec {
        &self.source_spec
    }

    pub const fn macro_node_count(&self) -> u128 {
        self.macro_node_count
    }

    pub fn body_ids_digest(&self) -> &str {
        &self.body_ids_digest
    }

    pub fn source_identity(&self) -> &StorageSourceIdentity {
        &self.source_identity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FibreId {
    F27,
    N125,
    R512,
}

impl FibreId {
    pub const fn side(self) -> u8 {
        match self {
            Self::F27 => 3,
            Self::N125 => 5,
            Self::R512 => 8,
        }
    }

    pub const fn positions(self) -> u128 {
        match self {
            Self::F27 => F27_POSITIONS,
            Self::N125 => N125_POSITIONS,
            Self::R512 => R512_POSITIONS,
        }
    }

    pub const fn offset(self) -> u128 {
        match self {
            Self::F27 => 0,
            Self::N125 => N125_OFFSET,
            Self::R512 => R512_OFFSET,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::F27 => "F27",
            Self::N125 => "N125",
            Self::R512 => "R512",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phi664Address {
    geometry_digest: String,
    macro_node_body_id: u64,
    macro_rank: u128,
    fibre: FibreId,
    x: u8,
    y: u8,
    z: u8,
}

impl Phi664Address {
    pub const fn macro_node_body_id(&self) -> u64 {
        self.macro_node_body_id
    }

    pub const fn fibre(&self) -> FibreId {
        self.fibre
    }

    pub const fn x(&self) -> u8 {
        self.x
    }

    pub const fn y(&self) -> u8 {
        self.y
    }

    pub const fn z(&self) -> u8 {
        self.z
    }
}

#[derive(Debug, Clone)]
pub struct Phi664Codec {
    manifest: MacroGraphManifest,
    sorted_body_ids: Vec<u64>,
    geometry: StorageGeometryIdentity,
}

pub fn canonical_body_ids_digest(mut body_ids: Vec<u64>) -> Result<String, Phi664Error> {
    if body_ids.is_empty() {
        return Err(Phi664Error::EmptyMacrograph);
    }
    body_ids.sort_unstable();
    for pair in body_ids.windows(2) {
        if pair[0] == pair[1] {
            return Err(Phi664Error::DuplicateBodyId { body_id: pair[0] });
        }
    }
    Ok(content_digest(&canonical_body_id_bytes(&body_ids)))
}

impl Phi664Codec {
    pub fn from_body_ids(
        source_spec: MacroSourceSpec,
        mut body_ids: Vec<u64>,
    ) -> Result<Self, Phi664Error> {
        if body_ids.is_empty() {
            return Err(Phi664Error::EmptyMacrograph);
        }

        body_ids.sort_unstable();
        for pair in body_ids.windows(2) {
            if pair[0] == pair[1] {
                return Err(Phi664Error::DuplicateBodyId { body_id: pair[0] });
            }
        }
        body_ids.shrink_to_fit();

        let manifest = MacroGraphManifest::build(source_spec, &body_ids)?;
        let logical_address_count = manifest
            .macro_node_count()
            .checked_mul(PHI664_POSITIONS_PER_MACRO_NODE)
            .ok_or(Phi664Error::AddressSpaceOverflow)?;

        let mut geometry_bytes = Vec::new();
        push_bytes(
            &mut geometry_bytes,
            manifest.source_identity().digest().as_bytes(),
        );
        for value in [
            "macro-order=ascending-bodyId",
            "phi664=F27-disjoint-union-N125-disjoint-union-R512",
            "F27=3x3x3",
            "N125=5x5x5",
            "R512=8x8x8",
            "local-order=x-major,y,z",
        ] {
            push_bytes(&mut geometry_bytes, value.as_bytes());
        }

        let geometry = StorageGeometryIdentity::from_canonical_bytes(
            "fly-phi664",
            PHI664_GEOMETRY_VERSION,
            logical_address_count,
            &geometry_bytes,
        )?;

        Ok(Self {
            manifest,
            sorted_body_ids: body_ids,
            geometry,
        })
    }

    pub fn manifest(&self) -> &MacroGraphManifest {
        &self.manifest
    }

    pub fn geometry(&self) -> &StorageGeometryIdentity {
        &self.geometry
    }

    pub fn macro_body_ids(&self) -> &[u64] {
        &self.sorted_body_ids
    }

    pub fn address(
        &self,
        macro_node_body_id: u64,
        fibre: FibreId,
        x: u8,
        y: u8,
        z: u8,
    ) -> Result<Phi664Address, Phi664Error> {
        let rank = self
            .sorted_body_ids
            .binary_search(&macro_node_body_id)
            .map_err(|_| Phi664Error::UnknownBodyId {
                body_id: macro_node_body_id,
            })?;
        validate_coordinates(fibre, x, y, z)?;
        Ok(Phi664Address {
            geometry_digest: self.geometry.digest().into(),
            macro_node_body_id,
            macro_rank: u128::try_from(rank).map_err(|_| Phi664Error::AddressSpaceOverflow)?,
            fibre,
            x,
            y,
            z,
        })
    }

    pub fn packed_index(&self, address: &Phi664Address) -> Result<u128, StorageContractError> {
        Ok(self.pack(address)?.index())
    }

    fn local_index(address: &Phi664Address) -> Result<u128, StorageContractError> {
        if address.geometry_digest.is_empty() {
            return Err(StorageContractError::GeometryMismatch);
        }
        let side = u128::from(address.fibre.side());
        let x = u128::from(address.x);
        let y = u128::from(address.y);
        let z = u128::from(address.z);
        if x >= side || y >= side || z >= side {
            return Err(StorageContractError::AddressOutOfRange {
                index: x.max(y).max(z),
                logical_address_count: side,
            });
        }
        x.checked_mul(side)
            .and_then(|value| value.checked_mul(side))
            .and_then(|value| value.checked_add(y * side))
            .and_then(|value| value.checked_add(z))
            .and_then(|value| value.checked_add(address.fibre.offset()))
            .ok_or(StorageContractError::AddressArithmeticOverflow)
    }

    fn decode_local(local: u128) -> (FibreId, u128) {
        if local < N125_OFFSET {
            (FibreId::F27, local)
        } else if local < R512_OFFSET {
            (FibreId::N125, local - N125_OFFSET)
        } else {
            (FibreId::R512, local - R512_OFFSET)
        }
    }
}

impl LogicalAddressCodec for Phi664Codec {
    type Address = Phi664Address;

    fn geometry_identity(&self) -> &StorageGeometryIdentity {
        &self.geometry
    }

    fn pack(&self, address: &Self::Address) -> Result<PackedAddress, StorageContractError> {
        if address.geometry_digest != self.geometry.digest() {
            return Err(StorageContractError::GeometryMismatch);
        }

        let local = Self::local_index(address)?;
        let index = address
            .macro_rank
            .checked_mul(PHI664_POSITIONS_PER_MACRO_NODE)
            .and_then(|value| value.checked_add(local))
            .ok_or(StorageContractError::AddressArithmeticOverflow)?;
        PackedAddress::bind(&self.geometry, index)
    }

    fn unpack(&self, packed: &PackedAddress) -> Result<Self::Address, StorageContractError> {
        packed.validate_geometry(&self.geometry)?;
        let rank = packed.index() / PHI664_POSITIONS_PER_MACRO_NODE;
        let local = packed.index() % PHI664_POSITIONS_PER_MACRO_NODE;
        let rank_usize =
            usize::try_from(rank).map_err(|_| StorageContractError::AddressArithmeticOverflow)?;
        let body_id = *self.sorted_body_ids.get(rank_usize).ok_or(
            StorageContractError::AddressOutOfRange {
                index: rank,
                logical_address_count: self.manifest.macro_node_count(),
            },
        )?;

        let (fibre, fibre_local) = Self::decode_local(local);
        let side = u128::from(fibre.side());
        let plane = side * side;
        let x = fibre_local / plane;
        let remainder = fibre_local % plane;
        let y = remainder / side;
        let z = remainder % side;

        Ok(Phi664Address {
            geometry_digest: self.geometry.digest().into(),
            macro_node_body_id: body_id,
            macro_rank: rank,
            fibre,
            x: u8::try_from(x).map_err(|_| StorageContractError::AddressArithmeticOverflow)?,
            y: u8::try_from(y).map_err(|_| StorageContractError::AddressArithmeticOverflow)?,
            z: u8::try_from(z).map_err(|_| StorageContractError::AddressArithmeticOverflow)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedCell {
    pub packed_index: u128,
    pub address: Phi664Address,
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Phi664Store {
    codec: Phi664Codec,
    payloads: BTreeMap<u128, Vec<u8>>,
    max_window_addresses: u128,
}

impl Phi664Store {
    pub fn new(codec: Phi664Codec, max_window_addresses: u128) -> Result<Self, Phi664Error> {
        if max_window_addresses == 0 {
            return Err(Phi664Error::ZeroWindowLimit);
        }
        if max_window_addresses > usize::MAX as u128 {
            return Err(Phi664Error::WindowLimitTooLarge {
                max_window_addresses,
            });
        }
        Ok(Self {
            codec,
            payloads: BTreeMap::new(),
            max_window_addresses,
        })
    }

    pub fn codec(&self) -> &Phi664Codec {
        &self.codec
    }

    pub const fn max_window_addresses(&self) -> u128 {
        self.max_window_addresses
    }

    pub fn write_payload(
        &mut self,
        address: &Phi664Address,
        payload: Vec<u8>,
    ) -> Result<PayloadIdentity, Phi664Error> {
        let packed = self.codec.pack(address)?;
        let identity = PayloadIdentity::from_payload_bytes(
            PHI664_PAYLOAD_CODEC,
            PHI664_PAYLOAD_CODEC_VERSION,
            &payload,
        )?;
        self.payloads.insert(packed.index(), payload);
        Ok(identity)
    }

    pub fn remove_payload(
        &mut self,
        address: &Phi664Address,
    ) -> Result<Option<Vec<u8>>, Phi664Error> {
        let packed = self.codec.pack(address)?;
        Ok(self.payloads.remove(&packed.index()))
    }

    pub fn payload(&self, address: &Phi664Address) -> Result<Option<&[u8]>, Phi664Error> {
        let packed = self.codec.pack(address)?;
        Ok(self.payloads.get(&packed.index()).map(Vec::as_slice))
    }

    pub fn materialize_window(
        &self,
        start: &Phi664Address,
        len: u128,
    ) -> Result<Vec<MaterializedCell>, Phi664Error> {
        if len > self.max_window_addresses {
            return Err(Phi664Error::WindowTooLarge {
                requested: len,
                max: self.max_window_addresses,
            });
        }

        let packed_start = self.codec.pack(start)?;
        let window = MaterializationWindow::new(self.codec.geometry(), packed_start, len)?;
        let capacity = usize::try_from(len).map_err(|_| Phi664Error::AddressSpaceOverflow)?;
        let mut cells = Vec::with_capacity(capacity);

        for index in window.start().index()..window.end_exclusive() {
            let packed = PackedAddress::bind(self.codec.geometry(), index)?;
            let address = self.codec.unpack(&packed)?;
            cells.push(MaterializedCell {
                packed_index: index,
                address,
                payload: self.payloads.get(&index).cloned(),
            });
        }
        Ok(cells)
    }

    pub fn materialized_address_count(&self) -> u128 {
        self.payloads.len() as u128
    }

    pub fn materialized_payload_bytes(&self) -> u128 {
        self.payloads
            .values()
            .map(|payload| payload.len() as u128)
            .sum()
    }

    /// Deterministic representation-owned lower bound.
    ///
    /// This counts the frozen u64 body-ID index plus materialized payload bytes.
    /// It deliberately excludes allocator/tree overhead and must not be
    /// substituted for process RSS.
    pub fn tracked_resident_bytes(&self) -> u128 {
        let index_bytes = (self.codec.sorted_body_ids.len() as u128) * 8;
        index_bytes + self.materialized_payload_bytes()
    }

    fn storage_digest(&self) -> String {
        let mut bytes = Vec::new();
        push_bytes(
            &mut bytes,
            self.codec.manifest.source_identity().digest().as_bytes(),
        );
        push_bytes(&mut bytes, self.codec.geometry().digest().as_bytes());
        for (index, payload) in &self.payloads {
            bytes.extend_from_slice(&index.to_be_bytes());
            push_bytes(&mut bytes, payload);
        }
        content_digest(&bytes)
    }

    pub fn snapshot(&self) -> StorageSnapshot {
        StorageSnapshot::from_facts(StorageSnapshotFacts {
            geometry: self.codec.geometry().clone(),
            source: self.codec.manifest.source_identity().clone(),
            logical_address_count: self.codec.geometry().logical_address_count(),
            materialized_address_count: self.materialized_address_count(),
            materialized_payload_bytes: self.materialized_payload_bytes(),
            resident_working_set_bytes: self.tracked_resident_bytes(),
            backing: PhysicalBacking::Sparse,
            exactness: StorageExactness::Exact,
            persistence: PersistenceBoundary::new(PHI664_PERSISTENCE_FORMAT, 1)
                .expect("static persistence identity is valid"),
            storage_digest: self.storage_digest(),
        })
        .expect("Phi664Store maintains the R6 storage invariants")
    }
}

impl ObservableStorage for Phi664Store {
    fn storage_observation_snapshot(&self) -> StorageSnapshot {
        self.snapshot()
    }
}

fn validate_coordinates(fibre: FibreId, x: u8, y: u8, z: u8) -> Result<(), Phi664Error> {
    let extent = fibre.side();
    for (axis, coordinate) in [("x", x), ("y", y), ("z", z)] {
        if coordinate >= extent {
            return Err(Phi664Error::CoordinateOutOfRange {
                fibre,
                axis,
                coordinate,
                extent,
            });
        }
    }
    Ok(())
}

fn canonical_body_id_bytes(sorted_body_ids: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + sorted_body_ids.len() * 8);
    bytes.extend_from_slice(&(sorted_body_ids.len() as u128).to_be_bytes());
    for body_id in sorted_body_ids {
        bytes.extend_from_slice(&body_id.to_be_bytes());
    }
    bytes
}

fn push_bytes(target: &mut Vec<u8>, bytes: &[u8]) {
    target.extend_from_slice(&(bytes.len() as u128).to_be_bytes());
    target.extend_from_slice(bytes);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phi664Error {
    EmptySourceField {
        field: &'static str,
    },
    EmptyMacrograph,
    DuplicateBodyId {
        body_id: u64,
    },
    UnknownBodyId {
        body_id: u64,
    },
    CoordinateOutOfRange {
        fibre: FibreId,
        axis: &'static str,
        coordinate: u8,
        extent: u8,
    },
    AddressSpaceOverflow,
    ZeroWindowLimit,
    WindowLimitTooLarge {
        max_window_addresses: u128,
    },
    WindowTooLarge {
        requested: u128,
        max: u128,
    },
    Storage(StorageContractError),
}

impl fmt::Display for Phi664Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySourceField { field } => write!(f, "source field {field} must not be empty"),
            Self::EmptyMacrograph => f.write_str("macrograph must contain at least one bodyId"),
            Self::DuplicateBodyId { body_id } => {
                write!(f, "macrograph contains duplicate bodyId {body_id}")
            }
            Self::UnknownBodyId { body_id } => {
                write!(f, "bodyId {body_id} is not present in the pinned macrograph")
            }
            Self::CoordinateOutOfRange {
                fibre,
                axis,
                coordinate,
                extent,
            } => write!(
                f,
                "{} coordinate {axis}={coordinate} is outside 0..{extent}",
                fibre.name()
            ),
            Self::AddressSpaceOverflow => f.write_str("Phi664 address-space arithmetic overflowed"),
            Self::ZeroWindowLimit => f.write_str("materialization window limit must be nonzero"),
            Self::WindowLimitTooLarge {
                max_window_addresses,
            } => write!(
                f,
                "materialization window limit {max_window_addresses} exceeds platform vector capacity"
            ),
            Self::WindowTooLarge { requested, max } => write!(
                f,
                "requested materialization window {requested} exceeds configured maximum {max}"
            ),
            Self::Storage(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Phi664Error {}

impl From<StorageContractError> for Phi664Error {
    fn from(value: StorageContractError) -> Self {
        Self::Storage(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsolqec_storage::observe_storage;

    fn spec() -> MacroSourceSpec {
        MacroSourceSpec {
            dataset_id: "fixture-macrograph:v1".into(),
            release_date: "2026-09-22".into(),
            license: "CC0".into(),
            node_source_uri: "fixture://nodes".into(),
            edge_source_uri: "fixture://edges".into(),
            node_identity_field: "bodyId".into(),
            node_projection: "all fixture bodyIds; ascending order".into(),
            edge_interpretation: "directed fixture edges".into(),
            graph_projection: "identity only".into(),
        }
    }

    fn codec() -> Phi664Codec {
        Phi664Codec::from_body_ids(spec(), vec![30, 10, 20]).unwrap()
    }

    #[test]
    fn phi664_is_disjoint_sum_not_cartesian_product() {
        assert_eq!(F27_POSITIONS + N125_POSITIONS + R512_POSITIONS, 664);
        assert_eq!(N125_OFFSET, 27);
        assert_eq!(R512_OFFSET, 152);
    }

    #[test]
    fn official_source_spec_is_version_pinned() {
        let source = MacroSourceSpec::male_cns_v1();
        assert_eq!(source.dataset_id, "male-cns:v1.0");
        assert_eq!(source.release_date, "2026-06-08");
        assert!(source.node_source_uri.contains("/v1.0/"));
        assert!(source.edge_source_uri.contains("/v1.0/"));
        assert!(source.edge_interpretation.contains("body_pre -> body_post"));
    }

    #[test]
    fn macro_node_identity_is_sorted_and_hashed_deterministically() {
        let first = Phi664Codec::from_body_ids(spec(), vec![30, 10, 20]).unwrap();
        let second = Phi664Codec::from_body_ids(spec(), vec![20, 30, 10]).unwrap();

        assert_eq!(first.macro_body_ids(), &[10, 20, 30]);
        assert_eq!(
            first.manifest().body_ids_digest(),
            second.manifest().body_ids_digest()
        );
        assert_eq!(
            first.manifest().source_identity(),
            second.manifest().source_identity()
        );
        assert_eq!(first.geometry(), second.geometry());
    }

    #[test]
    fn changing_macro_node_set_changes_source_and_geometry_identity() {
        let first = Phi664Codec::from_body_ids(spec(), vec![10, 20, 30]).unwrap();
        let second = Phi664Codec::from_body_ids(spec(), vec![10, 20, 40]).unwrap();

        assert_ne!(
            first.manifest().source_identity(),
            second.manifest().source_identity()
        );
        assert_ne!(first.geometry(), second.geometry());
    }

    #[test]
    fn duplicate_body_ids_are_rejected() {
        assert_eq!(
            Phi664Codec::from_body_ids(spec(), vec![10, 20, 10]).unwrap_err(),
            Phi664Error::DuplicateBodyId { body_id: 10 }
        );
    }

    #[test]
    fn logical_namespace_is_macro_nodes_times_664() {
        let codec = codec();
        assert_eq!(codec.manifest().macro_node_count(), 3);
        assert_eq!(codec.geometry().logical_address_count(), 3 * 664);
    }

    #[test]
    fn pack_unpack_round_trips_all_fibre_boundaries() {
        let codec = codec();
        let cases = [
            (10, FibreId::F27, 0, 0, 0, 0),
            (10, FibreId::F27, 2, 2, 2, 26),
            (10, FibreId::N125, 0, 0, 0, 27),
            (10, FibreId::N125, 4, 4, 4, 151),
            (10, FibreId::R512, 0, 0, 0, 152),
            (10, FibreId::R512, 7, 7, 7, 663),
            (20, FibreId::F27, 0, 0, 0, 664),
        ];

        for (body_id, fibre, x, y, z, expected_index) in cases {
            let address = codec.address(body_id, fibre, x, y, z).unwrap();
            let packed = codec.pack(&address).unwrap();
            assert_eq!(packed.index(), expected_index);
            assert_eq!(codec.unpack(&packed).unwrap(), address);
        }
    }

    #[test]
    fn address_constructor_rejects_unknown_body_and_bad_fibre_coordinate() {
        assert_eq!(
            codec().address(999, FibreId::F27, 0, 0, 0).unwrap_err(),
            Phi664Error::UnknownBodyId { body_id: 999 }
        );
        assert_eq!(
            codec().address(10, FibreId::F27, 3, 0, 0).unwrap_err(),
            Phi664Error::CoordinateOutOfRange {
                fibre: FibreId::F27,
                axis: "x",
                coordinate: 3,
                extent: 3,
            }
        );
    }

    #[test]
    fn addresses_cannot_cross_geometry_identity() {
        let first = codec();
        let second = Phi664Codec::from_body_ids(spec(), vec![10, 20, 40]).unwrap();
        let address = first.address(10, FibreId::F27, 0, 0, 0).unwrap();
        assert_eq!(
            second.pack(&address),
            Err(StorageContractError::GeometryMismatch)
        );
    }

    #[test]
    fn bounded_materialization_crosses_fibre_boundary_without_full_store() {
        let codec = codec();
        let start = codec.address(10, FibreId::F27, 2, 2, 1).unwrap();
        let store = Phi664Store::new(codec, 4).unwrap();
        let cells = store.materialize_window(&start, 3).unwrap();

        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].address.fibre(), FibreId::F27);
        assert_eq!(
            (
                cells[0].address.x(),
                cells[0].address.y(),
                cells[0].address.z()
            ),
            (2, 2, 1)
        );
        assert_eq!(cells[1].address.fibre(), FibreId::F27);
        assert_eq!(
            (
                cells[1].address.x(),
                cells[1].address.y(),
                cells[1].address.z()
            ),
            (2, 2, 2)
        );
        assert_eq!(cells[2].address.fibre(), FibreId::N125);
        assert_eq!(
            (
                cells[2].address.x(),
                cells[2].address.y(),
                cells[2].address.z()
            ),
            (0, 0, 0)
        );
        assert!(cells.iter().all(|cell| cell.payload.is_none()));
    }

    #[test]
    fn materialization_limit_is_enforced_before_allocation() {
        let codec = codec();
        let start = codec.address(10, FibreId::F27, 0, 0, 0).unwrap();
        let store = Phi664Store::new(codec, 2).unwrap();
        assert_eq!(
            store.materialize_window(&start, 3).unwrap_err(),
            Phi664Error::WindowTooLarge {
                requested: 3,
                max: 2,
            }
        );
    }

    #[test]
    fn sparse_payload_snapshot_keeps_logical_and_materialized_scale_separate() {
        let codec = codec();
        let a = codec.address(10, FibreId::F27, 0, 0, 0).unwrap();
        let b = codec.address(30, FibreId::R512, 7, 7, 7).unwrap();
        let mut store = Phi664Store::new(codec, 16).unwrap();

        store.write_payload(&a, vec![1, 2, 3]).unwrap();
        store.write_payload(&b, vec![4, 5]).unwrap();

        let snapshot = store.snapshot();
        let facts = snapshot.facts();
        assert_eq!(facts.logical_address_count, 3 * 664);
        assert_eq!(facts.materialized_address_count, 2);
        assert_eq!(facts.materialized_payload_bytes, 5);
        assert_eq!(facts.resident_working_set_bytes, 3 * 8 + 5);
        assert_eq!(facts.backing, PhysicalBacking::Sparse);
        assert_eq!(facts.exactness, StorageExactness::Exact);
    }

    #[test]
    fn payload_mutation_changes_storage_observation_identity() {
        let codec = codec();
        let address = codec.address(20, FibreId::N125, 1, 2, 3).unwrap();
        let mut store = Phi664Store::new(codec, 16).unwrap();

        let before = observe_storage(&store);
        store.write_payload(&address, vec![9, 8, 7]).unwrap();
        let after = observe_storage(&store);
        assert_ne!(before.artifact_id, after.artifact_id);

        store.remove_payload(&address).unwrap();
        let restored = observe_storage(&store);
        assert_eq!(before.artifact_id, restored.artifact_id);
    }

    #[test]
    fn payload_identity_is_independent_of_address() {
        let codec = codec();
        let a = codec.address(10, FibreId::F27, 0, 0, 0).unwrap();
        let b = codec.address(30, FibreId::R512, 7, 7, 7).unwrap();
        let mut store = Phi664Store::new(codec, 16).unwrap();

        let left = store.write_payload(&a, vec![1, 2, 3]).unwrap();
        let right = store.write_payload(&b, vec![1, 2, 3]).unwrap();
        assert_eq!(left, right);
    }
}

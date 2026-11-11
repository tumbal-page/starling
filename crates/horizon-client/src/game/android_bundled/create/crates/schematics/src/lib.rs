//! Strict, world-independent preflight for original Create 6.0.10 schematics.
//!
//! Source mapping (MIT, Copyright (c) The Create Team / The Creators of Create):
//! Create commit ac0c444d9828da3453ae8cc65338e8de063286fb, SchematicExport,
//! SchematicItem, SchematicPrinter, SchematicPlacePacket, BlockHelper and
//! SmartBlockEntity.writeSafe. Adapted placement policy, not a Java runtime.
//! See the repository THIRD_PARTY_NOTICES.md for the complete MIT notice.
//!
//! All world/registry access belongs to `Resolver` and the consuming adapter.
//! A failed preflight produces no Plan and this crate never changes a world.
//! No data-version migration, entities, alternate palette choice or transforms
//! are silently approximated. Supported source version is verified from the
//! Minecraft 1.21.1 client JAR's version.json (`world_version`: 3955).
pub mod nbt;
pub use nbt::{Compound, Tag, TagKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub const SOURCE_DATA_VERSION: i32 = 3955;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub path: String,
    pub message: String,
}
impl Error {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_uncompressed_bytes: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_collection_len: usize,
    pub max_string_bytes: usize,
    pub max_blocks: usize,
    pub max_side: i32,
    pub max_volume: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 << 20,
            max_uncompressed_bytes: 16 << 20,
            max_depth: 64,
            max_nodes: 500_000,
            max_collection_len: 65_536,
            max_string_bytes: 65_535,
            max_blocks: 32_768,
            max_side: 128,
            max_volume: 262_144,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}
impl BlockState {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            properties: BTreeMap::new(),
        }
    }
    pub fn command_string(&self) -> String {
        if self.properties.is_empty() {
            return self.name.clone();
        }
        format!(
            "{}[{}]",
            self.name,
            self.properties
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub relative_pos: [i32; 3],
    pub state_index: usize,
    pub block_entity: Option<Compound>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schematic {
    pub data_version: i32,
    pub size: [i32; 3],
    pub palettes: Vec<Vec<BlockState>>,
    pub blocks: Vec<Block>,
    /// Kept intact for diagnostics. Entity placement is outside this MVP.
    pub entities: Vec<Compound>,
}

fn list<'a>(tag: Option<&'a Tag>, kind: TagKind, path: &str) -> Result<&'a [Tag], Error> {
    match tag {
        Some(Tag::List { element, values }) if *element == kind || values.is_empty() => Ok(values),
        _ => Err(Error::new(path, format!("expected {kind:?} list"))),
    }
}
fn compound<'a>(tag: &'a Tag, path: &str) -> Result<&'a Compound, Error> {
    tag.as_compound()
        .ok_or_else(|| Error::new(path, "expected compound"))
}
fn fields(data: &Compound, allowed: &[&str], path: &str) -> Result<(), Error> {
    if let Some(key) = data.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(Error::new(format!("{path}.{key}"), "unsupported field"));
    }
    Ok(())
}
fn ints3(tag: Option<&Tag>, path: &str) -> Result<[i32; 3], Error> {
    let values = list(tag, TagKind::Int, path)?;
    if values.len() != 3 {
        return Err(Error::new(path, "expected exactly three integers"));
    }
    let mut result = [0; 3];
    for (out, value) in result.iter_mut().zip(values) {
        let Tag::Int(v) = value else {
            return Err(Error::new(path, "expected integer"));
        };
        *out = *v;
    }
    Ok(result)
}
fn string(tag: Option<&Tag>, path: &str) -> Result<String, Error> {
    match tag {
        Some(Tag::String(s)) => Ok(s.clone()),
        _ => Err(Error::new(path, "expected string")),
    }
}
fn read_palette(tags: &[Tag], path: &str) -> Result<Vec<BlockState>, Error> {
    tags.iter()
        .enumerate()
        .map(|(index, tag)| {
            let path = format!("{path}[{index}]");
            let data = compound(tag, &path)?;
            // Create 1.21.1 writes Name/Properties. Accept the current spelling too,
            // but reject ambiguous aliases rather than choosing one silently.
            fields(data, &["Name", "Properties", "id", "properties"], &path)?;
            if data.contains_key("Name") && data.contains_key("id")
                || data.contains_key("Properties") && data.contains_key("properties")
            {
                return Err(Error::new(path, "ambiguous palette aliases"));
            }
            let name = string(
                data.get("Name").or_else(|| data.get("id")),
                &format!("{path}.Name"),
            )?;
            let mut properties = BTreeMap::new();
            if let Some(tag) = data.get("Properties").or_else(|| data.get("properties")) {
                for (key, value) in compound(tag, &format!("{path}.Properties"))? {
                    properties.insert(
                        key.clone(),
                        string(Some(value), &format!("{path}.Properties.{key}"))?,
                    );
                }
            }
            Ok(BlockState { name, properties })
        })
        .collect()
}

pub fn read_gzip(bytes: &[u8], limits: &Limits) -> Result<Schematic, Error> {
    Schematic::from_compound(&nbt::decode_gzip(bytes, limits)?, limits)
}
impl Schematic {
    pub fn from_compound(root: &Compound, limits: &Limits) -> Result<Self, Error> {
        fields(
            root,
            &[
                "DataVersion",
                "size",
                "palette",
                "palettes",
                "blocks",
                "entities",
            ],
            "root",
        )?;
        let data_version = match root.get("DataVersion") {
            Some(Tag::Int(v)) => *v,
            _ => return Err(Error::new("DataVersion", "required integer missing")),
        };
        let size = ints3(root.get("size"), "size")?;
        let volume = size
            .iter()
            .try_fold(1u64, |v, s| {
                if *s <= 0 || *s > limits.max_side {
                    return None;
                }
                v.checked_mul(*s as u64)
            })
            .ok_or_else(|| Error::new("size", "nonpositive or excessive dimensions"))?;
        if volume > limits.max_volume {
            return Err(Error::new("size", "volume exceeds limit"));
        }
        if root.contains_key("palette") && root.contains_key("palettes") {
            return Err(Error::new("palette", "both palette and palettes supplied"));
        }
        let palettes = if root.contains_key("palettes") {
            let outer = list(root.get("palettes"), TagKind::List, "palettes")?;
            outer
                .iter()
                .enumerate()
                .map(|(i, tag)| {
                    let path = format!("palettes[{i}]");
                    read_palette(list(Some(tag), TagKind::Compound, &path)?, &path)
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![read_palette(
                list(root.get("palette"), TagKind::Compound, "palette")?,
                "palette",
            )?]
        };
        if palettes.is_empty() || palettes.iter().any(Vec::is_empty) {
            return Err(Error::new("palette", "empty palette"));
        }
        if palettes.iter().any(|p| p.len() > limits.max_collection_len) {
            return Err(Error::new("palette", "palette exceeds limit"));
        }
        let tags = list(root.get("blocks"), TagKind::Compound, "blocks")?;
        if tags.len() > limits.max_blocks {
            return Err(Error::new("blocks", "block count exceeds limit"));
        }
        let mut blocks = Vec::new();
        let mut positions = BTreeSet::new();
        for (i, tag) in tags.iter().enumerate() {
            let path = format!("blocks[{i}]");
            let data = compound(tag, &path)?;
            fields(data, &["pos", "state", "nbt"], &path)?;
            let relative_pos = ints3(data.get("pos"), &format!("{path}.pos"))?;
            if relative_pos
                .iter()
                .zip(size)
                .any(|(p, s)| *p < 0 || *p >= s)
            {
                return Err(Error::new(format!("{path}.pos"), "position outside size"));
            }
            if !positions.insert(relative_pos) {
                return Err(Error::new(
                    format!("{path}.pos"),
                    "duplicate block position",
                ));
            }
            let state_index = match data.get("state") {
                Some(Tag::Int(v)) if *v >= 0 => *v as usize,
                _ => {
                    return Err(Error::new(
                        format!("{path}.state"),
                        "expected nonnegative palette index",
                    ));
                }
            };
            if palettes.iter().any(|p| state_index >= p.len()) {
                return Err(Error::new(
                    format!("{path}.state"),
                    "palette index out of range",
                ));
            }
            let block_entity = data
                .get("nbt")
                .map(|t| compound(t, &format!("{path}.nbt")).cloned())
                .transpose()?;
            blocks.push(Block {
                relative_pos,
                state_index,
                block_entity,
            });
        }
        let entities = list(root.get("entities"), TagKind::Compound, "entities")?
            .iter()
            .enumerate()
            .map(|(i, t)| compound(t, &format!("entities[{i}]")).cloned())
            .collect::<Result<_, _>>()?;
        Ok(Self {
            data_version,
            size,
            palettes,
            blocks,
            entities,
        })
    }

    /// Vanilla 1.21.1 structure layout, suitable for our generated fixtures.
    pub fn to_compound(&self) -> Compound {
        let ints = |v: [i32; 3]| Tag::list(TagKind::Int, v.into_iter().map(Tag::Int).collect());
        let palette = |states: &[BlockState]| {
            Tag::list(
                TagKind::Compound,
                states
                    .iter()
                    .map(|s| {
                        let mut c = Compound::from([("Name".into(), Tag::String(s.name.clone()))]);
                        if !s.properties.is_empty() {
                            c.insert(
                                "Properties".into(),
                                Tag::Compound(
                                    s.properties
                                        .iter()
                                        .map(|(k, v)| (k.clone(), Tag::String(v.clone())))
                                        .collect(),
                                ),
                            );
                        }
                        Tag::Compound(c)
                    })
                    .collect(),
            )
        };
        let mut root = Compound::from([
            ("DataVersion".into(), Tag::Int(self.data_version)),
            ("size".into(), ints(self.size)),
            (
                "blocks".into(),
                Tag::list(
                    TagKind::Compound,
                    self.blocks
                        .iter()
                        .map(|b| {
                            let mut c = Compound::from([
                                ("pos".into(), ints(b.relative_pos)),
                                (
                                    "state".into(),
                                    Tag::Int(i32::try_from(b.state_index).unwrap_or(-1)),
                                ),
                            ]);
                            if let Some(nbt) = &b.block_entity {
                                c.insert("nbt".into(), Tag::Compound(nbt.clone()));
                            }
                            Tag::Compound(c)
                        })
                        .collect(),
                ),
            ),
            (
                "entities".into(),
                Tag::list(
                    TagKind::Compound,
                    self.entities.iter().cloned().map(Tag::Compound).collect(),
                ),
            ),
        ]);
        if self.palettes.len() == 1 {
            root.insert("palette".into(), palette(&self.palettes[0]));
        } else {
            root.insert(
                "palettes".into(),
                Tag::list(
                    TagKind::List,
                    self.palettes.iter().map(|p| palette(p)).collect(),
                ),
            );
        }
        root
    }
}

/// The adapter must supply exact registry validation, not a vanilla fallback.
pub trait Resolver {
    fn block_schema(&self, state: &BlockState) -> Result<BlockSchema, String>;
    /// Additional source BlockHelper/state-filter behavior for explicitly
    /// supported special blocks. Unsupported special blocks must be rejected.
    fn prepare_state(&self, state: &BlockState) -> Result<BlockState, String> {
        Ok(state.clone())
    }
    /// Validate semantic contents (ranges, item IDs/components, filters, text)
    /// in addition to the schema's outer tag types. Called before any Plan exists.
    fn validate_safe_nbt(&self, _state: &BlockState, _data: &Compound) -> Result<(), String> {
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct BlockSchema {
    /// Same requested state with omitted properties filled from registry defaults.
    pub state: BlockState,
    pub block_entity: Option<BlockEntitySchema>,
    /// SchematicPrinter's brittle/deferred pass, after ordinary blocks.
    pub deferred: bool,
}
#[derive(Clone, Debug)]
pub struct BlockEntitySchema {
    pub id: String,
    /// Source writeSafe fields actually implemented by the native adapter.
    pub safe_fields: BTreeMap<String, TagKind>,
    /// Known fields deliberately omitted by source safe-NBT, reported in notes.
    /// Unknown fields are rejected rather than claiming their behavior is supported.
    pub discarded_fields: BTreeSet<String>,
}
impl BlockEntitySchema {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            safe_fields: BTreeMap::new(),
            discarded_fields: BTreeSet::new(),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub include_air: bool,
    pub quarter_turns: u8,
    pub mirror: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub relative_pos: Option<[i32; 3]>,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub relative_pos: [i32; 3],
    pub field: String,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedBlockEntity {
    pub id: String,
    pub data: Compound,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedBlock {
    pub relative_pos: [i32; 3],
    pub state: BlockState,
    pub block_entity: Option<PlannedBlockEntity>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub size: [i32; 3],
    pub blocks: Vec<PlannedBlock>,
    pub notes: Vec<Note>,
    pub skipped_air: usize,
}

fn valid_id(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c))
        && path
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"/_.-".contains(&c))
}
fn resolve(resolver: &impl Resolver, state: &BlockState) -> Result<BlockSchema, String> {
    if !valid_id(&state.name) {
        return Err(format!("invalid namespaced block ID {:?}", state.name));
    }
    for (key, value) in &state.properties {
        if key.is_empty()
            || value.is_empty()
            || !key
                .bytes()
                .chain(value.bytes())
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c))
        {
            return Err(format!("invalid property syntax {key:?}={value:?}"));
        }
    }
    let schema = resolver.block_schema(state)?;
    if schema.state.name != state.name
        || state
            .properties
            .iter()
            .any(|(k, v)| schema.state.properties.get(k) != Some(v))
    {
        return Err(
            "resolver changed an explicit block ID/property instead of validating it".into(),
        );
    }
    if schema
        .block_entity
        .as_ref()
        .is_some_and(|be| !valid_id(&be.id))
    {
        return Err("invalid block-entity schema ID".into());
    }
    Ok(schema)
}

/// Validate the complete schematic before returning a world-independent plan.
/// The adapter must additionally preflight destination bounds/chunks/permissions
/// and snapshot/rollback a failed write. This function has no world handle.
pub fn preflight(
    schematic: &Schematic,
    resolver: &impl Resolver,
    options: &Options,
) -> Result<Plan, Vec<Issue>> {
    let mut issues = Vec::new();
    let mut fail = |path: String, pos: Option<[i32; 3]>, message: String| {
        issues.push(Issue {
            path,
            relative_pos: pos,
            message,
        })
    };
    if schematic.data_version != SOURCE_DATA_VERSION {
        fail(
            "DataVersion".into(),
            None,
            format!(
                "unsupported data version {}; expected {SOURCE_DATA_VERSION}; no migration is implemented",
                schematic.data_version
            ),
        );
    }
    if schematic.palettes.len() != 1 {
        fail(
            "palettes".into(),
            None,
            "exactly one palette is supported; random palette choice is not implemented".into(),
        );
    }
    if options.quarter_turns != 0 || options.mirror {
        fail(
            "transform".into(),
            None,
            "rotation and mirroring are not implemented".into(),
        );
    }
    let limits = Limits::default();
    let volume = schematic.size.iter().try_fold(1u64, |v, s| {
        if *s <= 0 || *s > limits.max_side {
            None
        } else {
            v.checked_mul(*s as u64)
        }
    });
    if volume.is_none_or(|v| v > limits.max_volume) {
        fail(
            "size".into(),
            None,
            "invalid or excessive dimensions".into(),
        );
    }
    if schematic.blocks.len() > limits.max_blocks {
        fail(
            "blocks".into(),
            None,
            "block count exceeds placement limit".into(),
        );
    }
    if schematic.palettes.iter().any(Vec::is_empty) {
        fail("palette".into(), None, "empty palette".into());
    }
    for (i, entity) in schematic.entities.iter().enumerate() {
        let id = entity
            .get("nbt")
            .and_then(Tag::as_compound)
            .and_then(|c| c.get("id"))
            .and_then(Tag::as_str)
            .unwrap_or("<unknown>");
        fail(
            format!("entities[{i}]"),
            None,
            format!("entity {id} is unsupported; entities are never silently dropped"),
        );
    }
    let mut resolved = Vec::new();
    for (p, palette) in schematic.palettes.iter().enumerate() {
        for (i, state) in palette.iter().enumerate() {
            match resolve(resolver, state) {
                Ok(schema) if p == 0 => resolved.push(Some(schema)),
                Ok(_) => {}
                Err(message) => {
                    fail(
                        format!("palettes[{p}][{i}]"),
                        schematic
                            .blocks
                            .iter()
                            .find(|b| b.state_index == i)
                            .map(|b| b.relative_pos),
                        message,
                    );
                    if p == 0 {
                        resolved.push(None);
                    }
                }
            }
        }
    }
    let mut blocks = Vec::new();
    let mut notes = Vec::new();
    let mut skipped_air = 0;
    let mut positions = BTreeSet::new();
    for (index, block) in schematic.blocks.iter().enumerate() {
        let path = format!("blocks[{index}]");
        let pos = block.relative_pos;
        if pos
            .iter()
            .zip(schematic.size)
            .any(|(p, s)| *p < 0 || *p >= s)
        {
            fail(
                format!("{path}.pos"),
                Some(pos),
                "position outside size".into(),
            );
        }
        if !positions.insert(pos) {
            fail(
                format!("{path}.pos"),
                Some(pos),
                "duplicate block position".into(),
            );
        }
        let Some(schema) = resolved.get(block.state_index) else {
            fail(
                format!("{path}.state"),
                Some(pos),
                "palette index out of range".into(),
            );
            continue;
        };
        let Some(schema) = schema else {
            continue;
        };
        let mut state = schema.state.clone();
        for property in ["waterlogged", "extended"] {
            if let Some(value) = state.properties.get_mut(property) {
                *value = "false".into();
            }
        }
        let prepared = match resolver
            .prepare_state(&state)
            .and_then(|s| resolve(resolver, &s))
        {
            Ok(prepared) => prepared,
            Err(message) => {
                fail(path, Some(pos), message);
                continue;
            }
        };
        for (key, before) in &schema.state.properties {
            if prepared.state.properties.get(key) != Some(before) {
                notes.push(Note {
                    relative_pos: pos,
                    field: key.clone(),
                    message: format!(
                        "source creative placement: {before} -> {}",
                        prepared
                            .state
                            .properties
                            .get(key)
                            .map(String::as_str)
                            .unwrap_or("<absent>")
                    ),
                });
            }
        }
        if prepared.state.name != schema.state.name {
            notes.push(Note {
                relative_pos: pos,
                field: "Name".into(),
                message: format!(
                    "source creative placement: {} -> {}",
                    schema.state.name, prepared.state.name
                ),
            });
        }
        let mut planned_be = prepared.block_entity.as_ref().map(|s| PlannedBlockEntity {
            id: s.id.clone(),
            data: Compound::new(),
        });
        if let Some(raw) = &block.block_entity {
            let Some(be_schema) = &prepared.block_entity else {
                fail(
                    format!("{path}.nbt"),
                    Some(pos),
                    "block does not support a block entity".into(),
                );
                continue;
            };
            if raw.get("id").and_then(Tag::as_str) != Some(be_schema.id.as_str()) {
                fail(
                    format!("{path}.nbt.id"),
                    Some(pos),
                    format!(
                        "expected block entity {}; got {:?}",
                        be_schema.id,
                        raw.get("id")
                    ),
                );
            }
            let mut safe = Compound::new();
            for (key, value) in raw {
                if key == "id" {
                    continue;
                }
                if ["x", "y", "z"].contains(&key.as_str()) {
                    if !matches!(value, Tag::Int(_)) {
                        fail(
                            format!("{path}.nbt.{key}"),
                            Some(pos),
                            "block-entity position metadata must be Int".into(),
                        );
                    }
                } else if let Some(kind) = be_schema.safe_fields.get(key) {
                    if value.kind() != *kind {
                        fail(
                            format!("{path}.nbt.{key}"),
                            Some(pos),
                            format!("expected {kind:?}, got {:?}", value.kind()),
                        );
                    } else {
                        safe.insert(key.clone(), value.clone());
                    }
                } else if be_schema.discarded_fields.contains(key) {
                    notes.push(Note {
                        relative_pos: pos,
                        field: key.clone(),
                        message: "discarded by explicitly supported source safe-NBT policy".into(),
                    });
                } else {
                    fail(
                        format!("{path}.nbt.{key}"),
                        Some(pos),
                        "unsupported block-entity field; safe placement policy is not implemented"
                            .into(),
                    );
                }
            }
            if let Err(message) = resolver.validate_safe_nbt(&prepared.state, &safe) {
                fail(format!("{path}.nbt"), Some(pos), message);
            }
            planned_be = Some(PlannedBlockEntity {
                id: be_schema.id.clone(),
                data: safe,
            });
        }
        if !options.include_air
            && matches!(
                prepared.state.name.as_str(),
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            )
        {
            skipped_air += 1;
            continue;
        }
        blocks.push((
            prepared.deferred,
            PlannedBlock {
                relative_pos: pos,
                state: prepared.state,
                block_entity: planned_be,
            },
        ));
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    // Source creative printer visits X first, then Z, then Y, and does a
    // separate deferred pass. Source BE/shape behavior still belongs to adapter.
    blocks.sort_by_key(|(deferred, b)| {
        (
            *deferred,
            b.relative_pos[1],
            b.relative_pos[2],
            b.relative_pos[0],
        )
    });
    Ok(Plan {
        size: schematic.size,
        blocks: blocks.into_iter().map(|(_, b)| b).collect(),
        notes,
        skipped_air,
    })
}

#[cfg(test)]
mod tests;

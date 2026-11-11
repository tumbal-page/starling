// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create 6.0.10 SchematicPlacePacket/SchematicPrinter/BlockHelper adapter.
// Identity transforms, source safe-NBT and explicit supported fixtures only.
// Adapted for Android bundled port.

use super::Game;
use crate::{
    containers::BlockEntity,
    mod_api::{NativeCommand, NativeServerPlayer},
};
use android_bundled::create::crates::schematics::{
    BlockEntitySchema, BlockSchema, BlockState, Compound, Limits, Options, Plan, Resolver, Tag,
    TagKind,
};
use horizon_world::{
    nbt::{Compound as WorldCompound, Tag as WorldTag},
    registry::BlockStateId,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Read};

pub(super) fn register(game: &Game<'_>, commands: &mut Vec<NativeCommand>) {
    if game.native_on("create") {
        commands.push(NativeCommand::op(
            "create schematic inspect <filename:word>",
            2,
        ));
        commands.push(NativeCommand::op(
            "create schematic paste <filename:word> <x:integer> <y:integer> <z:integer>",
            2,
        ));
    }
}

struct Registry<'a, 'b> {
    game: &'a Game<'b>,
}
fn supported_vanilla(name: &str) -> bool {
    let Some(name) = name.strip_prefix("minecraft:") else {
        return false;
    };
    matches!(
        name,
        "air"
            | "cave_air"
            | "void_air"
            | "stone"
            | "cobblestone"
            | "smooth_stone"
            | "andesite"
            | "diorite"
            | "granite"
            | "polished_andesite"
            | "polished_diorite"
            | "polished_granite"
            | "stone_bricks"
            | "bricks"
            | "deepslate"
            | "cobbled_deepslate"
            | "netherrack"
            | "end_stone"
            | "sand"
            | "red_sand"
            | "gravel"
            | "soul_sand"
            | "soul_soil"
            | "dirt"
            | "grass_block"
            | "glass"
            | "tinted_glass"
            | "iron_bars"
            | "iron_block"
            | "gold_block"
            | "copper_block"
            | "diamond_block"
            | "redstone_block"
            | "obsidian"
            | "crying_obsidian"
            | "bedrock"
            | "barrier"
            | "water"
            | "lava"
            | "fire"
            | "soul_fire"
            | "campfire"
            | "soul_campfire"
            | "lever"
            | "torch"
            | "wall_torch"
            | "redstone_torch"
            | "redstone_wall_torch"
            | "redstone_wire"
            | "repeater"
            | "comparator"
            | "chest"
            | "trapped_chest"
            | "barrel"
            | "hopper"
            | "dispenser"
            | "dropper"
            | "furnace"
            | "blast_furnace"
            | "smoker"
            | "magma_block"
    ) || [
        "_planks",
        "_log",
        "_wood",
        "_stairs",
        "_slab",
        "_fence",
        "_wall",
        "_stained_glass",
        "_concrete",
        "_terracotta",
        "_wool",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}
fn vanilla_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "minecraft:chest" | "minecraft:trapped_chest" => "minecraft:chest",
        "minecraft:barrel" => "minecraft:barrel",
        "minecraft:hopper" => "minecraft:hopper",
        "minecraft:dispenser" => "minecraft:dispenser",
        "minecraft:dropper" => "minecraft:dropper",
        "minecraft:furnace" => "minecraft:furnace",
        "minecraft:blast_furnace" => "minecraft:blast_furnace",
        "minecraft:smoker" => "minecraft:smoker",
        "minecraft:campfire" | "minecraft:soul_campfire" => "minecraft:campfire",
        _ => return None,
    })
}
impl Registry<'_, '_> {
    fn id(&self, state: &BlockState) -> Result<BlockStateId, String> {
        let registry = self.game.pack.assets.registry;
        let default = registry
            .default_state(&state.name)
            .ok_or_else(|| format!("unknown block {}", state.name))?;
        let mut descriptor = registry
            .state(default)
            .ok_or("missing registry default")?
            .clone();
        for (key, value) in &state.properties {
            if !descriptor.properties.contains_key(key) {
                return Err(format!("{} has no property {key}", state.name));
            }
            descriptor.properties.insert(key.clone(), value.clone());
        }
        registry
            .id(&descriptor)
            .ok_or_else(|| format!("invalid property combination {}", state.command_string()))
    }
}
impl Resolver for Registry<'_, '_> {
    fn block_schema(&self, state: &BlockState) -> Result<BlockSchema, String> {
        if state.name == "create:belt" {
            return Err("mechanical belt schematic chains have not passed native fixture validation; create chains with create:belt_connector instead".into());
        }
        if state.name == "create:basin"
            && state.properties.get("facing").is_some_and(|v| v != "down")
        {
            return Err(
                "basin side-spout behavior is outside the supported schematic subset".into(),
            );
        }
        let id = self.id(state)?;
        let registered = self
            .game
            .pack
            .assets
            .registry
            .state(id)
            .ok_or("missing resolved state")?;
        let mut entity = None;
        if state.name.starts_with("create:") {
            crate::game::create_port::entity(&state.name, &mut entity);
            if entity.is_none()
                && !matches!(
                    state.name.as_str(),
                    "create:andesite_casing" | "create:brass_casing" | "create:copper_casing"
                )
            {
                return Err(format!(
                    "source schematic behavior is not implemented for {}",
                    state.name
                ));
            }
        } else if !supported_vanilla(&state.name) {
            return Err(format!(
                "block {} is outside the supported source schematic subset",
                state.name
            ));
        }
        let block_entity = if let Some((id, _)) = entity {
            let mut schema = BlockEntitySchema::new(id);
            schema.discarded_fields.extend(
                [
                    "Speed",
                    "Source",
                    "Network",
                    "Sequence",
                    "NeedsSpeedUpdate",
                    "NeedsUpdate",
                    "Owner",
                    "Advancements",
                ]
                .map(str::to_owned),
            );
            match state.name.as_str() {
                "create:creative_motor" => {
                    schema
                        .safe_fields
                        .insert("ScrollValue".into(), TagKind::Int);
                    schema.discarded_fields.insert("HasAttachedComputer".into());
                }
                "create:depot" => {
                    schema
                        .discarded_fields
                        .extend(["HeldItem", "OutputBuffer", "Incoming"].map(str::to_owned));
                }
                "create:millstone" => {
                    schema
                        .discarded_fields
                        .extend(["Timer", "InputInventory", "OutputInventory"].map(str::to_owned));
                }
                "create:basin" => {
                    // SmartBlockEntity.writeSafe retains safe behaviours;
                    // ordinary inventories/tanks and output animation do not
                    // survive original creative schematic placement.
                    schema.discarded_fields.extend(
                        [
                            "InputItems",
                            "OutputItems",
                            "InputTanks",
                            "OutputTanks",
                            "PreferredSpoutput",
                            "DisabledSpoutput",
                            "Overflow",
                            "FluidOverflow",
                            "VisualizedItems",
                            "VisualizedFluids",
                        ]
                        .map(str::to_owned),
                    );
                    schema
                        .safe_fields
                        .insert("Filter".into(), TagKind::Compound);
                    schema
                        .safe_fields
                        .insert("FilterAmount".into(), TagKind::Int);
                    schema.safe_fields.insert("UpTo".into(), TagKind::Byte);
                }
                "create:mechanical_mixer" => {
                    schema
                        .discarded_fields
                        .extend(["Running", "Ticks"].map(str::to_owned));
                }
                "create:mechanical_press" => {
                    schema.discarded_fields.extend(
                        ["Running", "Mode", "Finished", "Ticks", "TracksCreated"]
                            .map(str::to_owned),
                    );
                }
                "create:mechanical_drill" => {
                    schema
                        .discarded_fields
                        .extend(["Progress", "NextTick", "Breaking"].map(str::to_owned));
                }
                "create:speedometer" | "create:stressometer" => {
                    schema
                        .discarded_fields
                        .extend(["Value", "Color", "Stress", "Capacity"].map(str::to_owned));
                }
                "create:hand_crank" => {
                    schema
                        .discarded_fields
                        .extend(["InUse", "Backwards", "IndependentAngle"].map(str::to_owned));
                }
                "create:water_wheel" | "create:large_water_wheel" => {
                    schema
                        .safe_fields
                        .insert("Material".into(), TagKind::Compound);
                    schema
                        .discarded_fields
                        .extend(["FlowScore", "FlowScoreUpdate"].map(str::to_owned));
                }
                _ => {}
            }
            Some(schema)
        } else if let Some(id) = vanilla_entity(&state.name) {
            let mut schema = BlockEntitySchema::new(id);
            // These vanilla containers are not source SAFE_NBT and have no
            // registered safe writer: creative Create placement empties them.
            schema.discarded_fields.extend(
                [
                    "Items",
                    "LootTable",
                    "LootTableSeed",
                    "CustomName",
                    "Lock",
                    "TransferCooldown",
                    "BurnTime",
                    "CookTime",
                    "CookTimeTotal",
                    "RecipesUsed",
                    "CookingTimes",
                    "CookingTotalTimes",
                ]
                .map(str::to_owned),
            );
            Some(schema)
        } else if self.game.res.facts().get(id).block_entity
            || BlockEntity::for_block(&state.name).is_some()
        {
            return Err(format!(
                "{} requires an unsupported block-entity placement policy",
                state.name
            ));
        } else {
            None
        };
        Ok(BlockSchema {
            state: BlockState {
                name: registered.name.clone(),
                properties: registered
                    .properties
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            },
            block_entity,
            deferred: matches!(
                state.name.as_str(),
                "minecraft:lever"
                    | "minecraft:torch"
                    | "minecraft:wall_torch"
                    | "minecraft:redstone_torch"
                    | "minecraft:redstone_wall_torch"
                    | "minecraft:redstone_wire"
                    | "minecraft:repeater"
                    | "minecraft:comparator"
            ),
        })
    }
    fn validate_safe_nbt(&self, state: &BlockState, data: &Compound) -> Result<(), String> {
        if state.name == "create:basin"
            && let Some(Tag::Compound(filter)) = data.get("Filter")
            && !filter.is_empty()
        {
            return Err("basin recipe filters are outside the supported schematic subset".into());
        }
        if let Some(Tag::Int(rpm)) = data.get("ScrollValue")
            && !(-256..=256).contains(rpm)
        {
            return Err("source motor ScrollValue must be between -256 and 256".into());
        }
        if let Some(Tag::Compound(material)) = data.get("Material") {
            if material
                .keys()
                .any(|k| !matches!(k.as_str(), "Name" | "Properties"))
            {
                return Err("unsupported wheel Material field".into());
            }
            let name = material
                .get("Name")
                .and_then(Tag::as_str)
                .ok_or("wheel Material.Name must be a string")?;
            if !name
                .strip_prefix("minecraft:")
                .and_then(|s| s.strip_suffix("_planks"))
                .is_some_and(|wood| android_bundled::create::crates::assets::WOODS.contains(&wood))
            {
                return Err("wheel Material is not an implemented source wood variant".into());
            }
            let mut properties = BTreeMap::new();
            if let Some(value) = material.get("Properties") {
                let Tag::Compound(values) = value else {
                    return Err("wheel Material.Properties must be a compound".into());
                };
                for (k, v) in values {
                    properties.insert(
                        k.clone(),
                        v.as_str()
                            .ok_or("wheel Material property must be a string")?
                            .to_owned(),
                    );
                }
            }
            self.id(&BlockState {
                name: name.into(),
                properties,
            })?;
            if !state.name.ends_with("water_wheel") {
                return Err("Material is only supported on water wheels".into());
            }
        }
        Ok(())
    }
    fn prepare_state(&self, state: &BlockState) -> Result<BlockState, String> {
        let mut state = state.clone();
        // BlockHelper.setZeroAge clears FireBlock.AGE_15 before placement.
        if state.name == "minecraft:fire" && state.properties.contains_key("age") {
            state.properties.insert("age".into(), "0".into());
        }
        Ok(state)
    }
}

fn world_tag(tag: &Tag) -> WorldTag {
    match tag {
        Tag::Byte(v) => WorldTag::Byte(*v),
        Tag::Short(v) => WorldTag::Short(*v),
        Tag::Int(v) => WorldTag::Int(*v),
        Tag::Long(v) => WorldTag::Long(*v),
        Tag::Float(v) => WorldTag::Float(*v),
        Tag::Double(v) => WorldTag::Double(*v),
        Tag::String(v) => WorldTag::String(v.as_str().into()),
        Tag::ByteArray(v) => WorldTag::ByteArray(v.iter().map(|v| *v as u8).collect()),
        Tag::IntArray(v) => WorldTag::IntArray(v.clone()),
        Tag::LongArray(v) => WorldTag::LongArray(v.clone()),
        Tag::List { values, .. } => WorldTag::List(values.iter().map(world_tag).collect()),
        Tag::Compound(v) => WorldTag::Compound(world_compound(v)),
    }
}
fn world_compound(compound: &Compound) -> WorldCompound {
    compound
        .iter()
        .map(|(k, v)| (k.as_str().into(), world_tag(v)))
        .collect()
}
fn read_file(filename: &str) -> Result<Vec<u8>, String> {
    if !filename.ends_with(".nbt")
        || filename.len() > 128
        || !filename
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err(
            "expected one plain .nbt filename in this instance's schematics directory".into(),
        );
    }
    let directory = Game::game_dir().join("schematics");
    if std::fs::symlink_metadata(&directory)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("symlinked schematics directory is unsupported".into());
    }
    let path = directory.join(filename);
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("schematic must be a regular file, not a symlink".into());
    }
    let maximum = Limits::default().max_compressed_bytes;
    if metadata.len() > maximum as u64 {
        return Err("compressed schematic exceeds limit".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > maximum {
        return Err("compressed schematic exceeds limit".into());
    }
    Ok(bytes)
}
fn failure(message: impl Into<String>) -> Value {
    json!({"ok":false,"issues":[{"path":"command","message":message.into()}],"notes":[],"blocks":0,"skipped_air":0})
}
fn reply(game: &mut Game<'_>, report: Value) {
    game.native_feedback(horizon_client::text::Text::literal(report.to_string()));
}

struct Write {
    pos: [i32; 3],
    state: BlockStateId,
    nbt: Option<String>,
    old: BlockStateId,
    old_entity: Option<BlockEntity>,
}
fn destination(game: &Game<'_>, plan: &Plan, origin: [i32; 3]) -> Result<Vec<Write>, Value> {
    let registry = Registry { game };
    let (min_y, height) = game.native_world_height();
    let mut writes = Vec::new();
    let mut issues = Vec::new();
    for block in &plan.blocks {
        let mut pos = [0; 3];
        let mut overflow = false;
        for i in 0..3 {
            if let Some(v) = origin[i].checked_add(block.relative_pos[i]) {
                pos[i] = v;
            } else {
                overflow = true;
            }
        }
        let reason = if overflow {
            Some("coordinate overflow")
        } else if pos[1] < min_y || pos[1] >= min_y.saturating_add(height) {
            Some("position outside world height")
        } else if !(-29_999_984..=29_999_984).contains(&pos[0])
            || !(-29_999_984..=29_999_984).contains(&pos[2])
            || !game
                .border
                .contains(f64::from(pos[0]) + 0.5, f64::from(pos[2]) + 0.5)
        {
            Some("position outside world border")
        } else if !game.world.loaded(pos[0], pos[2]) {
            Some("destination chunk is not loaded")
        } else if game
            .res
            .facts()
            .get(game.world.block(pos[0], pos[1], pos[2]))
            .hardness
            < 0.0
        {
            Some("source creative placement cannot replace an unbreakable destination")
        } else {
            None
        };
        if let Some(reason) = reason {
            issues.push(json!({"path":"destination","pos":pos,"message":reason}));
            continue;
        }
        let state = match registry.id(&block.state) {
            Ok(id) => id,
            Err(e) => {
                issues.push(json!({"path":"destination","pos":pos,"message":e}));
                continue;
            }
        };
        let nbt = block.block_entity.as_ref().map(|be| {
            let mut data = world_compound(&be.data);
            data.insert("id".into(), WorldTag::String(be.id.as_str().into()));
            for (k, v) in ["x", "y", "z"].iter().zip(pos) {
                data.insert((*k).into(), WorldTag::Int(v));
            }
            horizon_world::snbt_text::to_snbt(&WorldTag::Compound(data))
        });
        writes.push(Write {
            pos,
            state,
            nbt,
            old: game.world.block(pos[0], pos[1], pos[2]),
            old_entity: game
                .world
                .block_entities
                .get(&(pos[0], pos[1], pos[2]))
                .cloned(),
        });
    }
    if issues.is_empty() {
        Ok(writes)
    } else {
        Err(
            json!({"ok":false,"issues":issues,"notes":[],"blocks":0,"skipped_air":plan.skipped_air,"size":plan.size}),
        )
    }
}

pub(super) fn command(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    line: &str,
    ran: &mut bool,
) {
    let words = line.split_whitespace().collect::<Vec<_>>();
    if words.get(..2) != Some(&["create", "schematic"][..]) || !game.native_on("create") {
        return;
    }
    *ran = true;
    if player.game_mode != 1 || player.permission < 2 {
        reply(
            game,
            failure("schematic import requires creative mode and operator permission 2"),
        );
        return;
    }
    let (mode, filename, origin) = match words.as_slice() {
        ["create", "schematic", "inspect", name] => ("inspect", *name, None),
        ["create", "schematic", "paste", name, x, y, z] => {
            let parsed = [x.parse::<i32>(), y.parse::<i32>(), z.parse::<i32>()];
            let [Ok(x), Ok(y), Ok(z)] = parsed else {
                reply(
                    game,
                    failure("paste requires three absolute integer coordinates"),
                );
                return;
            };
            ("paste", *name, Some([x, y, z]))
        }
        _ => {
            reply(
                game,
                failure(
                    "usage: create schematic inspect FILE.nbt | create schematic paste FILE.nbt X Y Z",
                ),
            );
            return;
        }
    };
    let bytes = match read_file(filename) {
        Ok(bytes) => bytes,
        Err(e) => {
            reply(game, failure(e));
            return;
        }
    };
    let mut schematic = match android_bundled::create::crates::schematics::read_gzip(&bytes, &Limits::default()) {
        Ok(s) => s,
        Err(e) => {
            reply(game, failure(e.to_string()));
            return;
        }
    };
    // Java ScrollValueBehaviour.read uses getInt: missing value is zero when
    // an existing exported BE compound is read, unlike a never-loaded motor's16.
    for block in &mut schematic.blocks {
        if schematic.palettes[0]
            .get(block.state_index)
            .is_some_and(|s| s.name == "create:creative_motor")
            && let Some(raw) = &mut block.block_entity
        {
            // Verified original metadata includes this computed ComputerCraft
            // flag. Accept only the implemented unattached motor case.
            if let Some(value) = raw.get("HasAttachedComputer")
                && value != &Tag::Byte(0)
            {
                reply(
                    game,
                    failure(
                        "attached ComputerCraft motor behavior is unsupported; HasAttachedComputer must be Byte(0)",
                    ),
                );
                return;
            }
            raw.entry("ScrollValue".into()).or_insert(Tag::Int(0));
        }
    }
    let plan = match android_bundled::create::crates::schematics::preflight(
        &schematic,
        &Registry { game },
        &Options::default(),
    ) {
        Ok(plan) => plan,
        Err(issues) => {
            reply(
                game,
                json!({"ok":false,"issues":issues.iter().map(|i|json!({"path":i.path,"pos":i.relative_pos,"message":i.message})).collect::<Vec<_>>(),"notes":[],"blocks":0,"skipped_air":0,"size":schematic.size}),
            );
            return;
        }
    };
    let mut report = json!({"ok":true,"mode":mode,"issues":[],"notes":plan.notes.iter().map(|n|json!({"pos":n.relative_pos,"field":n.field,"message":n.message})).collect::<Vec<_>>(),"blocks":plan.blocks.len(),"skipped_air":plan.skipped_air,"size":plan.size,"data_version":schematic.data_version});
    if let Some(origin) = origin {
        let writes = match destination(game, &plan, origin) {
            Ok(writes) => writes,
            Err(e) => {
                reply(game, e);
                return;
            }
        };
        let dimension = game.dim.id().to_owned();
        let mut applied = 0;
        for (index, write) in writes.iter().enumerate() {
            // SchematicPrinter.shouldPlaceBlock skips an identical existing
            // state (our supported BEs have no merge-on-paste behavior).
            if write.old == write.state {
                continue;
            }
            game.native_set_block(&dimension, write.pos, write.state, 18);
            if game.world.block(write.pos[0], write.pos[1], write.pos[2]) != write.state {
                for old in writes.iter().take(index + 1).rev() {
                    game.native_set_block(&dimension, old.pos, old.old, 18);
                    if let Some(entity) = &old.old_entity {
                        game.world
                            .block_entities
                            .insert((old.pos[0], old.pos[1], old.pos[2]), entity.clone());
                    }
                    game.native_block_entity_sync(old.pos);
                }
                reply(
                    game,
                    failure(
                        "block placement was refused after preflight; affected block snapshots restored",
                    ),
                );
                return;
            }
            if let Some(nbt) = &write.nbt {
                game.load_block_entity_nbt(write.pos, nbt);
                game.native_block_entity_sync(write.pos);
            }
            applied += 1;
        }
        crate::game::create_port_kinetic_runtime::invalidate();
        report["placed"] = json!(applied);
    }
    reply(game, report);
}
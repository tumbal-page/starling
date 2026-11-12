// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Game-side adapter for Create mod. The engine crate contains the portable algorithms.
// Adapted for Android bundled port.

use super::{Game, create_engine};
use crate::game::native_block_entity_draw::BlockEntityDraw;
use create_engine::kinetics::Network;
use horizon_client::{
    containers::{BlockEntity, ModBlockEntity, ModContainer},
    items::ItemStack,
};
use super::create_engine::{
    kinetics::{Axis, Kind, Node},
    placement::{self, Direction},
    pressing::{Action, PressCycle},
};
use horizon_world::{
    nbt::{Compound, Tag},
    registry::BlockStateId,
};
use serde_json::Value;

static ORIGINAL_ASSETS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn original_assets() -> bool {
    ORIGINAL_ASSETS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Feed bundled content through Horizon's ordinary content loader, before it
/// registers blocks or announces mods. The native mod uses the same enable flag.
pub(crate) fn prepare_content(enabled: &std::collections::BTreeSet<String>) {
    if enabled.contains("create") {
        match super::assets::bundled_content(
            Game::game_dir(),
            b"",
        ) {
            Ok(path) => {
                log::info!("Create: bundled content enabled in {}", path.display());
            }
            Err(error) => log::error!("Create: bundled content could not be loaded: {error}"),
        }
    }
}

/// Imports the user's JAR before the initial load, and again on pack reload.
/// Uses Horizon's own pack loader/cache; the installed content archive is untouched.
pub(crate) fn asset_root(root: std::path::PathBuf) -> std::path::PathBuf {
    ORIGINAL_ASSETS.store(false, std::sync::atomic::Ordering::Relaxed);
    // Horizon consumes registry declarations while building the root; their
    // source data/*/horizon files are deliberately absent from that overlay.
    if !root
        .join("data/create/recipe/pressing/iron_ingot.json")
        .is_file()
        || !root.join("assets/create/items/shaft.json").is_file()
    {
        return root;
    }
    let home = Game::game_dir();
    let result = (|| -> std::io::Result<Option<std::path::PathBuf>> {
        let Some(imported) = super::assets::prepare(
            &home,
            horizon_client::packs::RESOURCE_FORMAT,
        )?
        else {
            log::warn!(
                "Create: using provisional resources. Place the original Create {} JAR in {}",
                super::assets::CREATE_VERSION,
                home.join(super::assets::INPUT_DIR).display()
            );
            return Ok(None);
        };
        let packs = horizon_client::packs::folder(
            imported.pack.parent().expect("pack parent"),
            horizon_client::packs::RESOURCE_FORMAT,
        );
        if packs.len() != 1 {
            return Err(std::io::Error::other(
                "The locally imported Create resource pack could not be read",
            ));
        }
        let overlay = horizon_client::packs::resource_root(
            &root,
            &packs,
            &home.join(".horizon-cache/create-assets/overlay"),
        )?;
        log::info!(
            "Create: {} local resources from Create {} ({}); JAR and installed mod unchanged",
            imported.files,
            super::assets::CREATE_VERSION,
            if imported.cached {
                "cache verified"
            } else {
                "imported"
            }
        );
        Ok(Some(overlay))
    })();
    match result {
        Ok(Some(overlay)) => {
            ORIGINAL_ASSETS.store(true, std::sync::atomic::Ordering::Relaxed);
            overlay
        }
        Ok(None) => root,
        Err(error) => {
            log::warn!(
                "Create: local resource import failed; using provisional resources: {error}"
            );
            root
        }
    }
}

pub(super) fn owns(block: &str) -> bool {
    matches!(
        block,
        "create:creative_motor"
            | "create:shaft"
            | "create:cogwheel"
            | "create:large_cogwheel"
            | "create:mechanical_press"
            | "create:depot"
            | "create:gearbox"
            | "create:clutch"
            | "create:gearshift"
            | "create:hand_crank"
            | "create:andesite_encased_shaft"
            | "create:brass_encased_shaft"
            | "create:encased_chain_drive"
            | "create:speedometer"
            | "create:stressometer"
            | "create:water_wheel"
            | "create:large_water_wheel"
            | "create:water_wheel_structure"
            | "create:millstone"
            | "create:encased_fan"
            | "create:mechanical_drill"
            | "create:basin"
            | "create:mechanical_mixer"
            | "create:blaze_burner"
            | "create:belt"
    )
}

pub(crate) fn entity(block: &str, entity: &mut Option<(String, Compound)>) {
    if owns(block) && block != "create:blaze_burner" && entity.is_none() {
        let id = match block {
            "create:creative_motor" => "create:motor",
            "create:mechanical_drill" => "create:drill",
            "create:shaft" | "create:cogwheel" | "create:large_cogwheel" => "create:simple_kinetic",
            "create:andesite_encased_shaft"
            | "create:brass_encased_shaft"
            | "create:encased_chain_drive" => "create:encased_shaft",
            _ => block,
        };
        let mut nbt = Compound::new();
        if block == "create:creative_motor" {
            nbt.insert("ScrollValue".into(), Tag::Int(16));
        }
        *entity = Some((id.to_owned(), nbt));
    }
}

pub(crate) fn ticks(block: &str, ticks: &mut bool) {
    if owns(block) {
        *ticks = true;
    }
}

pub(super) fn number(nbt: &Compound, key: &str) -> f32 {
    match nbt.get(&key.into()) {
        Some(Tag::Float(n)) => f32::from_bits(*n),
        Some(Tag::Int(n)) => *n as f32,
        Some(Tag::Long(n)) => *n as f32,
        _ => 0.0,
    }
}

/// KineticBlockEntity.getSpeed: debug stepping a frozen world does not expose
/// theoretical network RPM to a machine. Keep the stored Speed/port diagnostics
/// intact so unfreezing resumes the same network instead of propagating zero.
pub(super) fn running_speed(game: &Game<'_>, nbt: &Compound) -> f32 {
    if game.clock.frozen() {
        0.0
    } else {
        number(nbt, "Speed")
    }
}

pub(super) fn integer(nbt: &Compound, key: &str) -> i32 {
    match nbt.get(&key.into()) {
        Some(Tag::Int(n)) => *n,
        Some(Tag::Byte(n)) => i32::from(*n),
        _ => 0,
    }
}

/// ItemStack.getMaxStackSize after the component patch. Persistent numeric
/// values use Number.intValue; removing the prototype component defaults to1.
pub(super) fn stack_maximum_override(stack: &ItemStack) -> Option<u32> {
    let data = stack.data()?;
    if data.other("!minecraft:max_stack_size").is_some() {
        return Some(1);
    }
    let value = match data.other("minecraft:max_stack_size")? {
        Tag::Byte(v) => i32::from(*v),
        Tag::Short(v) => i32::from(*v),
        Tag::Int(v) => *v,
        Tag::Long(v) => *v as i32,
        Tag::Float(v) => f32::from_bits(*v) as i32,
        Tag::Double(v) => f64::from_bits(*v) as i32,
        _ => return None,
    };
    (1..=99).contains(&value).then_some(value as u32)
}

pub(super) fn stack_maximum(game: &Game<'_>, stack: &ItemStack) -> u32 {
    stack_maximum_override(stack)
        .unwrap_or_else(|| game.native_max_stack_size(&stack.item).unwrap_or(64))
}

thread_local! {
    static PLACEMENT_HIT: std::cell::Cell<Option<([i32; 3], [i32; 3])>> = const { std::cell::Cell::new(None) };
}

pub(super) fn use_on(game: &Game<'_>, stack: &ItemStack, pos: [i32; 3], face: [i32; 3]) {
    PLACEMENT_HIT.set((game.native_on("create") && owns(&stack.item)).then_some((pos, face)));
}

// Direction.orderedByNearest, adapted from Horizon's MIT level/place.rs.
// Uses the engine's vanilla-compatible trigonometric table.
pub(super) fn nearest(yaw: f32, pitch: f32) -> Direction {
    use horizon_core::geometry::trig;
    let pitch = pitch * (std::f32::consts::PI / 180.0);
    let yaw = -yaw * (std::f32::consts::PI / 180.0);
    let (ps, pc) = (trig::sin(f64::from(pitch)), trig::cos(f64::from(pitch)));
    let (ys, yc) = (trig::sin(f64::from(yaw)), trig::cos(f64::from(yaw)));
    let (x, y, z) = (ys.abs(), ps.abs(), yc.abs());
    let ax = if ys > 0.0 {
        Direction::East
    } else {
        Direction::West
    };
    let ay = if ps < 0.0 {
        Direction::Up
    } else {
        Direction::Down
    };
    let az = if yc > 0.0 {
        Direction::South
    } else {
        Direction::North
    };
    if x > z {
        if y > x * pc { ay } else { ax }
    } else if y > z * pc {
        ay
    } else {
        az
    }
}

pub(super) fn placed(game: &mut Game<'_>, stack: &ItemStack, pos: [i32; 3]) {
    let Some((clicked, face)) = PLACEMENT_HIT.take() else {
        return;
    };
    if !owns(&stack.item) {
        return;
    }
    let Some(face) = Direction::from_vector(face) else {
        return;
    };
    let ports = Direction::ALL.map(|d| {
        let offset = d.vector();
        node(game, std::array::from_fn(|i| pos[i] + offset[i]))
            .is_some_and(|n| n.shaft_port(offset.map(|v| -v)))
    });
    let sneaking = game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
    let look = nearest(game.yaw, game.pitch);
    let axis = match stack.item.as_ref() {
        "create:shaft"
        | "create:andesite_encased_shaft"
        | "create:brass_encased_shaft"
        | "create:clutch"
        | "create:gearshift" => Some(placement::shaft(look, face, sneaking, ports)),
        "create:cogwheel" | "create:large_cogwheel" => {
            let against = if game.native_block(clicked) == "create:cogwheel" {
                node(game, clicked).map(|n| n.axis)
            } else {
                None
            };
            Some(placement::cog(face, sneaking, ports, against))
        }
        _ => None,
    };
    if let Some(axis) = axis {
        game.native_set_block_property(
            pos,
            "axis",
            match axis {
                Axis::X => "x",
                Axis::Y => "y",
                Axis::Z => "z",
            },
        );
    } else {
        let facing = match stack.item.as_ref() {
            "create:creative_motor" => Some(placement::motor(look, sneaking, ports)),
            "create:hand_crank" => Some(placement::crank(face, sneaking, ports)),
            "create:mechanical_press" => Some(
                [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ][((f64::from(game.yaw) / 90.0 + 0.5).floor() as i64 & 3) as usize],
            ),
            _ => None,
        };
        if let Some(facing) = facing {
            game.native_set_block_property(pos, "facing", facing.name());
        }
    }
}

/// Ordinary native inventories support only their pinned canonical sizes.
/// An explicitly edited different handler size stays opaque until restored.
pub(super) fn inventory_supported(entity: &ModBlockEntity) -> bool {
    let inventories: &[(&str, i32)] = match entity.id.as_str() {
        "create:depot" => &[("OutputBuffer", 8)],
        "create:basin" => &[("InputItems", 9), ("OutputItems", 9)],
        "create:millstone" => &[("InputInventory", 1), ("OutputInventory", 9)],
        _ => return true,
    };
    inventories.iter().all(|&(key, count)| {
        !matches!(entity.nbt.get(&key.into()).and_then(|v| v.get("Size")),
            Some(Tag::Int(size)) if *size != count)
    })
}

pub(super) fn inventory_blocked(game: &Game<'_>, pos: [i32; 3]) -> bool {
    matches!(game.world.block_entities.get(&(pos[0],pos[1],pos[2])),
        Some(BlockEntity::Mod(entity)) if !inventory_supported(entity))
}

pub(super) fn depot_reacts(game: &Game<'_>, pos: [i32; 3], face: [i32; 3]) -> bool {
    if inventory_blocked(game, pos) {
        return false;
    }
    if crate::game::create_port_burner::reacts(game, pos) {
        return true;
    }
    if crate::game::create_port_basin::reacts(game, pos) {
        return true;
    }
    if crate::game::create_port_millstone::reacts(game, pos) {
        return true;
    }
    if crate::game::create_port_interactions::reacts(game, pos) {
        return true;
    }
    if !game.native_on("create") || game.native_block(pos) != "create:depot" || face != [0, 1, 0] {
        return false;
    }
    if game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft)
        && (game
            .inventory
            .get(game.selected)
            .is_some_and(Option::is_some)
            || game.offhand.is_some())
    {
        return false;
    }
    true
}

pub(super) fn right_click(game: &mut Game<'_>, pos: [i32; 3], face: [i32; 3], canceled: &mut bool) {
    if inventory_blocked(game, pos) {
        *canceled = true;
        return;
    }
    if crate::game::create_port_burner_interactions::right_click(game, pos) {
        *canceled = true;
        game.swing_arm();
        return;
    }
    if crate::game::create_port_burner::right_click(game, pos) {
        *canceled = true;
        game.swing_arm();
        return;
    }
    if crate::game::create_port_basin::right_click(game, pos) {
        *canceled = true;
        game.swing_arm();
        return;
    }
    if crate::game::create_port_belt::right_click(game, pos, face) {
        *canceled = true;
        game.swing_arm();
        return;
    }
    if crate::game::create_port_interactions::right_click(game, pos, face) {
        game.swing_arm();
        *canceled = true;
        return;
    }
    if crate::game::create_port_millstone::right_click(game, pos) {
        *canceled = true;
        game.swing_arm();
        return;
    }
    if !depot_reacts(game, pos, face) {
        return;
    }
    let held = game.held();
    let put = held.is_some_and(|s| s.count > 0 && s.item.as_ref() != "create:mechanical_arm");
    let Some(taken) = game.native_with_mod_block_entity(pos, |e| {
        let taken = e
            .items
            .iter_mut()
            .filter_map(Option::take)
            .collect::<Vec<_>>();
        e.nbt.remove(&"HeldItem".into());
        taken
    }) else {
        return;
    };
    let uuid = game.native_acting_id();
    if !taken.is_empty() {
        crate::game::create_port_sounds::pickup(game, pos);
    }
    for stack in taken {
        if let Some(rest) = game.native_give_stack(uuid, stack)
            && let Some(player) = game.native_player()
        {
            game.spawn_item(
                rest,
                [player.x, player.y + 1.0, player.z],
                [0.0, 0.2, 0.0],
                10,
            );
        }
    }
    // Read the hand after returning the old stack: Inventory.add may have
    // merged equal items into it. Creative players also empty their hand.
    if put {
        crate::game::create_port_sounds::depot(game, pos);
        let incoming = game.held_mut().take();
        game.native_with_mod_block_entity(pos, |e| {
            e.items[0] = incoming;
        });
    }
    game.native_with_mod_block_entity(pos, |e| {
        depot_data(e);
    });
    game.native_block_entity_sync(pos);
    game.swing_arm();
    *canceled = true;
}

/// Match DepotBehaviour/TransportedItemStack's public data names. Horizon also
/// saves its generic Items slots; Create ignores that additional container field.
fn depot_field(fields: &mut Compound, key: &str, value: Tag) -> bool {
    if fields.get(&key.into()) == Some(&value) {
        false
    } else {
        fields.insert(key.into(), value);
        true
    }
}

pub(super) fn depot_data(entity: &mut ModBlockEntity) -> bool {
    if !inventory_supported(entity) {
        return false;
    }
    let mut changed = false;
    if let Some(item) = entity
        .items
        .first()
        .and_then(Option::as_ref)
        .map(horizon_items::item_nbt::item_to_nbt)
    {
        let key: horizon_world::nbt::JavaString = "HeldItem".into();
        if !matches!(entity.nbt.get(&key), Some(Tag::Compound(_))) {
            entity.nbt.insert(
                key.clone(),
                Tag::Compound(Compound::from([
                    ("Pos".into(), Tag::Float(0.25f32.to_bits())),
                    ("PrevPos".into(), Tag::Float(0.25f32.to_bits())),
                    ("Offset".into(), Tag::Float(0.0f32.to_bits())),
                    ("PrevOffset".into(), Tag::Float(0.0f32.to_bits())),
                    ("InSegment".into(), Tag::Int(0)),
                    ("Angle".into(), Tag::Int(0)),
                    ("InDirection".into(), Tag::Int(3)),
                ])),
            );
            changed = true;
        }
        if let Some(Tag::Compound(held)) = entity.nbt.get_mut(&key) {
            changed |= depot_field(held, "Item", item);
        }
    } else {
        changed |= entity.nbt.remove(&"HeldItem".into()).is_some();
    }
    let buffer = entity
        .items
        .iter()
        .skip(1)
        .take(8)
        .filter_map(Option::as_ref)
        .map(horizon_items::item_nbt::item_to_nbt)
        .collect::<Vec<_>>();
    let key: horizon_world::nbt::JavaString = "OutputBuffer".into();
    if !matches!(entity.nbt.get(&key), Some(Tag::List(_))) {
        entity.nbt.insert(key.clone(), Tag::List(buffer.clone()));
        changed = true;
    }
    changed
}

pub(super) fn node(game: &Game<'_>, pos: [i32; 3]) -> Option<Node> {
    let key = (pos[0], pos[1], pos[2]);
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&key) else {
        return None;
    };
    if entity.id.is_empty() {
        return None;
    }
    entity.nbt.get(&"Network".into()).and_then(|n| {
        let data = match n {
            horizon_world::nbt::Tag::Compound(data) => data,
            _ => return None,
        };
        let kind = match data.get(&"Kind".into()) {
            Some(horizon_world::nbt::Tag::Byte(v)) => Kind::from_name(*v as u8)?,
            _ => return None,
        };
        let speed = match data.get(&"Speed".into()) {
            Some(horizon_world::nbt::Tag::Float(v)) => *v,
            _ => return None,
        };
        Some(Node { pos, axis: super::create_engine::kinetics::Axis::Y, kind, speed: f32::from_bits(speed) })
    })
}

pub(crate) fn container(block: &str, container: &mut Option<ModContainer>) {
    if !owns(block) {
        return;
    }
    let (id, slots) = match block {
        "create:depot" => ("create:depot", 9),
        "create:basin" => ("create:basin", 18),
        "create:millstone" => ("create:millstone", 10),
        "create:mechanical_mixer" => ("create:mechanical_mixer", 10),
        "create:blaze_burner" => ("create:blaze_burner", 10),
        _ => return,
    };
    *container = Some(ModContainer {
        slots,
        faces: Default::default(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        menu: None,
    }); // create port stub: ModContainer::new not available on host
}

pub(crate) fn update_tag(entity: &ModBlockEntity, tag: &mut Option<Compound>) {
    if entity.id.starts_with("create:") {
        let mut t = entity.nbt.clone();
        if let Some(Tag::Compound(held)) = t.get_mut(&"HeldItem".into()) {
            held.remove(&"Pos".into());
            held.remove(&"PrevPos".into());
            held.remove(&"Offset".into());
            held.remove(&"PrevOffset".into());
            held.remove(&"InSegment".into());
            held.remove(&"Angle".into());
            held.remove(&"InDirection".into());
        }
        *tag = Some(t);
    }
}

pub(crate) fn tick(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    if !entity.id.starts_with("create:") {
        return;
    }
    // Delegate to specific module tick functions
    match entity.id.as_str() {
        "create:basin" => { let _ = crate::game::create_port_basin::tick(game, pos, entity); }
        "create:mechanical_mixer" => { let _ = crate::game::create_port_mixer::tick(game, pos, entity, 0.0); }
        "create:encased_fan" => { let _ = crate::game::create_port_fan::tick(game, pos, entity, 0.0); }
        "create:mechanical_drill" => { let _ = crate::game::create_port_drill::tick(game, pos, entity, 0.0); }
        "create:mechanical_press" => { let _ = crate::game::create_port_belt_press::tick(game, pos, entity); }
        "create:millstone" => { let _ = crate::game::create_port_millstone::tick(game, pos, entity, 0.0); }
        "create:blaze_burner" => { let _ = crate::game::create_port_burner::tick(game, pos, entity); }
        "create:depot" => { let _ = crate::game::create_port_basin::depot_tick(game, pos, entity); }
        "create:creative_motor" => { let _ = crate::game::create_port_motor_control::tick(game, pos, entity); }
        _ => {}
    }
}

pub(super) fn ingredient(_item: &serde_json::Value) -> Option<serde_json::Value> {
    None
}

pub(super) fn network_matches(
    _entity: &ModBlockEntity,
    _network: &Network,
    _other: Option<&str>,
    _pos: [i32; 3],
) -> bool {
    false
}

pub(super) fn apply_network(
    _entity: &mut ModBlockEntity,
    _network: &Network,
    _other: Option<&str>,
    _pos: [i32; 3],
) {
}

pub(super) fn face_south(_draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw, _facing: super::create_engine::placement::Direction) {}

pub(super) fn result(_game: &mut Game<'_>, _stack: &ItemStack) -> Option<ItemStack> {
    None
}
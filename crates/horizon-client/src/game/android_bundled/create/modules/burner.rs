// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// BlazeBurnerBlock / BlazeBurnerBlockEntity, pinned Create6.0.10.
use super::{Game, GameLevel};
use crate::{
    containers::{BlockEntity, ModBlockEntity},
    items::ItemStack,
};
use create_engine::burner::{Burner, Candidate, Fuel, Heat};
use horizon_core::random::RandomSource;
use horizon_world::{
    nbt::{Compound, Tag},
    registry::BlockStateId,
};
type Pos = [i32; 3];
pub(super) const BLOCK: &str = "create:blaze_burner";
pub(super) const ENTITY: &str = "create:blaze_heater";
const CREATIVE: &str = "create:creative_blaze_cake";
const FIELDS: [&str; 5] = [
    "fuelLevel",
    "burnTimeRemaining",
    "isCreative",
    "Goggles",
    "TrainHat",
];

pub(super) fn heat(name: &str) -> Option<Heat> {
    Some(match name {
        "none" => Heat::None,
        "smouldering" => Heat::Smouldering,
        "fading" => Heat::Fading,
        "kindled" => Heat::Kindled,
        "seething" => Heat::Seething,
        _ => return None,
    })
}
fn heat_name(heat: Heat) -> &'static str {
    match heat {
        Heat::None => "none",
        Heat::Smouldering => "smouldering",
        Heat::Fading => "fading",
        Heat::Kindled => "kindled",
        Heat::Seething => "seething",
    }
}
pub(super) fn stateful(block: &str, owns: &mut bool) {
    if block == BLOCK {
        *owns = true;
    }
}
pub(super) fn entity(
    block: &str,
    properties: &crate::mod_api::BlockProperties,
    out: &mut Option<(String, Compound)>,
) {
    if out.is_some()
        || block != BLOCK
        || properties
            .get("blaze")
            .and_then(|v| heat(v))
            .is_none_or(|v| v == Heat::None)
    {
        return;
    }
    let mut nbt = Compound::new();
    store(&mut nbt, Burner::default());
    *out = Some((ENTITY.into(), nbt));
}

fn load(nbt: &Compound, heat: Heat) -> Option<Burner> {
    let number = |k| super::create_port::integer(nbt, k);
    Some(Burner {
        fuel: match number("fuelLevel") {
            0 => Fuel::None,
            1 => Fuel::Normal,
            2 => Fuel::Special,
            _ => return None,
        },
        remaining: number("burnTimeRemaining"),
        creative: number("isCreative") != 0,
        heat,
    })
}
fn store(nbt: &mut Compound, state: Burner) {
    if state.creative {
        nbt.remove(&"fuelLevel".into());
        nbt.remove(&"burnTimeRemaining".into());
        nbt.insert("isCreative".into(), Tag::Byte(1));
    } else {
        nbt.remove(&"isCreative".into());
        nbt.insert("fuelLevel".into(), Tag::Int(state.fuel as i32));
        nbt.insert("burnTimeRemaining".into(), Tag::Int(state.remaining));
    }
}
fn current(game: &Game<'_>, pos: Pos) -> Option<Burner> {
    if game.native_block(pos) != BLOCK {
        return None;
    }
    let heat = heat(&game.native_block_property(pos, "blaze")?)?;
    let BlockEntity::Mod(entity) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))? else {
        return None;
    };
    if entity.id != ENTITY {
        return None;
    }
    load(&entity.nbt, heat)
}
fn candidate(game: &Game<'_>, stack: &ItemStack) -> Candidate {
    if game.remote.is_some() {
        // NeoForge synchronizes these merged server definitions. The native
        // client waits for the complete world snapshot rather than guessing
        // from its local resource pack or duplicating a furnace fuel table.
        return super::create_port_burner_sync::candidate(game, stack).unwrap_or(Candidate {
            fuel: Fuel::None,
            burn_time: 0,
        });
    }
    let data = |id| {
        game.native_data_map("minecraft:item", id, &stack.item)
            .and_then(|v| v.as_i64().or_else(|| v["burn_time"].as_i64()))
            .and_then(|v| i32::try_from(v).ok())
            .filter(|v| *v > 0)
    };
    let tag = |name| game.native_server_item_in_tag(stack, name).unwrap_or(false);
    Candidate::resolve(
        data("create:superheated_blaze_burner_fuels"),
        data("create:regular_blaze_burner_fuels"),
        tag("create:blaze_burner_fuel/special"),
        game.res.providers().burn_time(game.items(), stack, false),
        tag("create:blaze_burner_fuel/regular"),
    )
}
fn set_heat(game: &mut Game<'_>, pos: Pos, heat: Heat) {
    if game.native_set_block_property(pos, "blaze", heat_name(heat)) {
        game.native_block_entity_sync(pos);
    }
}
pub(super) fn tick(game: &mut Game<'_>, pos: Pos, entity: &mut ModBlockEntity) {
    if entity.id != ENTITY {
        return;
    }
    let Some(heat) = game
        .native_block_property(pos, "blaze")
        .and_then(|v| heat(&v))
    else {
        return;
    };
    let Some(mut state) = load(&entity.nbt, heat) else {
        return;
    };
    state.tick(|heat| set_heat(game, pos, heat));
    store(&mut entity.nbt, state);
}
fn play(game: &mut Game<'_>, pos: Pos, sound: &str, ambient: bool) {
    let volume = 0.125 + game.world.random.next_float() * 0.125;
    let pitch = if ambient { 1.15 } else { 0.75 } - game.world.random.next_float() * 0.25;
    game.with_level(|_, level, _| {
        level.native_play_sound(sound, pos.map(|v| f64::from(v) + 0.5), 4, volume, pitch);
    });
}
pub(super) fn reacts(game: &Game<'_>, pos: Pos) -> bool {
    if !game.native_on("create")
        || game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft)
            && (game
                .inventory
                .get(game.selected)
                .is_some_and(Option::is_some)
                || game.offhand.is_some())
    {
        return false;
    }
    let Some(mut state) = current(game, pos) else {
        return false;
    };
    if state.heat == Heat::None {
        return false;
    }
    let Some(stack) = game.held().filter(|s| s.count > 0) else {
        return matches!(game.world.block_entities.get(&(pos[0],pos[1],pos[2])),Some(BlockEntity::Mod(e)) if e.nbt.contains_key(&"Goggles".into()));
    };
    if stack.item.as_ref() == CREATIVE {
        return true;
    }
    if stack.item.as_ref() == "create:goggles" {
        return matches!(game.world.block_entities.get(&(pos[0],pos[1],pos[2])),Some(BlockEntity::Mod(e)) if !e.nbt.contains_key(&"Goggles".into()));
    }
    state.insert(candidate(game, stack), true, true, |_| {})
}

/// Source client useItemOn: predict fuel storage and the particle burst, while
/// leaving inventory and block heat to the authoritative server. In particular,
/// applyCreativeFuel chooses the burst BEFORE the server skips FADING.
pub(super) fn predict_client(game: &mut Game<'_>, pos: Pos) -> bool {
    if game.remote.is_none() || !reacts(game, pos) {
        return false;
    }
    let held = game.held().filter(|s| s.count > 0).cloned();
    if held
        .as_ref()
        .is_none_or(|s| s.item.as_ref() == "create:goggles")
    {
        game.native_with_mod_block_entity(pos, |entity| {
            if held.is_some() {
                entity.nbt.insert("Goggles".into(), Tag::Byte(1));
            } else {
                entity.nbt.remove(&"Goggles".into());
            }
        });
        return true;
    }
    let held = held.unwrap();
    let Some(mut state) = current(game, pos) else {
        return false;
    };
    let soul = if held.item.as_ref() == CREATIVE {
        state.fuel = Fuel::None;
        state.remaining = 0;
        state.creative = true;
        state.heat.next_active() >= Heat::Seething
    } else {
        if !state.insert(candidate(game, &held), true, false, |_| {}) {
            return false;
        }
        state.fuel == Fuel::Special
    };
    game.native_with_mod_block_entity(pos, |entity| store(&mut entity.nbt, state));
    super::create_port_burner_presentation::burst(game, pos, soul);
    true
}
fn give(game: &mut Game<'_>, stack: ItemStack) {
    let id = game.native_acting_id();
    if let Some(rest) = game.native_give_stack(id, stack)
        && let Some(eye) = game.eye_position()
    {
        game.throw_from_eyes(rest, eye, false);
    }
}
pub(super) fn right_click(game: &mut Game<'_>, pos: Pos) -> bool {
    if !reacts(game, pos) {
        return false;
    }
    let held = game.held().filter(|s| s.count > 0).cloned();
    if held
        .as_ref()
        .is_none_or(|s| s.item.as_ref() == "create:goggles")
    {
        game.native_with_mod_block_entity(pos, |e| {
            if held.is_some() {
                e.nbt.insert("Goggles".into(), Tag::Byte(1));
            } else {
                e.nbt.remove(&"Goggles".into());
            }
        });
        game.native_block_entity_sync(pos);
        return true;
    }
    let held = held.unwrap();
    let Some(mut state) = current(game, pos) else {
        return false;
    };
    let old = state.heat;
    let creative = held.item.as_ref() == CREATIVE;
    if creative {
        // Source sound draws precede the block write.
        play(game, pos, "minecraft:entity.blaze.shoot", false);
        state.apply_creative(|heat| set_heat(game, pos, heat));
    } else {
        if !state.insert(candidate(game, &held), true, false, |_| {}) {
            return false;
        }
        play(game, pos, "minecraft:entity.blaze.shoot", false);
        set_heat(game, pos, state.heat);
        if old != state.heat {
            play(game, pos, "minecraft:entity.blaze.ambient", true);
        }
    }
    game.native_with_mod_block_entity(pos, |e| store(&mut e.nbt, state));
    // Always commit fuel storage; source notifies clients when heat changes.
    if old != state.heat {
        game.native_block_entity_sync(pos);
    }
    if !creative && !game.creative {
        let remainder = game
            .items()
            .get(&held.item)
            .and_then(|i| i.crafting_remainder.as_deref())
            .map(|id| ItemStack::new(id, 1));
        game.consume_held();
        if let Some(remainder) = remainder {
            if game.held().is_none_or(|s| s.count == 0) {
                *game.held_mut() = Some(remainder);
            } else {
                give(game, remainder);
            }
        }
    }
    true
}
pub(super) fn analog(game: &GameLevel<'_>, state: BlockStateId, out: &mut i32) {
    if game.native_state_block(state) == Some(BLOCK) {
        *out = game
            .registry
            .state(state)
            .and_then(|s| s.properties.get("blaze"))
            .and_then(|v| heat(v))
            .map_or(0, |h| i32::from(h.comparator()));
    }
}
/// Pending client load-tag boundary: raw adapter uses the exact validated
/// type/block/compound guards of Game::remote_block_entity_data. Java read
/// resets absent canonical fields, whereas Horizon's default merge retains them.
pub(super) fn packet(game: &mut Game<'_>, pos: Pos, kind: i32, tag: &Tag) {
    let Tag::Compound(fields) = tag else {
        return;
    };
    let Some(name) = game
        .remote
        .as_ref()
        .and_then(|r| r.builtin.get("minecraft:block_entity_type")?.name(kind))
    else {
        return;
    };
    if *game
        .res
        .facts()
        .get(game.world.block(pos[0], pos[1], pos[2]))
        .name
        != *name
        || name != BLOCK
    {
        return;
    }
    let Some(BlockEntity::Mod(entity)) =
        game.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    if entity.id != ENTITY {
        return;
    }
    for field in FIELDS {
        if !fields.contains_key(&field.into()) {
            entity.nbt.remove(&field.into());
        }
    }
}

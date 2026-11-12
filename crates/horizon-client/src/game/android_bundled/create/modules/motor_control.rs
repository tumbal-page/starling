// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Authoritative KineticScrollValueBehaviour settings. Client UI lives separately.
// Adapted for Android bundled port.

use super::Game;
use horizon_client::containers::BlockEntity;
use horizon_world::nbt::Tag;
use std::{cell::RefCell, path::PathBuf};

struct Request {
    player: u128,
    world: PathBuf,
    dimension: String,
    pos: [i32; 3],
    row: i32,
    raw: i32,
}
thread_local! {
    static REQUESTS: RefCell<Vec<Request>> = const { RefCell::new(Vec::new()) };
}
pub(super) fn reset() {
    REQUESTS.with_borrow_mut(Vec::clear);
}
pub(super) fn receive(game: &mut Game<'_>, player: u128, channel: &str, data: &[u8]) {
    if !game.native_on("create") {
        return;
    }
    let fields = match (channel, data.len()) {
        ("create:motor_settings_v2", 20) => 5,
        ("create:motor_speed", 16) => 4, // accepted for older diagnostic clients
        _ => return,
    };
    let word =
        |i: usize| i32::from_be_bytes(data[i * 4..i * 4 + 4].try_into().expect("bounded word"));
    let pos = [word(0), word(1), word(2)];
    let (row, raw) = if fields == 5 {
        (word(3), word(4))
    } else {
        let value = word(3);
        if value == 0 || !(-256..=256).contains(&value) {
            return;
        }
        (i32::from(value >= 0), value.abs())
    };
    if !(0..=1).contains(&row) || !(0..=256).contains(&raw) {
        return;
    }
    let Some(world) = game.native_world_dir() else {
        return;
    };
    let Some(p) = game
        .native_server_players()
        .into_iter()
        .find(|p| p.uuid == player)
    else {
        return;
    };
    if !game.create_motor_configuration_allowed(player, pos) {
        return;
    }
    REQUESTS.with_borrow_mut(|requests| {
        if requests.len() < 256 {
            // Preserve packet order: each distinct raw setting has source feedback.
            requests.push(Request {
                player,
                world,
                dimension: p.dimension,
                pos,
                row,
                raw,
            });
        }
    });
}
pub(super) fn level_tick(game: &mut Game<'_>) {
    let dim = game.dim.id().to_owned();
    let world = game.native_world_dir();
    let players = game.native_server_players();
    let requests = REQUESTS.with_borrow_mut(|pending| {
        let mut selected = Vec::new();
        pending.retain(|r| {
            if world.as_ref() != Some(&r.world)
                || !players
                    .iter()
                    .any(|p| p.uuid == r.player && p.dimension == r.dimension)
            {
                false
            } else if r.dimension == dim {
                selected.push((r.player, r.pos, r.row, r.raw));
                false
            } else {
                true
            }
        });
        selected
    });
    for (player, pos, row, raw) in requests {
        if !players
            .iter()
            .any(|p| p.uuid == player && p.dimension == dim)
        {
            continue;
        };
        if !game.create_motor_configuration_allowed(player, pos)
            || !game.world.loaded(pos[0], pos[2])
            || game.native_block(pos) != "create:creative_motor"
        {
            continue;
        }
        let Some(BlockEntity::Mod(entity)) =
            game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
        else {
            continue;
        };
        let old = match entity.nbt.get(&"ScrollValue".into()) {
            Some(Tag::Int(value)) => *value,
            _ => 16,
        };
        if row != i32::from(old >= 0) || raw != old.saturating_abs() {
            let at = pos.map(|v| f64::from(v) + 0.5);
            game.with_level(|_, level, _| {
                level.native_play_sound("minecraft:entity.item_frame.add_item", at, 4, 0.25, 2.0);
                level.native_play_sound(
                    "minecraft:block.note_block.iron_xylophone",
                    at,
                    4,
                    0.03,
                    1.125,
                );
            });
        }
        let value = raw.max(1) * if row == 0 { -1 } else { 1 };
        if value != old {
            game.native_with_mod_block_entity(pos, |e| {
                e.nbt.insert("ScrollValue".into(), Tag::Int(value));
            });
            // ScrollValueBehaviour.setValue callback, unlike NBT read.
            crate::game::create_port_kinetic_runtime::generated_changed(game, pos);
            game.native_block_entity_sync(pos);
        }
    }
}

pub(super) fn tick(_game: &mut Game<'_>, _pos: [i32; 3], _entity: &mut horizon_client::containers::ModBlockEntity) {}
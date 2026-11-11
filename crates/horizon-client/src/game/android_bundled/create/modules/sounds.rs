// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Selected AllSoundEvents composite registrations. Their wrapped event gains
// and pitches apply on top of each call's gain and pitch.
// Adapted for Android bundled port.

use super::Game;

fn event(game: &mut Game<'_>, pos: [i32; 3], sound: &str, volume: f32, pitch: f32) {
    let at = pos.map(|v| f64::from(v) + 0.5);
    game.with_level(|_, level, _| {
        level.native_play_sound(sound, at, 4, volume, pitch);
    });
    if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
        log::info!("Create sound: {sound} {pos:?} volume={volume} pitch={pitch}");
    }
}

pub(super) fn press(game: &mut Game<'_>, pos: [i32; 3], speed: f32) {
    if game.native_block([pos[0], pos[1] - 2, pos[2]]) == "create:belt" {
        event(game, pos, "minecraft:block.wool.hit", 0.75, 1.0);
        event(game, pos, "minecraft:entity.item.break", 0.15, 0.75);
        return;
    }
    let pitch = 0.75 + speed.abs() / 1024.0;
    event(
        game,
        pos,
        "create:mechanical_press_activation",
        0.125 * 0.5,
        pitch,
    );
    event(game, pos, "minecraft:entity.item.break", 0.5 * 0.5, pitch);
}

pub(super) fn depot(game: &mut Game<'_>, pos: [i32; 3]) {
    event(game, pos, "create:depot_slide", 0.125, 1.5);
}

pub(super) fn pickup(game: &mut Game<'_>, pos: [i32; 3]) {
    event(game, pos, "minecraft:entity.item.pickup", 0.2, 1.0);
}

pub(super) fn wrench(game: &mut Game<'_>, pos: [i32; 3], remove: bool) {
    if remove {
        event(game, pos, "create:wrench_remove", 0.25, 0.75);
        event(game, pos, "minecraft:block.netherite_block.hit", 0.25, 0.75);
    } else {
        event(game, pos, "create:wrench_rotate", 0.25, 1.25);
    }
}

pub(super) fn casing(game: &mut Game<'_>, pos: [i32; 3]) {
    event(game, pos, "minecraft:block.wood.place", 1.0, 0.8);
}

pub(super) fn land(game: &mut Game<'_>, pos: [i32; 3]) {
    event(game, pos, "create:depot_plop", 0.25, 1.25);
}
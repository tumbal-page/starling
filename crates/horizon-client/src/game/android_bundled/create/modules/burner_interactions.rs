// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create6.0.10 ac0c444: BlazeBurnerBlock/BlazeBurnerBlockItem/LitBlazeBurnerBlock.
use super::Game;
use crate::{containers::BlockEntity, items::ItemStack};
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_core::random::RandomSource;
use horizon_world::{nbt::Tag, player_inventory::EquipmentSlot};

type Pos = [i32; 3];
const BLOCK: &str = "create:blaze_burner";
const LIT: &str = "create:lit_blaze_burner";
const EMPTY: &str = "create:empty_blaze_burner";
const CAPTURABLE: &str = "create:blaze_burner_capturable";

#[derive(Clone, Copy)]
enum Action {
    Ignite,
    Extinguish,
    Soul,
}

/// Only the base clone ID; the caller retains its normal Ctrl-pick NBT path.
pub(super) fn clone_item(game: &Game<'_>, pos: Pos) -> Option<&'static str> {
    match game.native_block(pos).as_str() {
        LIT => Some(EMPTY),
        BLOCK => Some(
            if game.native_block_property(pos, "blaze").as_deref() == Some("none") {
                EMPTY
            } else {
                BLOCK
            },
        ),
        _ => None,
    }
}

fn action(game: &Game<'_>, pos: Pos, held: &ItemStack) -> Option<Action> {
    let bypass = game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft)
        && (game
            .inventory
            .get(game.selected)
            .is_some_and(Option::is_some)
            || game.offhand.is_some());
    if bypass {
        return None;
    }
    match game.native_block(pos).as_str() {
        BLOCK
            if game.native_block_property(pos, "blaze").as_deref() == Some("none")
                && held.item.as_ref() == "minecraft:flint_and_steel" =>
        {
            Some(Action::Ignite)
        }
        LIT if game
            .items()
            .get(&held.item)
            .is_some_and(|i| i.block_transformer.as_deref() == Some("minecraft:shovel")) =>
        {
            Some(Action::Extinguish)
        }
        LIT if game.native_block_property(pos, "flame_type").as_deref() == Some("regular")
            && game
                .items()
                .item_in_tag("minecraft:soul_fire_base_blocks", &held.item) =>
        {
            Some(Action::Soul)
        }
        _ => None,
    }
}
fn capturable(game: &Game<'_>, kind: &str) -> bool {
    let matches = |id: &str| {
        id.strip_prefix("minecraft:").unwrap_or(id)
            == kind.strip_prefix("minecraft:").unwrap_or(kind)
    };
    if game.remote.is_some() {
        super::create_port_burner_sync::capturable(game, kind).unwrap_or(false)
    } else {
        game.tag_entries("entity_type", CAPTURABLE)
            .iter()
            .any(|id| matches(id))
    }
}
fn tag_kind(tag: &horizon_world::nbt::Compound) -> Option<String> {
    match tag.get(&"id".into()) {
        Some(Tag::String(id)) => id.to_string().ok(),
        _ => None,
    }
}
fn spawner(game: &Game<'_>, pos: Pos) -> bool {
    let Some(BlockEntity::Spawner(spawner)) =
        game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return false;
    };
    if !spawner.potentials.is_empty() {
        return spawner
            .potentials
            .iter()
            .any(|(data, _)| tag_kind(&data.entity).is_some_and(|id| capturable(game, &id)));
    }
    let data = match spawner.next.as_ref() {
        Some(horizon_client::chunk::block_entity::Next::Own(data)) => Some(data),
        Some(horizon_client::chunk::block_entity::Next::Potential(index)) => {
            spawner.potentials.get(*index).map(|(data, _)| data)
        }
        None => None,
    };
    data.and_then(|data| tag_kind(&data.entity))
        .is_some_and(|id| capturable(game, &id))
}
fn capture_item(game: &mut Game<'_>) {
    if !game.creative {
        game.consume_held();
    }
    let filled = ItemStack::new(BLOCK, 1);
    if game.held().is_none_or(|s| s.count == 0) {
        *game.held_mut() = Some(filled);
    } else {
        let id = game.native_acting_id();
        if let Some(rest) = game.native_give_stack(id, filled)
            && let Some(eye) = game.eye_position()
        {
            game.throw_from_eyes(rest, eye, false);
        }
    }
}
fn capture_sound(game: &mut Game<'_>, at: [f64; 3]) {
    let at = at.map(|v| v.floor() + 0.5);
    game.with_level(|_, level, _| {
        level.native_play_sound("minecraft:entity.blaze.hurt", at, 5, 0.25, 0.75);
        level.native_play_sound("minecraft:block.fire.extinguish", at, 5, 0.5, 0.75);
    });
}
fn block_sound(game: &mut Game<'_>, pos: Pos, action: Action, client: bool) {
    let (sound, volume, pitch) = match action {
        Action::Extinguish => ("minecraft:entity.generic.extinguish_fire", 0.5, 2.0),
        Action::Ignite | Action::Soul => {
            let random = if client {
                game.particles.level_random()
            } else {
                &mut game.world.random
            };
            (
                if matches!(action, Action::Ignite) {
                    "minecraft:item.flintandsteel.use"
                } else {
                    "minecraft:block.soul_sand.place"
                },
                1.0,
                random.next_float() * 0.4 + 0.8,
            )
        }
    };
    // Existing guest_use_sounds excludes the predicting player for our two
    // hand_operated block IDs; the client plays this same event locally once.
    game.audio.play_on(
        4,
        sound,
        Some(pos.map(|v| f64::from(v) + 0.5)),
        volume,
        pitch,
    );
}

pub(super) fn reacts(game: &Game<'_>, pos: Pos, held: Option<&ItemStack>) -> bool {
    if !game.native_on("create") {
        return false;
    }
    held.is_some_and(|s| {
        s.count > 0
            && (action(game, pos, s).is_some() || s.item.as_ref() == EMPTY && spawner(game, pos))
    })
}
/// Run before the filled burner's fuel handler; source block writes follow sound/wear.
pub(super) fn right_click(game: &mut Game<'_>, pos: Pos) -> bool {
    let Some(held) = game.held().filter(|s| s.count > 0).cloned() else {
        return false;
    };
    if !reacts(game, pos, Some(&held)) {
        return false;
    }
    if held.item.as_ref() == EMPTY && spawner(game, pos) {
        capture_sound(game, pos.map(|v| f64::from(v) + 0.5));
        capture_item(game);
        return true;
    }
    let Some(action) = action(game, pos, &held) else {
        return false;
    };
    block_sound(game, pos, action, false);
    match action {
        Action::Ignite => {
            let slot = if game.hand == crate::game::Hand::Off {
                EquipmentSlot::OffHand
            } else {
                EquipmentSlot::MainHand
            };
            game.damage_held_in_slot(1, slot);
            if let Some(state) = game.pack.assets.registry.default_state(LIT) {
                game.native_set_block(game.dim.id(), pos, state, 3);
            }
        }
        Action::Extinguish => {
            game.damage_held_in_slot(1, EquipmentSlot::MainHand);
            if let Some(state) = game.pack.assets.registry.default_state(BLOCK) {
                game.native_set_block(game.dim.id(), pos, state, 3);
            }
        }
        Action::Soul => {
            game.native_set_block_property(pos, "flame_type", "soul");
        }
    }
    true
}
/// Source interactLivingEntity returns FAIL: consume the click without a swing.
/// The post-mob item hook consumes this hand turn without an arm swing.
pub(super) fn interact_entity(game: &mut Game<'_>, id: i32, kind: &str) -> bool {
    if !game.native_on("create")
        || !game
            .held()
            .is_some_and(|s| s.count > 0 && s.item.as_ref() == EMPTY)
        || !capturable(game, kind)
    {
        return false;
    }
    let Some(mob) = game.mobs.iter().find(|m| m.id as i32 == id && !m.removed) else {
        return false;
    };
    let at = [mob.position.x, mob.position.y, mob.position.z];
    if game.native_mob(game.dim.id(), id as u32).is_none() {
        return false;
    }
    capture_sound(game, at);
    capture_item(game);
    game.native_remove_mob(game.dim.id(), id as u32)
}
fn capture_particles(game: &mut Game<'_>, at: [f64; 3]) {
    for _ in 0..40 {
        let random = game.particles.level_random();
        let motion = std::array::from_fn::<_, 3, _>(|_| {
            f64::from((random.next_float() - 0.5) * 2.0 * 0.125)
        });
        game.particles
            .add(&ParticleOptions::simple(ParticleType::Flame), at, motion);
        let length = (motion[0] * motion[0] + motion[2] * motion[2]).sqrt();
        let circle = if length < 1.0e-4 {
            [0.0, 0.0]
        } else {
            [motion[0] / length * 0.5, motion[2] / length * 0.5]
        };
        // Original source omits the origin's X/Z here; preserve that behavior.
        game.particles.add(
            &ParticleOptions::simple(ParticleType::Smoke),
            [circle[0], at[1], circle[1]],
            [0.0, -0.125, 0.0],
        );
    }
}
/// After the normal UseItemOn packet, before vanilla prediction/other-hand fallback.
/// Caller swings once for this SUCCESS; inventory/state changes remain server-owned.
pub(super) fn predict_block(game: &mut Game<'_>, pos: Pos, held: Option<&ItemStack>) -> bool {
    if !reacts(game, pos, held) {
        return false;
    }
    if let Some(action) = held.and_then(|s| action(game, pos, s)) {
        block_sound(game, pos, action, true);
    } else {
        capture_particles(game, pos.map(|v| f64::from(v) + 0.5));
    }
    true
}
/// After the normal Interact packet, return true without swinging or mutating the stack.
pub(super) fn entity_reacts(game: &mut Game<'_>, index: usize, hand: crate::game::Hand) -> bool {
    if !game.native_on("create")
        || !game.with_hand(hand, |g| {
            g.held()
                .is_some_and(|s| s.count > 0 && s.item.as_ref() == EMPTY)
        })
    {
        return false;
    }
    let Some(mob) = game.mobs.get(index) else {
        return false;
    };
    if !capturable(game, &mob.kind.id) {
        return false;
    }
    true
}
pub(super) fn predict_entity(game: &mut Game<'_>, index: usize, hand: crate::game::Hand) -> bool {
    if !entity_reacts(game, index, hand) {
        return false;
    }
    let mob = &game.mobs[index];
    capture_particles(game, [mob.position.x, mob.position.y, mob.position.z]);
    true
}
pub(super) fn hand_operated(name: &str) -> bool {
    matches!(name, BLOCK | LIT)
}

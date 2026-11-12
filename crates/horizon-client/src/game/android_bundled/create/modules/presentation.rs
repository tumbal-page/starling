// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Client adapter for crank interpolation, depot piles, goggles and kinetic feedback.
use super::{Game, create_engine};
use crate::game::native_block_entity_draw::BlockEntityDraw;
use crate::game::create_port::{integer, number};
use horizon_client::{
    containers::{BlockEntity, ModBlockEntity},
    hud::HudBatch,
    items::ItemStack,
    particles::{ParticleOptions, ParticleType},
};
use super::create_engine::{
    kinetics::Pos,
    particles::{self as particle_rules, KineticEffects, ParticleRandom},
    visual::{self, CrankMotion},
};
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_world::nbt::{Compound, Tag};
use std::{cell::RefCell, collections::BTreeMap};
#[derive(Default)]
struct Motion {
    tick: i64,
    block: String,
    crank: CrankMotion,
    dial: f32,
    previous_dial: f32,
    speed: f32,
    status: String,
    effects: KineticEffects,
}
thread_local! {static MOTION_WORLD:std::cell::Cell<(usize,usize)>=const{std::cell::Cell::new((0,0))};}
thread_local! {static MOTION: RefCell<BTreeMap<(String,Pos),Motion>> = const {RefCell::new(BTreeMap::new())};}
struct Random<'a>(&'a mut LegacyRandom);
impl ParticleRandom for Random<'_> {
    fn next_float(&mut self) -> f32 {
        self.0.next_float()
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        self.0.next_int_bound(bound).unwrap_or(0)
    }
}
fn text(nbt: &Compound, key: &str) -> String {
    match nbt.get(&key.into()) {
        Some(Tag::String(s)) => s.to_string().unwrap_or_default(),
        _ => String::new(),
    }
}
fn in_tag(game: &Game<'_>, item: &ItemStack, tag: &str) -> bool {
    game.native_recipe_ingredient(&serde_json::Value::String(format!("#{tag}")))
        .is_some_and(|i| i.test(item))
}
fn key(game: &Game<'_>, pos: Pos) -> (String, Pos) {
    (game.dim.id().to_owned(), pos)
}
pub(super) fn crank_angle(game: &Game<'_>, pos: Pos, partial: f32) -> f32 {
    MOTION.with(|m| {
        m.borrow()
            .get(&key(game, pos))
            .map_or(0.0, |v| v.crank.rendered(partial))
    })
}
pub(super) fn dial(game: &Game<'_>, pos: Pos, partial: f32) -> f32 {
    MOTION.with(|m| {
        m.borrow().get(&key(game, pos)).map_or(0.0, |v| {
            v.previous_dial + (v.dial - v.previous_dial) * partial
        })
    })
}
// KineticBlockEntityVisual.applyOverstressEffect / standardKineticRotationTransform:
// only the rotating instances receive this fading red/spring-green tint.
pub(super) fn kinetic_tint(game: &Game<'_>, pos: Pos) -> u32 {
    MOTION.with(|m| {
        let effect = m
            .borrow()
            .get(&key(game, pos))
            .map_or(0.0, |v| v.effects.overstressed_effect);
        particle_rules::mix_colors(
            0xFFFFFF,
            if effect > 0.0 { 0xFF0000 } else { 0x00FFBB },
            effect.abs(),
        ) & 0xFFFFFF
    })
}
pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() || game.remote.is_none() || !game.native_on("create") {
        return;
    }
    let identity = (
        std::ptr::from_ref(game) as usize,
        std::ptr::from_ref(&game.world) as usize,
    );
    MOTION_WORLD.with(|id| {
        if id.replace(identity) != identity {
            MOTION.with(|m| m.borrow_mut().clear());
        }
    });
    crate::game::create_port_particles::client_tick(game);
    let time = game.world.game_time;
    let entries: Vec<_> = game
        .world
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), entity)| {
            let BlockEntity::Mod(e) = entity else {
                return None;
            };
            crate::game::create_port::owns(&game.native_block([x, y, z]))
                .then(|| ([x, y, z], game.native_block([x, y, z]), e.nbt.clone()))
        })
        .collect();
    for (pos, block, nbt) in entries {
        let speed = number(&nbt, "Speed");
        crate::game::create_port_nativeaudio::block(game, pos, &block, &nbt, speed);
        let status = text(&nbt, "NetworkStatus");
        let stress_effect = MOTION.with(|m| {
            let mut m = m.borrow_mut();
            let state = m.entry(key(game, pos)).or_default();
            if state.block != block || state.tick > time {
                *state = Motion {
                    tick: -1,
                    block: block.clone(),
                    ..Motion::default()
                };
            }
            if state.tick == time {
                return None;
            }
            let previous_status = state.status.clone();
            state.speed = speed;
            state.status = status.clone();
            state.crank.tick(speed);
            state.previous_dial = state.dial;
            state.dial += (number(&nbt, "Value") - state.dial) * 0.125;
            if state.dial > 1.0 && game.particles.level_random().next_float() < 0.5 {
                state.dial -= (state.dial - 1.0) * game.particles.level_random().next_float();
            }
            state.tick = time;
            if previous_status != status
                && (previous_status == "Overstressed" || status == "Overstressed")
            {
                state.effects.trigger_overstress();
            }
            state.effects.tick(true, status == "Overstressed").stress
        });
        if let Some(effect) = stress_effect {
            let options = ParticleOptions::simple(if effect.smoke {
                ParticleType::Smoke
            } else {
                ParticleType::Cloud
            });
            for _ in 0..effect.amount {
                let velocity = particle_rules::offset_randomly(
                    &mut Random(game.particles.level_random()),
                    effect.max_motion,
                );
                game.particles
                    .add(&options, pos.map(|v| f64::from(v) + 0.5), velocity);
            }
            trace(
                "stress_particles",
                pos,
                serde_json::json!({"overstressed":effect.smoke,"count":effect.amount}),
            );
        }
    }
    MOTION.with(|m| {
        m.borrow_mut()
            .retain(|(dimension, _), v| dimension == game.dim.id() && time - v.tick < 200)
    });
}
fn trace(event: &str, pos: Pos, details: serde_json::Value) {
    if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
        log::info!(
            "Create visual: {}",
            serde_json::json!({"event":event,"pos":pos,"details":details})
        );
    }
}
fn saved_stack(nbt: &Compound) -> Option<ItemStack> {
    nbt.get(&"Item".into())
        .and_then(horizon_items::item_nbt::item_from_nbt)
}
fn stack(game: &Game<'_>, draw: &mut BlockEntityDraw, item: &ItemStack, angle: f32) {
    let block = game
        .pack
        .assets
        .registry
        .default_state(&item.item)
        .is_some();
    let upright = in_tag(game, item, "create:upright_on_belt")
        && !in_tag(game, item, "create:not_upright_on_belt");
    draw.rotate_about([0.0, 1.0, 0.0], angle, [0.0; 3]);
    let mut random = LegacyRandom::new(0);
    if upright {
        draw.translate([0.0, 3.0 / 32.0, -1.0 / 16.0]);
    }
    for i in 0..visual::depot_layers(item.count) {
        draw.push();
        if block {
            draw.translate([
                random.next_float() * 0.0625 * i as f32,
                0.0,
                random.next_float() * 0.0625 * i as f32,
            ]);
        }
        draw.scale([0.5; 3]);
        if !block && !upright {
            draw.translate([0.0, -3.0 / 16.0, 0.0]);
            draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.0; 3]);
        }
        game.native_draw_item(
            draw,
            item,
            horizon_assets::item_model::DisplayContext::Fixed,
            None,
            0,
        );
        draw.pop();
        if !upright {
            if !block {
                draw.rotate_about([0.0, 1.0, 0.0], 10.0, [0.0; 3]);
            }
            draw.translate([0.0, if block { 1.0 / 64.0 } else { 1.0 / 16.0 }, 0.0]);
        } else {
            draw.translate([0.0, 0.0, -1.0 / 16.0]);
        }
    }
}
pub(super) fn depot(game: &Game<'_>, _: Pos, entity: &ModBlockEntity, draw: &mut BlockEntityDraw) {
    draw.push();
    draw.translate([0.5, 15.0 / 16.0, 0.5]);
    if let Some(Tag::Compound(held)) = entity.nbt.get(&"HeldItem".into())
        && let Some(item) = saved_stack(held)
    {
        draw.push();
        stack(game, draw, &item, integer(held, "Angle") as f32);
        draw.pop();
    }
    if let Some(Tag::List(items)) = entity
        .nbt
        .get(&"OutputBuffer".into())
        .and_then(|t| t.get("Items"))
    {
        for (i, tag) in items.iter().enumerate() {
            let Some(item) = horizon_items::item_nbt::item_from_nbt(tag) else {
                continue;
            };
            let slot = tag
                .get("Slot")
                .and_then(|t| match t {
                    Tag::Byte(v) => Some(*v as usize),
                    Tag::Int(v) => Some(*v as usize),
                    _ => None,
                })
                .unwrap_or(i);
            draw.push();
            draw.rotate_about([0.0, 1.0, 0.0], 45.0 * slot as f32, [0.0; 3]);
            draw.translate([0.35, 0.0, 0.0]);
            stack(
                game,
                draw,
                &item,
                (LegacyRandom::new(slot as i64 + 1).next_float() * 360.0) as i32 as f32,
            );
            draw.pop();
        }
    }
    draw.pop();
}
// Placement guides require the original Create outliner implementation. The
// previous port_hint arrow models were invented substitutes and are removed.
pub(super) fn hints(_: &Game<'_>, _: Pos, _: &mut BlockEntityDraw) {}
pub(super) fn hud(game: &Game<'_>, batch: &mut HudBatch) {
    crate::game::create_port_goggles::hud(game, batch);
}

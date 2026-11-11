// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// MechanicalMixerBlockEntity + BasinOperatingBlockEntity, Create 6.0.10.
// Source heat-aware item/water/lava mixing and ordinary shapeless crafting. processingTicks and
// currentRecipe deliberately remain transient, exactly as the source serializer.
use super::create_port_basin_recipes::Recipe;
use super::{Game, create_port_basin as basin, native_block_entity_draw::BlockEntityDraw};
use crate::containers::{BlockEntity, ModBlockEntity, ModRevision};
use create_engine::{basin as rules, kinetics::Axis, transport::SessionCache};
use horizon_client_render::sound_engine::{SoundEngine, SoundInstance, Sounds};
use horizon_core::random::RandomSource;
use horizon_world::nbt::Tag;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

const ID: &str = "create:mechanical_mixer";
type Pos = [i32; 3];

#[derive(Default)]
struct Running {
    owner: Option<ModRevision>,
    cycle: rules::Mixer,
    recipe: Option<Arc<Recipe>>,
    tick: Option<i64>,
    removed: bool,
}
#[derive(Default)]
struct Client {
    owner: Option<ModRevision>,
    tick: Option<i64>,
    received: Option<(bool, i32)>,
    cycle: rules::Mixer,
}
#[derive(Default)]
struct World {
    running: BTreeMap<Pos, Running>,
    client: BTreeMap<Pos, Client>,
    maintenance: Option<i64>,
}
thread_local! {
    static WORLDS: RefCell<SessionCache<World>> = RefCell::new(SessionCache::default());
}
fn with_world<R>(game: &Game<'_>, action: impl FnOnce(&mut World) -> R) -> R {
    WORLDS.with(|cache| {
        action(
            cache
                .borrow_mut()
                .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id()),
        )
    })
}
pub(super) fn reset(reload: bool) {
    // A data-pack reload keeps the live source BE and its current recipe.
    // Chunk/world reload re-creates it, losing processingTicks/currentRecipe.
    if !reload {
        WORLDS.with(|cache| cache.borrow_mut().clear());
    }
}
pub(super) fn maintenance(game: &mut Game<'_>) {
    with_world(game, |world| {
        if world.maintenance == Some(game.world.game_time) {
            return;
        }
        world.maintenance = Some(game.world.game_time);
        let loaded = |p: &Pos| game.world.loaded(p[0], p[2]) && game.native_block(*p) == ID;
        world.running.retain(|p, _| loaded(p));
        world.client.retain(|p, _| loaded(p));
    });
}
pub(super) fn initialize(entity: &mut ModBlockEntity) {
    if entity.id != ID {
        return;
    }
    entity.nbt.entry("Running".into()).or_insert(Tag::Byte(0));
    entity.nbt.entry("Ticks".into()).or_insert(Tag::Int(0));
    entity
        .nbt
        .entry("NeedsUpdate".into())
        .or_insert(Tag::Byte(0));
}
pub(super) fn schedule(game: &mut Game<'_>, pos: Pos) {
    if game.native_block(pos) != ID {
        return;
    }
    game.native_with_mod_block_entity(pos, |e| {
        if e.id == ID {
            e.nbt.insert("NeedsUpdate".into(), Tag::Byte(1));
        }
    });
}
/// BasinOperatingBlockEntity.onSpeedChanged schedules its DeferralBehaviour.
/// The runtime supplies real previous/current kinetic or effective speeds;
/// first observing an already-loaded machine is not a speed-change event.
pub(super) fn on_speed_changed(entity: &mut ModBlockEntity, previous: f32, current: f32) {
    if entity.id == ID && previous != current {
        entity.nbt.insert("NeedsUpdate".into(), Tag::Byte(1));
    }
}
pub(super) fn basin_removed(game: &mut Game<'_>, pos: Pos) {
    if game.native_block(pos) == ID {
        with_world(game, |w| w.running.entry(pos).or_default().removed = true);
    }
}
fn saved(entity: &ModBlockEntity) -> (bool, i32) {
    (
        super::create_port::integer(&entity.nbt, "Running") != 0,
        super::create_port::integer(&entity.nbt, "Ticks"),
    )
}
fn choose(game: &mut Game<'_>, pos: Pos, state: &mut basin::Basin) -> Option<Arc<Recipe>> {
    super::create_port_basin_recipes::choose(
        game,
        pos,
        state,
        super::create_port_basin_recipes::Operator::Mixing,
    )
}
fn apply(game: &mut Game<'_>, pos: Pos, recipe: Option<&Recipe>) -> bool {
    super::create_port_basin_recipes::apply(game, pos, recipe)
}
/// Runs once after the kinetic graph has resolved this mixer's actual RPM.
pub(super) fn tick(game: &mut Game<'_>, pos: Pos, entity: &mut ModBlockEntity, rpm: f32) -> bool {
    if entity.id != ID {
        return false;
    }
    initialize(entity);
    let now = game.world.game_time;
    let mut state = with_world(game, |w| w.running.remove(&pos).unwrap_or_default());
    let owner = entity.revision();
    let received = saved(entity);
    // Source transient fields belong to the BE object, not its coordinate or
    // saved counters. Ordinary load/update writes retain that object's timer.
    if state.tick.is_some_and(|t| t > now) || state.owner.is_some_and(|old| !old.same_entity(owner))
    {
        state = Running::default();
    }
    state.owner = Some(owner);
    if state.tick == Some(now) {
        with_world(game, |w| {
            w.running.insert(pos, state);
        });
        return false;
    }
    state.cycle.running = received.0;
    state.cycle.ticks = received.1;
    // DeferralBehaviour.read assigns this flag: an explicit false or absent
    // field clears it on the same BE. Keep no sticky duplicate in the cache.
    let mut pending = super::create_port::integer(&entity.nbt, "NeedsUpdate") != 0;
    let basin_pos = [pos[0], pos[1] - 2, pos[2]];
    let mut basin = basin::read(game, basin_pos);
    let exists = matches!(game.world.block_entities.get(&(basin_pos[0],basin_pos[1],basin_pos[2])),Some(BlockEntity::Mod(e)) if e.id == "create:basin");
    let mut sync = false;
    if !exists && state.cycle.running {
        state.cycle.basin_removed();
        sync = true;
    }
    if state.removed {
        state.cycle.basin_removed();
        state.removed = false;
        sync = true;
    }
    if pending {
        // DeferralBehaviour runs before MechanicalMixerBlockEntity.tick.
        pending = false;
        if !state.cycle.running
            && rpm != 0.0
            && rpm.abs() >= rules::DEFAULT_MIXER_MINIMUM_RPM
            && let Some(basin) = basin.as_mut()
            && let Some(recipe) = choose(game, basin_pos, basin)
        {
            state.recipe = Some(recipe);
            sync |= state.cycle.start();
        }
    }
    let recipe = state.recipe.clone();
    let events = state.cycle.tick(
        rpm,
        rules::DEFAULT_MIXER_MINIMUM_RPM,
        recipe.as_ref().map_or(0, |r| r.core.duration),
        || apply(game, basin_pos, recipe.as_deref()),
    );
    if events.initialized_processing
        && basin.as_ref().is_some_and(|basin| {
            basin
                .input_fluids
                .iter()
                .chain(&basin.output_fluids)
                .flatten()
                .any(|fluid| fluid.amount > 0)
        })
    {
        // MechanicalMixerBlockEntity.tick: one authoritative sound when the
        // processing timer is initialized, in addition to the client mixing loop.
        game.with_level(|_, level, _| {
            level.native_play_sound(
                "minecraft:block.bubble_column.whirlpool_ambient",
                pos.map(|v| f64::from(v) + 0.5),
                4,
                0.75,
                if rpm.abs() < 65.0 { 0.75 } else { 1.5 },
            );
        });
    }
    if events.apply_called {
        sync = true;
        if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
            log::info!(
                "Create mixing: {}",
                serde_json::json!({"position":pos,"recipe":recipe.as_ref().map(|r|&r.id),"running_ticks":state.cycle.ticks})
            );
        }
    }
    pending |= events.schedule_basin_check;
    entity
        .nbt
        .insert("Running".into(), Tag::Byte(i8::from(state.cycle.running)));
    entity
        .nbt
        .insert("Ticks".into(), Tag::Int(state.cycle.ticks));
    entity
        .nbt
        .insert("NeedsUpdate".into(), Tag::Byte(i8::from(pending)));
    state.tick = Some(now);
    with_world(game, |w| {
        w.running.insert(pos, state);
    });
    sync
}

// Minecraft1.21.1 Mth: float indexing is significant at lookup boundaries.
fn sine() -> &'static [f32; 65536] {
    static TABLE: OnceLock<[f32; 65536]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32)
    })
}
pub(super) fn cos(value: f32) -> f32 {
    sine()[((value * 10430.378 + 16384.0) as i32 & 65535) as usize]
}
pub(super) fn sin(value: f32) -> f32 {
    sine()[((value * 10430.378) as i32 & 65535) as usize]
}
pub(super) fn rotate_y(vector: [f64; 3], degrees: f32) -> [f64; 3] {
    if degrees == 0.0 {
        return vector;
    }
    let radians = (f64::from(degrees) / 180.0 * std::f64::consts::PI) as f32;
    let sin = f64::from(sine()[((radians * 10430.378) as i32 & 65535) as usize]);
    let cos = f64::from(cos(radians));
    let [x, y, z] = vector;
    [x * cos + z * sin, y, z * cos - x * sin]
}
pub(super) fn models(models: &mut Vec<String>) {
    models.extend(
        [
            "create:block/cogwheel_shaftless",
            "create:block/mechanical_mixer/pole",
            "create:block/mechanical_mixer/head",
        ]
        .map(str::to_owned),
    );
}
pub(super) fn bounds(entity: &ModBlockEntity, bounds: &mut [f32; 6]) {
    if entity.id == ID {
        *bounds = [0.0, -1.5, 0.0, 1.0, 1.0, 1.0];
    }
}
pub(super) fn draw(
    game: &Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) -> bool {
    if entity.id != ID {
        return false;
    }
    let cycle = with_world(game, |w| {
        w.client
            .get(&pos)
            .filter(|c| {
                c.owner
                    .is_some_and(|old| old.same_entity(entity.revision()))
            })
            .map_or_else(
                || {
                    let (running, ticks) = saved(entity);
                    rules::Mixer {
                        running,
                        ticks,
                        ..rules::Mixer::default()
                    }
                },
                |c| c.cycle,
            )
    });
    let rpm = super::create_port::number(&entity.nbt, "Speed");
    let time = draw.game_time as f64 + f64::from(draw.partial);
    let angle = create_engine::visual::angle(
        time,
        rpm,
        create_engine::visual::rotation_offset(pos, Axis::Y, false),
    );
    draw.push();
    draw.rotate_about([0.0, 1.0, 0.0], angle, [0.5; 3]);
    game.native_draw_model(
        draw,
        "create:block/cogwheel_shaftless",
        super::create_port_presentation::kinetic_tint(game, pos),
    );
    draw.pop();
    let offset = cycle.rendered_head_offset(draw.partial, cos);
    draw.push();
    draw.translate([0.0, -offset, 0.0]);
    game.native_draw_model(draw, "create:block/mechanical_mixer/pole", 0xFFFFFF);
    draw.pop();
    let angle = ((time as f32 * cycle.rendered_rotation_speed(rpm) * 6.0 / 10.0) % 360.0) / 180.0
        * std::f32::consts::PI;
    draw.push();
    draw.rotate_about([0.0, 1.0, 0.0], angle.to_degrees(), [0.5; 3]);
    draw.translate([0.0, -offset, 0.0]);
    game.native_draw_model(draw, "create:block/mechanical_mixer/head", 0xFFFFFF);
    draw.pop();
    true
}
pub(super) fn client_tick(game: &mut Game<'_>) {
    if !game.native_window() || game.remote.is_none() || !game.native_on("create") {
        return;
    }
    let time = game.world.game_time;
    let entries: Vec<_> = game
        .world
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), e)| {
            let BlockEntity::Mod(e) = e else {
                return None;
            };
            (e.id == ID).then(|| {
                (
                    [x, y, z],
                    e.revision(),
                    saved(e),
                    super::create_port::number(&e.nbt, "Speed"),
                )
            })
        })
        .collect();
    let live: std::collections::BTreeSet<_> = entries.iter().map(|entry| entry.0).collect();
    with_world(game, |world| {
        world.client.retain(|pos, _| live.contains(pos))
    });
    for (pos, owner, received, rpm) in entries {
        let particles = with_world(game, |w| {
            let c = w.client.entry(pos).or_default();
            if c.tick.is_some_and(|t| t > time)
                || c.owner.is_some_and(|old| !old.same_entity(owner))
            {
                *c = Client::default();
            }
            c.owner = Some(owner);
            if c.tick == Some(time) {
                return false;
            }
            if c.received != Some(received) {
                c.cycle.running = received.0;
                c.cycle.ticks = received.1;
                c.received = Some(received);
            }
            let particles = c.cycle.running && c.cycle.ticks == 20;
            if c.cycle.ticks >= 40 {
                c.cycle.running = false;
                c.cycle.ticks = 0;
            } else if c.cycle.running {
                if rpm == 0.0 || rpm.abs() < rules::DEFAULT_MIXER_MINIMUM_RPM {
                    if c.cycle.ticks < 20 {
                        c.cycle.ticks = 40 - c.cycle.ticks;
                    } else if c.cycle.ticks == 20 {
                        c.cycle.ticks += 1;
                    }
                }
                if c.cycle.ticks != 20 {
                    c.cycle.ticks += 1;
                }
            }
            c.tick = Some(time);
            particles
        });
        if !particles {
            continue;
        }
        if rpm.abs() >= 65.0 || time % 2 != 0 {
            let at = pos.map(|v| f64::from(v) + 0.5);
            let distance: f64 = at
                .iter()
                .zip(game.audio.listener)
                .map(|(a, b)| (a - b).powi(2))
                .sum();
            let delay = if distance > 100.0 {
                (distance.sqrt() / 40.0 * 20.0) as i32
            } else {
                0
            };
            let sound =
                SoundInstance::at("create:mixing", 4, 0.75, 1.0, SoundEngine::unseeded(), at);
            if delay > 0 {
                let id = game.audio.engine.add(sound, false);
                game.audio.engine.play_delayed(id, delay);
            } else {
                game.audio.play_instance(sound, false);
            }
        }
        let stacks = basin::read(game, [pos[0], pos[1] - 2, pos[2]])
            .map(|b| {
                b.input
                    .iter()
                    .chain(&b.output)
                    .flatten()
                    .filter(|s| s.count > 0)
                    .map(basin::native_stack)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for stack in stacks {
            let angle = game.particles.level_random().next_float() * 360.0;
            let offset = rotate_y([0.0, 0.0, 0.25], angle);
            let mut target = rotate_y(offset, if rpm > 0.0 { 25.0 } else { -25.0 });
            target[1] += 0.25;
            let velocity = std::array::from_fn(|i| {
                target[i] - offset[i]
                    + f64::from((game.particles.level_random().next_float() - 0.5) * 2.0 / 128.0)
            });
            let center = [
                f64::from(pos[0]) + 0.5 + offset[0],
                f64::from(pos[1]) + 0.5 + offset[1] - 1.75,
                f64::from(pos[2]) + 0.5 + offset[2],
            ];
            let options = game.item_particle(&stack);
            game.particles.add(&options, center, velocity);
        }
    }
}

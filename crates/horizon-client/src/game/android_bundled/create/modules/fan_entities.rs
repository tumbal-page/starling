// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// AirCurrent entity motion and AllFanProcessingTypes server entity effects.
// Player force runs on its client; the server applies hazards and clears fall
// bookkeeping. Sending an additional server impulse would apply the force twice.
use super::{Game, create_engine};
use crate::game::commands::Target;
use super::create_engine::{
    fan::{self, ProcessingType, Segment},
    fan_entities::{self as rules, DamageType, Facts, Horse},
    placement::Direction,
};
use horizon_client::{
    effects::EffectInstance,
    mobs::{Damage, DamageKind},
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::RandomSource,
};
use horizon_world::nbt::{Compound, Tag};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
};

type Key = (String, [i32; 3]);
#[derive(Clone, Copy)]
enum Caught {
    Mob(u32),
    Player(u32),
}
#[derive(Default)]
struct Current {
    search: i32,
    caught: Vec<Caught>,
    local_player: bool,
}
#[derive(Default)]
struct Index {
    stamp: Option<(String, i64)>,
    mobs: HashMap<u32, usize>,
    cells: BTreeMap<[i32; 2], Vec<u32>>,
}
#[derive(Default)]
struct Cache {
    game: Option<usize>,
    server: BTreeMap<Key, Current>,
    client: BTreeMap<Key, Current>,
    index: Index,
    cleaned: Option<(String, i64)>,
    damage_flags: BTreeMap<&'static str, u32>,
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); }

pub(super) fn reset(_reload: bool) {
    CACHE.with(|c| *c.borrow_mut() = Cache::default());
}
pub(super) fn clear(dimension: &str, origin: [i32; 3]) {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.server.remove(&(dimension.into(), origin));
        c.client.remove(&(dimension.into(), origin));
    });
}
pub(super) fn maintenance(game: &Game<'_>) {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        prepare(&mut c, game);
        let stamp = (game.dim.id().to_owned(), game.world.game_time);
        if c.cleaned.as_ref() == Some(&stamp) {
            return;
        }
        c.cleaned = Some(stamp);
        if game.world.game_time.rem_euclid(20) == 0 {
            let keep = |(dim, pos): &Key, _: &mut Current| {
                dim != game.dim.id() || game.native_block(*pos) == "create:encased_fan"
            };
            c.server.retain(keep);
            c.client.retain(keep);
        }
    });
}
fn prepare(cache: &mut Cache, game: &Game<'_>) {
    let id = std::ptr::from_ref(game) as usize;
    if cache.game != Some(id) {
        *cache = Cache {
            game: Some(id),
            ..Cache::default()
        };
    }
}
fn corners(bounds: Aabb) -> [[f64; 3]; 2] {
    let (a, b) = (bounds.min_position(), bounds.max_position());
    [[a.x, a.y, a.z], [b.x, b.y, b.z]]
}
fn bounds(origin: [i32; 3], facing: Direction, distance: f32) -> [[f64; 3]; 2] {
    let mut a = origin.map(f64::from);
    let mut b = a.map(|v| v + 1.0);
    let axis = facing.axis().index();
    if facing.vector()[axis] > 0 {
        a[axis] += 1.0;
        b[axis] = a[axis] + f64::from(distance);
    } else {
        b[axis] -= 1.0;
        a[axis] = b[axis] - f64::from(distance);
    }
    [a, b]
}
fn cells(bounds: [[f64; 3]; 2]) -> impl Iterator<Item = [i32; 2]> {
    let min = bounds[0].map(|v| (v.floor() as i32) >> 4);
    let max = bounds[1].map(|v| (v.floor() as i32) >> 4);
    (min[0]..=max[0]).flat_map(move |x| (min[2]..=max[2]).map(move |z| [x, z]))
}
fn index(cache: &mut Cache, game: &Game<'_>) {
    let stamp = (game.dim.id().to_owned(), game.world.game_time);
    if cache.index.stamp.as_ref() == Some(&stamp) {
        return;
    }
    cache.index.stamp = Some(stamp);
    cache.index.mobs.clear();
    cache.index.cells.clear();
    for (i, mob) in game
        .mobs
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.removed && m.alive())
    {
        cache.index.mobs.insert(mob.id, i);
        for cell in cells(corners(mob.bounds())) {
            cache.index.cells.entry(cell).or_default().push(mob.id);
        }
    }
}
fn kind_at(
    origin: [i32; 3],
    facing: Direction,
    rpm: f32,
    distance: f32,
    segments: &[Segment],
    position: [f64; 3],
) -> Option<ProcessingType> {
    let axis = facing.axis().index();
    let offset = if facing.vector()[axis] > 0 {
        position[axis] - f64::from(origin[axis]) - 1.0
    } else {
        f64::from(origin[axis]) - position[axis]
    };
    fan::type_at(distance, fan::pushing(rpm, facing), segments, offset as f32)
}
fn flow(facing: Direction, rpm: f32) -> [i32; 3] {
    if fan::pushing(rpm, facing) {
        facing.vector()
    } else {
        facing.opposite().vector()
    }
}

pub(super) fn server_tick(
    game: &mut Game<'_>,
    origin: [i32; 3],
    facing: Direction,
    rpm: f32,
    distance: f32,
    segments: &[Segment],
) {
    if rpm == 0.0 || distance < 0.25 {
        clear(game.dim.id(), origin);
        return;
    }
    let key = (game.dim.id().to_owned(), origin);
    let area = bounds(origin, facing, distance);
    let (mut current, mob_indices) = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        prepare(&mut c, game);
        index(&mut c, game);
        let mut current = c.server.remove(&key).unwrap_or_default();
        if current.search <= 0 {
            current.search = 5;
            current.caught.clear();
            let mut found = BTreeSet::new();
            for cell in cells(area) {
                if let Some(ids) = c.index.cells.get(&cell) {
                    found.extend(ids);
                }
            }
            for id in found {
                if let Some(&i) = c.index.mobs.get(id)
                    && rules::intersects(area, corners(game.mobs[i].bounds()))
                {
                    current.caught.push(Caught::Mob(*id));
                }
            }
            if let Some(host) = &game.lan {
                for player in host.hub.guests.iter().filter(|p| host.hub.here(p)) {
                    if rules::eligible(
                        !player.dead && player.health > 0.0,
                        player.game_mode == 3,
                        player.creative(),
                        player.flying,
                    ) && rules::intersects(area, corners(player.bounds()))
                    {
                        current.caught.push(Caught::Player(player.conn));
                    }
                }
            }
        } else {
            current.search -= 1;
        }
        let indices = current
            .caught
            .iter()
            .filter_map(|e| match e {
                Caught::Mob(id) => c.index.mobs.get(id).map(|i| (*id, *i)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        (current, indices)
    });
    current.caught.retain(|target| match *target {
        Caught::Mob(id) => {
            let Some(&i) = mob_indices.get(&id) else {
                return false;
            };
            // A conversion may append a mob; existing indices remain stable.
            let Some(mob) = game.mobs.get(i).filter(|m| m.id == id) else {
                return false;
            };
            if mob.removed || !mob.alive() || !rules::intersects(area, corners(mob.bounds())) {
                return false;
            }
            let position = [mob.position.x, mob.position.y, mob.position.z];
            let velocity = rules::movement(
                [mob.velocity.x, mob.velocity.y, mob.velocity.z],
                position,
                origin.map(|v| f64::from(v) + 0.5),
                flow(facing, rpm),
                rpm,
                distance,
                mob.extra.crouching,
            );
            let mob = &mut game.mobs[i];
            mob.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
            mob.fall_distance = 0.0;
            mob.needs_sync = true;
            if let Some(kind) = kind_at(origin, facing, rpm, distance, segments, position) {
                affect_mob(game, i, kind);
            }
            true
        }
        Caught::Player(conn) => {
            let Some(player) = game.command_guest(conn) else {
                return false;
            };
            if player.dimension != game.dim.id()
                || !rules::eligible(
                    !player.dead && player.health > 0.0,
                    player.game_mode == 3,
                    player.creative(),
                    player.flying,
                )
                || !rules::intersects(area, corners(player.bounds()))
            {
                return false;
            }
            let kind = kind_at(origin, facing, rpm, distance, segments, player.position);
            game.for_player(Target::Guest(conn), |game| {
                if let Some(player) = game.player.as_mut() {
                    player.fall_distance = 0.0;
                }
                if let Some(kind) = kind {
                    affect_player(game, kind);
                }
            });
            if let Some(player) = game.lan.as_mut().and_then(|h| h.hub.guest_mut(conn)) {
                player.above_ground_ticks = 0;
            }
            true
        }
    });
    CACHE.with(|c| c.borrow_mut().server.insert(key, current));
}

pub(super) fn client_tick(
    game: &mut Game<'_>,
    origin: [i32; 3],
    facing: Direction,
    rpm: f32,
    distance: f32,
    _segments: &[Segment],
) {
    if rpm == 0.0 || distance < 0.25 {
        clear(game.dim.id(), origin);
        return;
    }
    let Some(player) = game.player.as_ref() else {
        return;
    };
    let eligible =
        rules::eligible(
            player.health.health() > 0.0,
            player.spectator,
            game.creative,
            player.flying,
        ) && rules::intersects(bounds(origin, facing, distance), corners(player.bounds()));
    let apply = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        prepare(&mut c, game);
        let current = c
            .client
            .entry((game.dim.id().to_owned(), origin))
            .or_default();
        if current.search <= 0 {
            current.search = 5;
            current.local_player = eligible;
        } else {
            current.search -= 1;
            current.local_player &= eligible;
        }
        current.local_player
    });
    if !apply {
        return;
    }
    let position = [player.position.x, player.position.y, player.position.z];
    let before = [player.velocity.x, player.velocity.y, player.velocity.z];
    let after = rules::movement(
        before,
        position,
        origin.map(|v| f64::from(v) + 0.5),
        flow(facing, rpm),
        rpm,
        distance,
        player.shift_key_down,
    );
    game.native_push_player(std::array::from_fn(|i| (after[i] - before[i]) as f32));
    if let Some(player) = game.player.as_ref() {
        let velocity = [player.velocity.x, player.velocity.y, player.velocity.z];
        super::create_port_nativeaudio::enable_fan(game, rpm, velocity);
    }
    if let Some(player) = game.player.as_mut() {
        player.fall_distance = 0.0;
    }
}

fn affect_player(game: &mut Game<'_>, kind: ProcessingType) {
    let facts = Facts {
        living: true,
        on_fire: game.player.as_ref().is_some_and(|p| p.fire_ticks > 0),
        ..Facts::default()
    };
    let effects = rules::effects(kind, facts, 0);
    if effects.ignite_ticks != 0
        && let Some(player) = game.player.as_mut()
    {
        let _ = player.ignite_for_ticks(effects.ignite_ticks);
    }
    if let Some((damage, amount)) = effects.damage {
        game.hurt_player_from(amount, None, damage.id(), Some(None));
    }
    for status in effects.statuses.into_iter().flatten() {
        let mut effect = EffectInstance::new(
            status.id.trim_start_matches("minecraft:"),
            status.ticks,
            status.amplifier,
        );
        effect.ambient = status.ambient;
        effect.visible = status.particles;
        game.add_player_effect(effect);
    }
    if effects.extinguish {
        let Some(player) = game.player.as_mut() else {
            return;
        };
        player.set_remaining_fire_ticks(0);
        let at = [player.position.x, player.position.y, player.position.z];
        let pitch = 1.6 + (game.world.random.next_float() - game.world.random.next_float()) * 0.4;
        sound(
            game,
            "minecraft:entity.generic.extinguish_fire",
            at,
            0.7,
            pitch,
        );
    }
}
fn horse_progress(game: &Game<'_>, id: u32) -> i32 {
    game.persist
        .raw
        .get(&id)
        .and_then(|t| t.get("NeoForgeData"))
        .and_then(|t| t.get("CreateHaunting"))
        .and_then(|t| if let Tag::Int(v) = t { Some(*v) } else { None })
        .unwrap_or(0)
}
fn store_horse_progress(game: &mut Game<'_>, id: u32, progress: i32) {
    let root = game
        .persist
        .raw
        .entry(id)
        .or_insert_with(|| Tag::Compound(Compound::new()));
    let Tag::Compound(root) = root else {
        return;
    };
    let data = root
        .entry("NeoForgeData".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    if let Tag::Compound(data) = data {
        data.insert("CreateHaunting".into(), Tag::Int(progress));
    }
}
fn affect_mob(game: &mut Game<'_>, index: usize, kind: ProcessingType) {
    let mob = &game.mobs[index];
    let id = mob.id;
    let facts = Facts {
        living: true,
        horse: mob.kind.id.as_str() == "minecraft:horse",
        fire_immune: mob.kind.flags.fire_immune,
        on_fire: mob.is_on_fire(),
        water_sensitive: matches!(
            mob.kind.id.as_str(),
            "minecraft:enderman" | "minecraft:snow_golem" | "minecraft:blaze"
        ),
    };
    let effects = rules::effects(kind, facts, horse_progress(game, id));
    let at = [mob.position.x, mob.position.y, mob.position.z];
    let damage = effects
        .damage
        .map(|(damage, amount)| (damage_kind(game, damage), amount));
    let mut added = Vec::new();
    {
        let mob = &mut game.mobs[index];
        if effects.ignite_ticks != 0 {
            mob.ignite_for_ticks(effects.ignite_ticks);
        }
        if let Some((kind, amount)) = damage {
            mob.damage(amount, Damage::of(kind));
        }
        for status in effects.statuses.into_iter().flatten() {
            let new = !mob.has_effect(status.id);
            mob.add_effect_with(
                status.id,
                status.ticks,
                status.amplifier,
                status.ambient,
                status.particles,
            );
            added.push((status, new));
        }
        if effects.extinguish {
            mob.fire_ticks = 0;
        }
        mob.needs_sync = true;
    }
    for (status, new) in added {
        game.mod_effect_added(status.id, Some(id as i32), status.amplifier, new);
    }
    if effects.extinguish {
        let pitch = 1.6 + (game.world.random.next_float() - game.world.random.next_float()) * 0.4;
        sound(
            game,
            "minecraft:entity.generic.extinguish_fire",
            at,
            0.7,
            pitch,
        );
    }
    match effects.horse {
        Horse::Progress { value, sound_pitch } => {
            if let Some(pitch) = sound_pitch {
                sound(game, "minecraft:particle.soul_escape", at, 1.0, pitch);
            }
            store_horse_progress(game, id, value);
        }
        Horse::Convert => convert_horse(game, id, at),
        Horse::None => {}
    }
}
fn damage_kind(_game: &Game<'_>, damage: DamageType) -> DamageKind {
    if damage == DamageType::Drown {
        return DamageKind::Drown;
    }
    DamageKind::Generic
}
fn sound(game: &mut Game<'_>, id: &str, at: [f64; 3], volume: f32, pitch: f32) {
    // AllFanProcessingTypes calls the BlockPos overload of Level.playSound.
    let center = at.map(|v| v.floor() + 0.5);
    game.with_level(|_, level, _| level.native_play_sound(id, center, 6, volume, pitch));
}
fn convert_horse(game: &mut Game<'_>, id: u32, at: [f64; 3]) {
    let Some(entity) = game.native_entity(id) else {
        return;
    };
    let _ = game.native_entity_nbt(&entity); // create port stub: entity NBT not converted on host
    let armor = game
        .mobs
        .iter()
        .find(|m| m.id == id)
        .and_then(|_m| None); // create port stub: equipment_stacks not wired on host
    let previous = game
        .mobs
        .iter()
        .find(|m| m.id == id)
        .map(|m| [m.previous.x, m.previous.y, m.previous.z])
        .unwrap_or(at);
    let dim = game.dim.id().to_owned();
    if game
        .native_summon(&dim, "minecraft:skeleton_horse", previous, Compound::new())
        .is_none()
    {
        log::warn!("Create haunting could not convert horse {id}; keeping original entity");
        return;
    }
    if let Some(armor) = armor {
        game.spawn_item(armor, at, crate::game::dispenser::new_item_motion(), 10);
    }
    game.native_remove_mob(&dim, id);
    // Later fans in this tick must be able to discover the new skeleton horse.
    CACHE.with(|c| c.borrow_mut().index.stamp = None);
    sound(
        game,
        "minecraft:entity.generic.extinguish_fire",
        at,
        1.25,
        0.65,
    );
}

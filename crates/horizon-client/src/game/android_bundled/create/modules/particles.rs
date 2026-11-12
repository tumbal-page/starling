// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Native adapters for the source RotationIndicatorParticle and AirFlowParticle.
// Controllers retain values only; no Game or level borrow survives a callback.
use super::Game;
use crate::game::mod_api::{Particle, ParticleController, ParticleEngineExt, ParticleLevel, ParticleOptions, ParticleType, ParticleUpdate};
use horizon_client::{
    containers::{BlockEntity, ModBlockEntity},
};
use horizon_core::random::RandomSource;
use super::create_engine::{
    fan::{self, ProcessingType, Segment},
    kinetics::{Axis, Pos},
    particles::{self as rules, Airflow, AirflowContext, Extra, Kind, Rotation},
    placement::Direction,
};
use super::create_engine::particles::ParticleRandom as EngineParticleRandom;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, OnceLock},
};

struct Random<'a>(&'a mut dyn crate::game::mod_api::ParticleRandom);
impl EngineParticleRandom for Random<'_> {
    fn next_float(&mut self) -> f32 {
        self.0.next_float()
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        self.0.next_int(bound)
    }
}
fn rgb(color: u32) -> [f32; 3] {
    [16, 8, 0].map(|s| ((color >> s) & 255) as f32 / 255.0)
}
fn controller_options(kind: Kind) -> ParticleOptions {
    // Particle options do not need a retained Game. These vanilla source options
    // use fixed colors and the default lava particle sprite.
    let ty = match kind {
        Kind::Flame => ParticleType::Flame,
        Kind::LavaBlock => ParticleType::Block,
        Kind::SoulFireFlame => ParticleType::SoulFireFlame,
        Kind::Smoke => ParticleType::Smoke,
        Kind::LargeSmoke => ParticleType::LargeSmoke,
        Kind::Bubble => ParticleType::Bubble,
        Kind::BubblePop => ParticleType::BubblePop,
        Kind::Poof => ParticleType::Poof,
        Kind::BlueDust => ParticleType::Dust,
        Kind::Spit => ParticleType::Spit,
        Kind::Cloud => ParticleType::Cloud,
    };
    let mut o = ParticleOptions::simple(ty);
    if kind == Kind::LavaBlock {
        o.block = Some(("block/lava_still".into(), 0xFFFFFF));
    }
    if kind == Kind::BlueDust {
        o.color = [0.0, 85.0 / 255.0, 1.0, 1.0];
    }
    o
}
fn emit(_game: &mut Game<'_>, _extras: impl IntoIterator<Item = Extra>) {
    // create port stub: particle emission not wired on host
}
fn spawned(extras: Vec<Extra>) -> Vec<(ParticleOptions, [f64; 3], [f64; 3])> {
    extras
        .into_iter()
        .map(|e| (controller_options(e.kind), e.position, e.velocity))
        .collect()
}
fn tracing() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CREATE_HORIZON_PARTICLE_TRACE").is_some())
}
fn trace_spawn(game: &Game<'_>, particle: &Particle, kind: &str, origin: Pos) {
    if !tracing() {
        return;
    }
    let snapshot = ParticleEngineExt::snapshot(&game.particles, particle);
    let (block, sky) = game.native_light(snapshot.position.map(|v| v.floor() as i32));
    let light = if kind == "rotation_indicator" {
        0xF000F0
    } else {
        u32::from(sky) << 20 | u32::from(block) << 4
    };
    // ParticleEngine::snapshot uses a fixed bright-sky probe. Record the
    // normal renderer's world light separately rather than calling that probe
    // an observation of the actual scene.
    let quad = snapshot.quad.map(|q| serde_json::json!({"sprite":q.sprite,"size":q.size,"rgba":q.rgba,"uv":q.uv,"light":light,"probe_light":q.light}));
    log::info!(
        "Create particle: {}",
        serde_json::json!({"event":"spawn","kind":kind,"origin":origin,"game_time":game.world.game_time,"animation_ticks":CLIENT.with(|c|c.borrow().ticks),
        "age":snapshot.age,"lifetime":snapshot.lifetime,"position":snapshot.position,"previous":snapshot.previous,"velocity":snapshot.velocity,"quad":quad})
    );
}

fn trace_tick(
    p: &Particle,
    level: &ParticleLevel<'_>,
    kind: &str,
    origin: Pos,
    frame: usize,
    removed: bool,
) {
    if !tracing() {
        return;
    }
    log::info!(
        "Create particle: {}",
        serde_json::json!({"event":"tick","kind":kind,"origin":origin,"game_time":level.world,"animation_ticks":CLIENT.with(|c|c.borrow().ticks),
        "age":p.age,"lifetime":p.lifetime,"position":p.position,"previous":p.previous,"velocity":p.velocity,"rgba":p.rgba,"size":p.size,"frame":frame,"removed":removed})
    );
}

#[derive(Clone, Debug)]
struct Flow {
    facing: Direction,
    rpm: f32,
    distance: f32,
    segments: Vec<Segment>,
    handlers: Vec<(Pos, ProcessingType)>,
    bounds: [[f64; 3]; 2],
}
#[derive(Default)]
struct Client {
    world: usize,
    game: usize,
    dimension: String,
    stamp: Option<i64>,
    ticks: i32,
    flows: BTreeMap<Pos, Flow>,
    caught: BTreeMap<Pos, Caught>,
}
#[derive(Default)]
struct Caught {
    search: i32,
    items: Vec<u32>,
    horses: Vec<u32>,
}
thread_local! {static CLIENT: RefCell<Client> = RefCell::new(Client::default());}
#[derive(Clone, Debug)]
struct AirController {
    origin: Pos,
    world: usize,
    initial: Arc<OnceLock<Airflow>>,
    state: Option<Airflow>,
}
impl ParticleController for AirController {
    fn tick(&mut self, particle: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate {
        CLIENT.with(|cache| {
            let cache = cache.borrow();
            let Some(flow) = cache.flows.get(&self.origin).filter(|_| {
                cache.world == self.world && self.world == level.world
            }) else {
                particle.remove();
                return ParticleUpdate::default();
            };
            let state = self.state.get_or_insert_with(|| {
                self.initial
                    .get()
                    .expect("airflow initialized before push")
                    .clone()
            });
            state.on_ground = particle.on_ground;
            let extras = state.tick(
                &AirflowContext {
                    origin: self.origin,
                    facing: flow.facing,
                    pushing: fan::pushing(flow.rpm, flow.facing),
                    distance: flow.distance,
                    bounds: flow.bounds,
                    source_removed: false,
                    segments: &flow.segments,
                },
                &mut Random(particle.random()),
            );
            particle.previous = state.previous;
            particle.position = state.position;
            particle.velocity = state.velocity;
            particle.age = state.age;
            particle.rgba[..3].copy_from_slice(&rgb(state.color));
            particle.rgba[3] = state.alpha;
            if state.removed {
                particle.remove();
            }
            let frame = (state.sprite.max(0) as usize * 7) / 8;
            trace_tick(
                particle,
                level,
                "air_flow",
                self.origin,
                frame,
                state.removed,
            );
            ParticleUpdate {
                frame: Some(frame),
                spawned: Vec::new(),
            }
        })
    }
}
#[derive(Clone, Debug)]
struct RotationController {
    rotation: Rotation,
    fade: [f32; 3],
    visible: bool,
    world: usize,
}
impl ParticleController for RotationController {
    fn tick(&mut self, p: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate {
        if level.world != self.world {
            p.remove();
            return ParticleUpdate::default();
        }
        if p.advance_age() {
            let time = CLIENT.with(|c| c.borrow().ticks);
            p.position = self.rotation.position(time as f32, p.age, p.lifetime);
        }
        // SimpleAnimatedParticle.tick: source fade begins strictly after half.
        if p.age > p.lifetime / 2 {
            p.rgba[3] = 1.0 - (p.age as f32 - (p.lifetime / 2) as f32) / p.lifetime as f32;
            for i in 0..3 {
                p.rgba[i] += (self.fade[i] - p.rgba[i]) * 0.2;
            }
        }
        p.set_visible(self.visible);
        self.rotation.tick_radius();
        let frame = (p.age.max(0) as usize * 7) / p.lifetime.max(1) as usize;
        trace_tick(
            p,
            level,
            "rotation_indicator",
            self.rotation.origin.map(|v| v.floor() as i32),
            frame,
            p.age > p.lifetime,
        );
        ParticleUpdate {
            frame: Some(frame),
            ..Default::default()
        }
    }
}
fn flow_bounds(pos: Pos, d: Direction, distance: f32) -> [[f64; 3]; 2] {
    if distance < 0.25 {
        return [[0.0; 3]; 2];
    }
    let mut a = pos.map(f64::from);
    let mut b = a.map(|v| v + 1.0);
    let axis = d.axis().index();
    if d.vector()[axis] > 0 {
        a[axis] += 1.0;
        b[axis] = a[axis] + f64::from(distance);
    } else {
        b[axis] -= 1.0;
        a[axis] = b[axis] - f64::from(distance);
    }
    [a, b]
}
fn corners(a: horizon_core::geometry::Aabb) -> [[f64; 3]; 2] {
    let (a, b) = (a.min_position(), a.max_position());
    [[a.x, a.y, a.z], [b.x, b.y, b.z]]
}
fn intersects(a: [[f64; 3]; 2], b: [[f64; 3]; 2]) -> bool {
    (0..3).all(|i| a[0][i] < b[1][i] && a[1][i] > b[0][i])
}
fn cell_range(b: [[f64; 3]; 2]) -> impl Iterator<Item = [i32; 2]> {
    let a = b[0].map(|v| (v.floor() as i32) >> 4);
    let b = b[1].map(|v| (v.floor() as i32) >> 4);
    (a[0]..=b[0]).flat_map(move |x| (a[2]..=b[2]).map(move |z| [x, z]))
}
fn type_at(origin: Pos, flow: &Flow, p: [f64; 3]) -> Option<ProcessingType> {
    let dir = flow.facing.vector();
    let axis = flow.facing.axis().index();
    let offset = (p[axis] - (f64::from(origin[axis]) + 0.5)) * f64::from(dir[axis]) - 0.5;
    fan::type_at(
        flow.distance,
        fan::pushing(flow.rpm, flow.facing),
        &flow.segments,
        offset as f32,
    )
}
fn airflow(game: &mut Game<'_>, origin: Pos, flow: &Flow, world: usize) {
    if !rules::spawn_airflow(&mut Random(&mut game.world.random), 0.5) {
        return;
    }
    let position = rules::airflow_spawn_position(
        origin,
        flow.facing,
        fan::pushing(flow.rpm, flow.facing),
        flow.distance,
    );
    // Constructor evaluates the level RNG gravity argument before base creation.
    let gravity = rules::airflow_gravity(&mut Random(&mut game.world.random));
    let initial = Arc::new(OnceLock::new());
    let controller = AirController {
        origin,
        world,
        initial: initial.clone(),
        state: None,
    };
    let Some(mut p) = game
        .particles
        .controlled("create:air_flow", position, true, controller)
    else {
        return;
    };
    let state = Airflow::new(position, &mut Random(p.random()));
    p.position = state.position;
    p.previous = state.previous;
    p.lifetime = state.lifetime;
    p.size *= rules::QUAD_SIZE_MULTIPLIER;
    p.rgba = [238.0 / 255.0, 238.0 / 255.0, 238.0 / 255.0, 0.25];
    p.gravity = gravity as f64;
    // The controller is owned at construction; initialize its state by the same
    // source constructor on its first tick, without repeating any RNG draws.
    let _ = initial.set(state);
    let p_copy = (*p).clone();
    trace_spawn(game, &p_copy, "air_flow", origin);
    // controlled() already registers the particle
}

// The rotation event uses the ordinary one-shot native mod network channel.
// It is never saved, cached in a block-entity update or replayed on chunk load.
const CHANNEL: &str = "create:rotation_particles";
const PACKET_MAGIC: &[u8; 4] = b"CRP1";
const RECORD_LEN: usize = 32;
const MAX_RECORDS: usize = (30_000 - 4) / RECORD_LEN;
#[derive(Clone, Copy)]
struct RotationEvent {
    pos: Pos,
    color: u32,
    speed: f32,
    initial: f32,
    target: f32,
    axis: Axis,
}
#[derive(Clone, Copy)]
struct Generator {
    speed: f32,
    theoretical: f32,
    overpowered: bool,
}
// GeneratingKineticBlockEntity.updateGeneratedRotation queues before applying
// a new setting, only when it has no source and crosses a SpeedLevel. When an
// external faster source disappears, removeSource first clears this.speed;
// reActivateSource therefore crosses NONE even if the two running speeds were
// in the same tier. Strict dominance is observable in the solved graph; equal
// compatible generator ownership is not represented by that graph.
fn generator_transition(
    previous: Option<Generator>,
    old_rpm: f32,
    theoretical: f32,
    generated: f32,
) -> (Generator, bool, bool) {
    let current = Generator {
        speed: generated,
        theoretical,
        overpowered: theoretical.abs() > generated.abs(),
    };
    let was_overpowered = previous.is_some_and(|p| p.overpowered);
    let setting_changed = previous.map_or(old_rpm != generated, |p| p.speed != generated);
    let recovered = previous.is_some_and(|p| {
        p.overpowered && !current.overpowered && p.speed == generated && tier(generated) != 0
    });
    let changed_tier = setting_changed
        && !was_overpowered
        && !current.overpowered
        && tier(previous.map_or(old_rpm, |p| p.theoretical)) != tier(generated);
    // GeneratingKineticBlockEntity.tick handles reActivateSource only after
    // super.tick/effects.tick; ordinary setting callbacks queue before it.
    (current, changed_tier, recovered)
}
#[derive(Default)]
struct Server {
    game: usize,
    queues: BTreeMap<(String, Pos), i32>,
    generated: BTreeMap<(String, Pos), Generator>,
    cleanup: Option<(String, i64)>,
    pending: BTreeMap<String, Vec<RotationEvent>>,
}
thread_local! {static SERVER:RefCell<Server>=RefCell::new(Server::default());}
pub(super) fn reset() {
    SERVER.with(|s| *s.borrow_mut() = Server::default());
    CLIENT.with(|c| *c.borrow_mut() = Client::default());
}
pub(super) fn changed(dimension: &str, pos: Pos) {
    SERVER.with(|s| {
        let mut s = s.borrow_mut();
        let key = (dimension.to_owned(), pos);
        s.queues.remove(&key);
        s.generated.remove(&key);
        if let Some(events) = s.pending.get_mut(dimension) {
            events.retain(|e| e.pos != pos);
        }
    });
}
fn prepare_server(server: &mut Server, game: &Game<'_>) {
    let id = std::ptr::from_ref(game) as usize;
    if server.game != id {
        *server = Server {
            game: id,
            ..Server::default()
        };
    }
}
pub(super) fn queue_rotation(game: &Game<'_>, pos: Pos) {
    SERVER.with(|s| {
        let mut s = s.borrow_mut();
        prepare_server(&mut s, game);
        s.queues.insert((game.dim.id().into(), pos), 2);
    });
}
fn radii(block: &str) -> (f32, f32) {
    match block {
        "create:shaft" => (0.125, 0.35),
        "create:large_cogwheel" | "create:water_wheel" => (1.0, 1.125),
        "create:large_water_wheel" => (2.25, 2.5),
        _ => (0.75, 0.65),
    }
}
fn tier(speed: f32) -> i32 {
    if speed.abs() >= 100.0 {
        3
    } else if speed.abs() >= 30.0 {
        2
    } else if speed.abs() >= 1.0 {
        1
    } else {
        0
    }
}
pub(super) fn server_tick(
    game: &mut Game<'_>,
    pos: Pos,
    entity: &mut ModBlockEntity,
    old_rpm: f32,
    rpm: f32,
    generated: Option<f32>,
) {
    let key = (game.dim.id().to_owned(), pos);
    let due = SERVER.with(|s| {
        let mut s = s.borrow_mut();
        prepare_server(&mut s, game);
        let stamp = (game.dim.id().to_owned(), game.world.game_time);
        if s.cleanup.as_ref() != Some(&stamp) && game.world.game_time.rem_euclid(200) == 0 {
            s.cleanup = Some(stamp);
            s.queues.retain(|(dim, p), _| {
                dim != game.dim.id() || super::create_port::owns(&game.native_block(*p))
            });
            s.generated.retain(|(dim, p), _| {
                dim != game.dim.id()
                    || matches!(
                        game.native_block(*p).as_str(),
                        "create:creative_motor"
                            | "create:hand_crank"
                            | "create:water_wheel"
                            | "create:large_water_wheel"
                    )
            });
        }
        let mut after_tick = false;
        if let Some(generated) = generated {
            let theoretical = match entity.nbt.get(&"create_horizon".into()) {
                Some(horizon_world::nbt::Tag::Compound(data)) => {
                    super::create_port::number(data, "TheoreticalSpeed")
                }
                _ => rpm,
            };
            let (state, before_tick, recovered) = generator_transition(
                s.generated.get(&key).copied(),
                old_rpm,
                theoretical,
                generated,
            );
            s.generated.insert(key.clone(), state);
            after_tick = recovered;
            if before_tick {
                s.queues.insert(key.clone(), 2);
            }
        }
        let due = if let Some(count) = s.queues.get_mut(&key) {
            *count -= 1;
            *count == 0
        } else {
            false
        };
        if due {
            s.queues.remove(&key);
        }
        if after_tick {
            s.queues.insert(key, 2);
        }
        due
    });
    if !due {
        return;
    }
    let Some(node) = super::create_port::node(game, pos) else {
        return;
    };
    let (a, b) = radii(&game.native_block(pos));
    let Some(burst) = rules::rotation_burst(rpm, 30.0, 100.0, a, b, node.axis) else {
        return;
    };
    SERVER.with(|s| {
        s.borrow_mut()
            .pending
            .entry(game.dim.id().into())
            .or_default()
            .push(RotationEvent {
                pos,
                color: burst.color,
                speed: burst.speed,
                initial: a,
                target: b,
                axis: node.axis,
            })
    });
}
fn encode(events: &[RotationEvent]) -> Vec<u8> {
    let mut data = PACKET_MAGIC.to_vec();
    for e in events {
        for word in [
            e.pos[0] as u32,
            e.pos[1] as u32,
            e.pos[2] as u32,
            e.color,
            e.speed.to_bits(),
            e.initial.to_bits(),
            e.target.to_bits(),
            e.axis.index() as u32,
        ] {
            data.extend_from_slice(&word.to_le_bytes());
        }
    }
    data
}
fn decode(data: &[u8]) -> Option<Vec<RotationEvent>> {
    if data.len() < 4
        || data.len() > 30_000
        || &data[..4] != PACKET_MAGIC
        || !(data.len() - 4).is_multiple_of(RECORD_LEN)
    {
        return None;
    }
    let mut events = Vec::with_capacity((data.len() - 4) / RECORD_LEN);
    for row in data[4..].as_chunks::<RECORD_LEN>().0 {
        let words: Vec<_> = row
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        let axis = *[Axis::X, Axis::Y, Axis::Z].get(words[7] as usize)?;
        let (speed, initial, target) = (
            f32::from_bits(words[4]),
            f32::from_bits(words[5]),
            f32::from_bits(words[6]),
        );
        if !speed.is_finite()
            || !initial.is_finite()
            || !target.is_finite()
            || !(0.0..=8.0).contains(&initial)
            || !(0.0..=8.0).contains(&target)
        {
            return None;
        }
        events.push(RotationEvent {
            pos: [words[0] as i32, words[1] as i32, words[2] as i32],
            color: words[3],
            speed,
            initial,
            target,
            axis,
        });
    }
    Some(events)
}
pub(super) fn flush(game: &mut Game<'_>, dimension: &str) {
    let events = SERVER.with(|s| s.borrow_mut().pending.remove(dimension).unwrap_or_default());
    if events.is_empty() {
        return;
    }
    let players: Vec<_> = game
        .lan
        .as_ref()
        .map(|host| {
            host.hub
                .guests
                .iter()
                .filter(|g| host.hub.here(g) && g.brand == "horizon")
                .map(|g| (g.uuid, g.position))
                .collect()
        })
        .unwrap_or_default();
    for (uuid, position) in players {
        let selected: Vec<_> = events
            .iter()
            .copied()
            .filter(|e| {
                (0..3)
                    .map(|i| (position[i].floor() - f64::from(e.pos[i])).powi(2))
                    .sum::<f64>()
                    < 32.0 * 32.0
            })
            .collect();
        for batch in selected.chunks(MAX_RECORDS) {
            if let Err(error) = game.native_net_send_to_player(uuid, CHANNEL, &encode(batch)) {
                log::warn!("Create rotation particle batch rejected: {error}");
                break;
            }
        }
    }
}
pub(super) fn packet(game: &mut Game<'_>, channel: &str, data: &[u8]) {
    if channel != CHANNEL
        || !game.native_window()
        || game.remote.is_none()
        || !game.native_on("create")
    {
        return;
    }
    let Some(events) = decode(data) else {
        log::warn!("Create: invalid rotation particle packet");
        return;
    };
    let world = std::ptr::from_ref(&game.world) as usize;
    for e in events {
        spawn_rotation(game, e, world);
    }
}
fn spawn_rotation(game: &mut Game<'_>, event: RotationEvent, world: usize) {
    let RotationEvent {
        pos,
        color,
        speed,
        initial,
        target,
        axis,
    } = event;
    let center = pos.map(|v| f64::from(v) + 0.5);
    let visible = game.armor[0]
        .as_ref()
        .is_some_and(|s| s.item.as_ref() == "create:goggles");
    for _ in 0..20 {
        // ClientPacketListener.handleParticleEvent consumes all six Gaussians
        // even though zero spread and this provider discard position/velocity.
        for _ in 0..6 {
            game.particles.random().next_gaussian();
        }
        let rotation = Rotation {
            origin: center,
            axis,
            speed,
            radius: initial,
            target_radius: target,
        };
        let controller = RotationController {
            rotation,
            fade: rgb(color),
            visible,
            world,
        };
        let Some(mut p) =
            game.particles
                .controlled("create:rotation_indicator", center, true, controller)
        else {
            continue;
        };
        p.lifetime = rules::rotation_lifetime(10, &mut Random(p.random()));
        p.size *= rules::QUAD_SIZE_MULTIPLIER;
        p.rgba[..3].copy_from_slice(&rgb(rules::mix_colors(color, 0xFFFFFF, 0.5)));
        p.rgba[3] = 1.0;
        p.set_visible(visible);
        p.set_full_bright(true);
        let ticks = CLIENT.with(|c| c.borrow().ticks);
        p.position = rotation.position(ticks as f32, 0, p.lifetime);
        p.previous = p.position;
        let p_copy = (*p).clone();
        trace_spawn(game, &p_copy, "rotation_indicator", pos);
        // controlled() already registers the particle
    }
}
/// Catnip AnimationTickHolder.getRenderTime for an ordinary client level.
pub(super) fn animation_time(partial: f32) -> f32 {
    CLIENT.with(|client| client.borrow().ticks as f32 + partial)
}

pub(super) fn client_tick(game: &mut Game<'_>) {
    if !game.native_window() || game.remote.is_none() || !game.native_on("create") {
        return;
    }
    let world = std::ptr::from_ref(&game.world) as usize;
    let game_id = std::ptr::from_ref(game) as usize;
    let time = game.world.game_time;
    let run = CLIENT.with(|c| {
        let mut c = c.borrow_mut();
        if c.world != world
            || c.game != game_id
            || c.dimension != game.dim.id()
            || c.stamp.is_some_and(|old| old > time)
        {
            *c = Client {
                world,
                game: game_id,
                dimension: game.dim.id().into(),
                ..Client::default()
            };
        }
        // Catnip AnimationTickHolder advances on every unpaused client tick,
        // including /tick freeze; BE spawning still follows level game time.
        if !game.pause {
            c.ticks = (c.ticks + 1) % 1_728_000;
        }
        if c.stamp == Some(time) {
            return false;
        }
        c.stamp = Some(time);
        true
    });
    if !run {
        return;
    }
    let entries: Vec<_> = game
        .world
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), e)| {
            let BlockEntity::Mod(e) = e else {
                return None;
            };
            if !e.id.starts_with("create:") {
                return None;
            }
            Some((
                [x, y, z],
                if e.id == "create:encased_fan" {
                    super::create_port_fan::synced_current(e)
                } else {
                    None
                },
            ))
        })
        .collect();
    let mut flows = BTreeMap::new();
    for (pos, current) in entries {
        let Some(current) = current else {
            continue;
        };
        if game.native_block_property(pos, "facing").as_deref() != Some(current.facing.name()) {
            continue;
        }
        flows.insert(
            pos,
            Flow {
                facing: current.facing,
                rpm: current.rpm,
                distance: current.distance,
                bounds: flow_bounds(pos, current.facing, current.distance),
                segments: current.segments,
                handlers: current.handlers,
            },
        );
    }
    CLIENT.with(|c| {
        let mut c = c.borrow_mut();
        c.caught.retain(|p, _| flows.contains_key(p));
        c.flows = flows.clone();
    });
    if !flows.values().any(|f| f.rpm != 0.0) {
        return;
    }
    // One spatial index per client tick, independent of the number of fans.
    let mut items = HashMap::new();
    let mut horses = HashMap::new();
    let mut cells: BTreeMap<[i32; 2], Vec<(bool, u32)>> = BTreeMap::new();
    for e in game.dropped.iter().filter(|e| e.stack.count > 0) {
        items.insert(
            e.id,
            (
                [e.position.x, e.position.y, e.position.z],
                corners(e.bounds()),
            ),
        );
        for cell in cell_range(corners(e.bounds())) {
            cells.entry(cell).or_default().push((false, e.id));
        }
    }
    for m in game
        .mobs
        .iter()
        .filter(|m| !m.removed && m.alive() && m.kind.id == "minecraft:horse")
    {
        horses.insert(
            m.id,
            (
                [m.position.x, m.position.y, m.position.z],
                [m.previous.x, m.previous.y, m.previous.z],
                corners(m.bounds()),
            ),
        );
        for cell in cell_range(corners(m.bounds())) {
            cells.entry(cell).or_default().push((true, m.id));
        }
    }
    for (pos, flow) in flows {
        if flow.rpm == 0.0 {
            CLIENT.with(|c| {
                c.borrow_mut().caught.remove(&pos);
            });
            continue;
        }
        airflow(game, pos, &flow, world);
        let (caught_items, caught_horses) = CLIENT.with(|c| {
            let mut c = c.borrow_mut();
            let caught = c.caught.entry(pos).or_default();
            if caught.search <= 0 {
                caught.search = 5;
                caught.items.clear();
                caught.horses.clear();
                let mut seen = BTreeSet::new();
                for cell in cell_range(flow.bounds) {
                    for &(horse, id) in cells.get(&cell).into_iter().flatten() {
                        if !seen.insert((horse, id)) {
                            continue;
                        }
                        let b = if horse {
                            horses.get(&id).map(|v| v.2)
                        } else {
                            items.get(&id).map(|v| v.1)
                        };
                        if b.is_some_and(|b| intersects(flow.bounds, b)) {
                            if horse {
                                caught.horses.push(id)
                            } else {
                                caught.items.push(id)
                            }
                        }
                    }
                }
            } else {
                caught.search -= 1;
            }
            caught
                .items
                .retain(|id| items.get(id).is_some_and(|v| intersects(flow.bounds, v.1)));
            caught
                .horses
                .retain(|id| horses.get(id).is_some_and(|v| intersects(flow.bounds, v.2)));
            (caught.items.clone(), caught.horses.clone())
        });
        for id in caught_items {
            if let Some((position, _)) = items.get(&id)
                && let Some(kind) = type_at(pos, &flow, *position)
            {
                let extras = rules::processing_particles(
                    kind,
                    *position,
                    &mut Random(&mut game.world.random),
                );
                emit(game, extras);
            }
        }
        for id in caught_horses {
            if let Some((position, previous, _)) = horses.get(&id)
                && type_at(pos, &flow, *position) == Some(ProcessingType::Haunting)
            {
                let extras =
                    rules::horse_haunting_particles(*previous, &mut Random(&mut game.world.random));
                emit(game, extras);
            }
        }
        for (handler, kind) in &flow.handlers {
            let positions = if game.native_block(*handler) == "create:belt" {
                super::create_port_belt::processing_positions(game, *handler, 0.51)
            } else {
                let Some(BlockEntity::Mod(e)) = game
                    .world
                    .block_entities
                    .get(&(handler[0], handler[1], handler[2]))
                else {
                    continue;
                };
                if e.id != "create:depot" || !e.nbt.contains_key(&"HeldItem".into()) {
                    continue;
                }
                vec![handler.map(|v| f64::from(v) + 0.5)]
            };
            for position in positions {
                let extras = rules::processing_particles(
                    *kind,
                    position,
                    &mut Random(&mut game.world.random),
                );
                emit(game, extras);
            }
        }
    }
}

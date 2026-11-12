// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// PressingBehaviour's actual client read/tick boundary, Create 6.0.10.
use super::{Game, create_port as port};
use crate::game::mod_api::{ModBlockEntityExt, ModRevision};
use horizon_client::{
    containers::{BlockEntity, ModBlockEntity},
    items::ItemStack,
};
use super::create_engine::{
    particles::ParticleRandom,
    pressing::{PressClient, PressMode, PressPacket, PressParticleContext},
    transport::SessionCache,
};
use horizon_core::random::RandomSource;
use horizon_world::nbt::{Compound, Tag};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

const ID: &str = "create:mechanical_press";
type Pos = [i32; 3];
#[derive(Default)]
struct Client {
    owner: Option<ModRevision>,
    cycle: PressClient<ItemStack>,
}
#[derive(Default)]
struct World {
    entries: BTreeMap<Pos, Client>,
    tick: Option<u64>,
}
thread_local! {
    static CLIENTS: RefCell<SessionCache<World>> = RefCell::new(SessionCache::default());
}
struct Context<'a, 'res>(&'a mut Game<'res>);
impl ParticleRandom for Context<'_, '_> {
    fn next_float(&mut self) -> f32 {
        self.0.particles.level_random().next_float()
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        self.0
            .particles
            .level_random()
            .next_int_bound(bound)
            .unwrap_or(0)
    }
}
impl PressParticleContext<ItemStack> for Context<'_, '_> {
    fn emit(&mut self, item: &ItemStack, position: [f64; 3], velocity: [f64; 3]) {
        let options = self.0.item_particle(item);
        self.0.particles.add(&options, position, velocity);
    }
}
fn saved(nbt: &Compound) -> Option<PressPacket> {
    Some(PressPacket {
        running: port::integer(nbt, "Running") != 0,
        finished: port::integer(nbt, "Finished") != 0,
        ticks: port::integer(nbt, "Ticks"),
        mode: PressMode::from_index(port::integer(nbt, "Mode"))?,
    })
}
fn bind(client: &mut Client, owner: ModRevision, initial: PressPacket) {
    if !client.owner.is_some_and(|old| old.same_entity(&owner)) {
        *client = Client::default();
        client.owner = Some(owner);
        client.cycle.read(initial);
    }
}
pub(super) fn reset() {
    CLIENTS.with(|cache| cache.borrow_mut().clear());
}

/// This is the pinned HEAD fallback for the exact accepted receipt. Official
/// RETURN planning rejects this function's closure/try control flow. Both a
/// block update and initial chunk tags pass through this one entry point.
pub(super) fn packet(game: &mut Game<'_>, pos: Pos, kind: i32, tag: &Tag) {
    if !game.native_window() || !game.native_on("create") {
        return;
    }
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
    if name != ID || game.native_block(pos) != ID {
        return;
    }
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    if entity.id != ID {
        return;
    }
    let Some(packet) = saved(fields) else {
        return;
    };
    let owner = entity.revision();
    let items = match fields.get(&"ParticleItems".into()) {
        Some(Tag::List(items)) => items
            .iter()
            .filter_map(horizon_items::item_nbt::item_from_nbt)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    // Capture only session/dimension before borrowing the client RNG; no
    // reference to Game or a block entity is retained in the cache.
    let world = std::ptr::from_ref(&game.world) as usize;
    let dimension = game.dim.id().to_owned();
    let count = CLIENTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let client = cache
            .dimension(world, &dimension)
            .entries
            .entry(pos)
            .or_default();
        bind(client, owner, packet);
        client
            .cycle
            .read_packet(packet, items, pos, &mut Context(game))
    });
    if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
        log::info!(
            "Create press client: {}",
            serde_json::json!({
                "event":"packet", "pos":pos, "running":packet.running,
                "finished":packet.finished, "ticks":packet.ticks,
                "mode":packet.mode as i32, "particle_requests":count,
            })
        );
    }
}

/// Only call from the actual client Level.tickBlockEntities boundary. The
/// existing tick_block_entity_anims HEAD runs inside clock.world_runs(),
/// unlike native_tick, which also runs during freeze and loading screens.
pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() || game.remote.is_none() || !game.native_on("create") {
        return;
    }
    let rows = game
        .world
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), entity)| {
            let BlockEntity::Mod(entity) = entity else {
                return None;
            };
            let pos = [x, y, z];
            if entity.id != ID || game.native_block(pos) != ID {
                return None;
            }
            Some((
                pos,
                entity.revision(),
                saved(&entity.nbt)?,
                port::running_speed(game, &entity.nbt),
            ))
        })
        .collect::<Vec<_>>();
    let world = std::ptr::from_ref(&game.world) as usize;
    let time = game.ticks;
    CLIENTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let world = cache.dimension(world, game.dim.id());
        if world.tick == Some(time) {
            return;
        }
        world.tick = Some(time);
        let live = rows.iter().map(|row| row.0).collect::<BTreeSet<_>>();
        world.entries.retain(|pos, _| live.contains(pos));
        for (pos, owner, initial, rpm) in rows {
            let client = world.entries.entry(pos).or_default();
            bind(client, owner, initial);
            client.cycle.tick(rpm, false, true);
            if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
                log::info!(
                    "Create press client: {}",
                    serde_json::json!({
                        "event":"tick", "pos":pos, "client_tick":time,
                        "running":client.cycle.running, "finished":client.cycle.finished,
                        "ticks":client.cycle.ticks, "previous_ticks":client.cycle.previous_ticks,
                        "mode":client.cycle.mode as i32,
                    })
                );
            }
        }
    });
}

/// Actual source prev/current interpolation, with a scalar-only cold fallback.
/// Saved ParticleItems are never polled or replayed by drawing or ticking.
pub(super) fn travel(game: &Game<'_>, pos: Pos, entity: &ModBlockEntity, partial: f32) -> f32 {
    CLIENTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let world = cache.dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id());
        world
            .entries
            .get(&pos)
            .filter(|client| {
                let revision = entity.revision();
                client
                    .owner
                    .is_some_and(|old| old.same_entity(&revision))
            })
            .map_or_else(
                || {
                    let mut cycle = PressClient::<ItemStack>::default();
                    if let Some(packet) = saved(&entity.nbt) {
                        cycle.read(packet);
                    }
                    cycle.rendered_travel(partial)
                },
                |client| client.cycle.rendered_travel(partial),
            )
    })
}

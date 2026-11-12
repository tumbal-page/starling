// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Loaded connected-component equations are cached per tick. Source generation
// is published by initialize/updateGeneratedRotation, not by reading a setting.
// This preserves supported stationary-machine event ordering; it is not a
// complete RotationPropagator implementation (source ownership/flicker/breakage).
// Adapted for Android bundled port.

use super::Game;
use horizon_client::containers::{BlockEntity, ModBlockEntity};
use crate::game::mod_api::{ModBlockEntityExt, ModRevision};
use super::create_engine::{
    kinetics::{self, Kind, Network, Node, Pos},
    transport::SessionCache,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
#[derive(Default)]
struct Cache {
    world: Option<(usize, i64, String)>,
    components: BTreeMap<(Pos, u8), Rc<Network>>,
    published: BTreeSet<usize>,
}
impl Cache {
    fn invalidate(&mut self) {
        self.components.clear();
        self.published.clear();
    }
    fn first_publication(&mut self, network: &Network) -> bool {
        self.published.insert(std::ptr::from_ref(network) as usize)
    }
}
#[derive(Default)]
struct Lifetime {
    owner: Option<ModRevision>,
    attached: bool,
    waiting_for_neighbour: bool,
    initialized_source: bool,
    reactivate_source: bool,
    generation: Option<f32>,
}
impl Lifetime {
    fn callback(&mut self, requested: f32) -> f32 {
        // updateGeneratedRotation is also called outside the BE's own tick.
        // It does not initialize SmartBlockEntity or consume reActivateSource.
        self.generation = Some(requested);
        requested
    }
    fn source_turn(
        &mut self,
        requested: f32,
        initialize_motor: bool,
        changed: bool,
    ) -> (f32, bool) {
        // KineticBlockEntity.switchToBlockState is the explicit wrench path;
        // Generating.tick consumes reActivateSource on its next own turn.
        // A raw state change or NBT read does not call that source callback.
        let re_activate = std::mem::take(&mut self.reactivate_source);
        let publish = !self.initialized_source && initialize_motor || changed || re_activate;
        self.initialized_source = true;
        if publish {
            self.generation = Some(requested);
        }
        (self.generation.unwrap_or(0.0), publish)
    }
}
#[derive(Default)]
struct Lifetimes {
    nodes: BTreeMap<Pos, Lifetime>,
    cleaned: Option<i64>,
}
impl Lifetimes {
    fn maintain(&mut self, now: i64, mut live: impl FnMut(Pos, ModRevision) -> bool) {
        if now.rem_euclid(20) != 0 || self.cleaned == Some(now) {
            return;
        }
        self.cleaned = Some(now);
        self.nodes
            .retain(|pos, state| state.owner.is_some_and(|owner| live(*pos, owner)));
    }
}
const MAX_PENDING_TRANSMISSIONS: usize = 2048;
#[derive(Default)]
struct Pending {
    positions: BTreeSet<Pos>,
    scan_loaded: bool,
}
thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
    static CHANGED: RefCell<BTreeMap<String, Pending>> = const { RefCell::new(BTreeMap::new()) };
    static LIFETIMES: RefCell<SessionCache<Lifetimes>> = RefCell::new(SessionCache::default());
}
fn with_lifetime<R>(
    game: &Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    action: impl FnOnce(&mut Lifetime) -> R,
) -> R {
    LIFETIMES.with(|cell| {
        let mut cache = cell.borrow_mut();
        let world = cache.dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id());
        let state = world.nodes.entry(pos).or_default();
        let owner = entity.revision();
        if state.owner.is_some_and(|old| !old.same_entity(&owner)) {
            *state = Lifetime::default();
        }
        state.owner = Some(owner);
        action(state)
    })
}
pub(super) fn reset() {
    invalidate();
    LIFETIMES.with(|cell| cell.borrow_mut().clear());
    CHANGED.with(|cell| cell.borrow_mut().clear());
}
pub(super) fn invalidate() {
    CACHE.with(|c| c.borrow_mut().invalidate());
}
fn theoretical(entity: &ModBlockEntity) -> f32 {
    match entity.nbt.get(&"create_horizon".into()) {
        Some(horizon_world::nbt::Tag::Compound(data)) => {
            crate::game::create_port::number(data, "TheoreticalSpeed")
        }
        _ => crate::game::create_port::number(&entity.nbt, "Speed"),
    }
}
fn generation(game: &Game<'_>, pos: Pos, entity: &ModBlockEntity) -> f32 {
    with_lifetime(game, pos, entity, |state| {
        *state.generation.get_or_insert_with(|| theoretical(entity))
    })
}
fn published_node(game: &Game<'_>, mut node: Node) -> Node {
    if let Some(BlockEntity::Mod(entity)) =
        game.world
            .block_entities
            .get(&(node.pos[0], node.pos[1], node.pos[2]))
        && !entity.id.is_empty()
        && let Kind::Motor { rpm, .. } | Kind::WaterWheel { rpm, .. } = &mut node.kind
    {
        *rpm = generation(game, node.pos, entity);
    }
    node
}
/// CreativeMotor.initialize publishes its setting. Other generators require
/// their own real turn/expiry/flow callback even on their first BE turn. Reading ScrollValue via /data is not that event.
pub(super) fn source_turn(
    game: &Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    requested: f32,
    initialize_motor: bool,
    changed: bool,
) -> (f32, bool) {
    let result = with_lifetime(game, pos, entity, |state| {
        state.generation.get_or_insert_with(|| theoretical(entity));
        state.source_turn(requested, initialize_motor, changed)
    });
    if result.1 {
        invalidate();
    }
    result
}
/// A newborn passive node attaches from an immediate neighbour's *actual*
/// rotation. A distant loaded motor behind an unticked shaft is not a shortcut.
/// Synchronous propagation may already have supplied this node before its turn.
pub(super) fn attach(
    game: &Game<'_>,
    node: Node,
    entity: &ModBlockEntity,
    potential: f32,
) -> (bool, bool) {
    let (first, waiting) = with_lifetime(game, node.pos, entity, |state| {
        let first = !state.attached;
        state.attached = true;
        (first, state.waiting_for_neighbour)
    });
    if first && std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
        log::info!(
            "Create kinetic turn: {}",
            serde_json::json!({
                "time": game.world.game_time, "pos": node.pos,
                "speed": crate::game::create_port::number(&entity.nbt, "Speed"),
                "needs_update": crate::game::create_port::integer(&entity.nbt, "NeedsUpdate"),
            })
        );
    }
    if (!first && !waiting) || theoretical(entity) != 0.0 {
        with_lifetime(game, node.pos, entity, |state| {
            state.waiting_for_neighbour = false
        });
        return (first, false);
    }
    if potential != 0.0 {
        for x in -1i32..=1 {
            for y in -1i32..=1 {
                for z in -1i32..=1 {
                    if !(1..=2).contains(&(x.abs() + y.abs() + z.abs())) {
                        continue;
                    }
                    let pos = [node.pos[0] + x, node.pos[1] + y, node.pos[2] + z];
                    let Some(other) = crate::game::create_port::node(game, pos) else {
                        continue;
                    };
                    if kinetics::ratio(node, other).is_none() {
                        continue;
                    }
                    if let Some(BlockEntity::Mod(neighbour)) =
                        game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
                        && !neighbour.id.is_empty()
                        && theoretical(neighbour) != 0.0
                    {
                        with_lifetime(game, node.pos, entity, |state| {
                            state.waiting_for_neighbour = false
                        });
                        return (true, false);
                    }
                }
            }
        }
    }
    with_lifetime(game, node.pos, entity, |state| {
        state.waiting_for_neighbour = true
    });
    (first, true)
}
pub(super) fn network(game: &Game<'_>, start: Node, half: u8) -> Rc<Network> {
    let start = published_node(game, start);
    CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        let world = (
            std::ptr::from_ref(&game.world) as usize,
            game.world.game_time,
            game.dim.id().to_owned(),
        );
        if cache.world.as_ref() != Some(&world) {
            cache.world = Some(world);
            cache.invalidate();
        }
        if let Some(network) = cache.components.get(&(start.pos, half)) {
            return Rc::clone(network);
        }
        let network = Rc::new(kinetics::solve_half(start, half, |pos| {
            if pos == start.pos {
                Some(start)
            } else {
                crate::game::create_port::node(game, pos).map(|node| published_node(game, node))
            }
        }));
        for key in &network.vertices {
            cache.components.insert(*key, Rc::clone(&network));
        }
        network
    })
}
/// updateGeneratedRotation/attachKinetics publishes to neighbours immediately,
/// including machines whose behaviour turn already ran in this level pass.
pub(super) fn publish(game: &mut Game<'_>, network: &Network, current: Pos) {
    // A propagated source already visits every neighbour. Their subsequent
    // first own turns must not repeat that full-component scan (N squared).
    // Each source event invalidates both the solved graph and this guard.
    if !CACHE.with(|cell| cell.borrow_mut().first_publication(network)) {
        return;
    }

    for &(pos, _) in &network.vertices {
        if pos == current {
            continue;
        }
        let Some(node) = crate::game::create_port::node(game, pos) else {
            continue;
        };
        let other = matches!(node.kind, Kind::Clutch { powered: true }).then(|| {
            let half = network
                .vertices
                .iter()
                .find(|(p, _)| *p == pos)
                .map_or(0, |(_, h)| 1 - *h);
            self::network(game, node, half)
        });
        let Some(BlockEntity::Mod(entity)) =
            game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
        else {
            continue;
        };
        if entity.id.is_empty()
            || crate::game::create_port::network_matches(entity, network, None, pos)
        {
            continue;
        }
        game.native_with_mod_block_entity(pos, |entity| {
            crate::game::create_port::apply_network(entity, network, None, pos);
        });
        game.native_block_entity_sync(pos);
    }
}
/// Successful generator wrench rotation, distinct from raw state/NBT reads.
/// switchToBlockState marks it now; Generating.tick applies it next own turn.
pub(super) fn wrenched(game: &Game<'_>, pos: Pos) {
    if let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
        && !entity.id.is_empty()
    {
        with_lifetime(game, pos, entity, |state| state.reactivate_source = true);
    }
}
/// Ordinary source-setting callback while the source BE is in the world.
pub(super) fn generated_changed(game: &mut Game<'_>, pos: Pos) {
    let Some(mut node) = crate::game::create_port::node(game, pos) else {
        return;
    };
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    let (Kind::Motor { rpm, .. } | Kind::WaterWheel { rpm, .. }) = &mut node.kind else {
        return;
    };
    *rpm = with_lifetime(game, pos, entity, |state| state.callback(*rpm));
    invalidate();
    let network = self::network(game, node, 0);
    game.native_with_mod_block_entity(pos, |entity| {
        crate::game::create_port::apply_network(entity, &*network, None, pos);
    });
    publish(game, &network, pos);
    game.native_block_entity_sync(pos);
}

/// The block notification hook has no Game borrow. Retain bounded nearby
/// transmission candidates for the next ordinary level's block phase.
pub(super) fn changed(dimension: &str, pos: Pos) {
    invalidate();
    CHANGED.with(|cell| {
        let mut changes = cell.borrow_mut();
        let pending = changes.entry(dimension.to_owned()).or_default();
        if pending.scan_loaded {
            return;
        }
        if pending.positions.len() + 7 > MAX_PENDING_TRANSMISSIONS {
            pending.positions.clear();
            pending.scan_loaded = true;
            return;
        }
        pending.positions.insert(pos);
        for direction in super::create_engine::placement::Direction::ALL {
            let delta = direction.vector();
            pending
                .positions
                .insert(std::array::from_fn(|i| pos[i] + delta[i]));
        }
    });
}
/// Clutch/Gearshift reopening is a scheduled block tick before the BE pass.
/// This preserves that next-tick machine boundary. It does not claim immediate
/// between-tick neighbour callback parity: the current hook has no Game owner.
pub(super) fn refresh_transmissions(game: &mut Game<'_>) {
    LIFETIMES.with(|cell| {
        cell.borrow_mut().dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .maintain(game.world.game_time, |pos, owner| {
                matches!(game.world.block_entities.get(&(pos[0], pos[1], pos[2])),
                    Some(BlockEntity::Mod(entity)) if !entity.id.is_empty() && owner.same_entity(&entity.revision()))
            });
    });

    let pending = CHANGED.with(|cell| cell.borrow_mut().remove(game.dim.id()).unwrap_or_default());
    let positions: Vec<Pos> = if pending.scan_loaded {
        game.world
            .block_entities
            .iter()
            .filter_map(|(&(x, y, z), _)| {
                let pos = [x, y, z];
                matches!(
                    game.native_block(pos).as_str(),
                    "create:clutch" | "create:gearshift"
                )
                .then_some(pos)
            })
            .collect()
    } else {
        pending.positions.into_iter().collect()
    };
    for pos in positions {
        let block = game.native_block(pos);
        if !matches!(block.as_str(), "create:clutch" | "create:gearshift") {
            continue;
        }
        let dimension = game.dim.id().to_owned();
        let powered = game.native_neighbor_signal(&dimension, pos) > 0;
        if !game.native_set_block_property(pos, "powered", if powered { "true" } else { "false" }) {
            continue;
        }
        invalidate();
        let Some(node) = crate::game::create_port::node(game, pos) else {
            continue;
        };
        let network = self::network(game, node, 0);
        let other = matches!(node.kind, Kind::Clutch { powered: true })
            .then(|| self::network(game, node, 1));
        game.native_with_mod_block_entity(pos, |entity| {
            crate::game::create_port::apply_network(entity, &*network, None, pos);
        });
        publish(game, &network, pos);
        if let Some(other) = other {
            publish(game, &other, pos);
        }
        game.native_block_entity_sync(pos);
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    #[test]
    fn a_component_publishes_once_until_source_or_tick_invalidation() {
        let empty = || Network {
            status: kinetics::Status::Idle,
            capacity: 0.0,
            stress: 0.0,
            theoretical: BTreeMap::new(),
            ports: BTreeMap::new(),
            vertices: Vec::new(),
        };
        let first = Rc::new(empty());
        let second = Rc::new(empty());
        let mut cache = Cache::default();
        assert!(cache.first_publication(&first));
        assert!(!cache.first_publication(&first));
        assert!(cache.first_publication(&second));
        cache.invalidate();
        assert!(
            cache.first_publication(&first),
            "a new source/tick generation must publish again"
        );
        assert!(!cache.first_publication(&first));
    }
    #[test]
    fn explicit_wrench_marker_reactivates_on_own_turn_while_raw_settings_stay_inert() {
        let mut motor = Lifetime::default();
        assert_eq!(motor.source_turn(32.0, true, false), (32.0, true));
        // /data and equivalent raw state changes do not set the marker.
        assert_eq!(motor.source_turn(64.0, true, false), (32.0, false));
        motor.reactivate_source = true;
        assert_eq!(motor.source_turn(-64.0, true, false), (-64.0, true));
        assert!(!motor.reactivate_source);
        assert_eq!(motor.source_turn(-128.0, true, false), (-128.0, false));
        assert_eq!(motor.source_turn(-128.0, true, true), (-128.0, true));
        for speed in [8.0, 32.0] {
            let mut source = Lifetime {
                generation: Some(speed),
                ..Lifetime::default()
            };
            assert_eq!(source.source_turn(64.0, false, false), (speed, false));
            source.reactivate_source = true;
            assert_eq!(source.source_turn(-speed, false, false), (-speed, true));
            assert_eq!(source.source_turn(0.0, false, true), (0.0, true));
        }
        let mut newborn = Lifetime {
            reactivate_source: true,
            ..Lifetime::default()
        };
        assert_eq!(newborn.callback(64.0), 64.0);
        assert!(
            !newborn.initialized_source,
            "callback is not SmartBlockEntity.initialize"
        );
        assert!(
            newborn.reactivate_source,
            "callback must not consume a pending wrench turn"
        );
        assert_eq!(newborn.source_turn(32.0, true, false), (32.0, true));
        assert!(!newborn.reactivate_source);
        assert_eq!(newborn.source_turn(128.0, true, false), (32.0, false));
        let mut stopped = Lifetime::default();
        assert_eq!(stopped.source_turn(0.0, true, false), (0.0, true));
        assert_eq!(stopped.source_turn(32.0, true, false), (0.0, false));
    }
    #[test]
    fn pending_bulk_changes_fall_back_to_one_loaded_scan_without_dropping_the_dimension() {
        reset();
        for x in 0..MAX_PENDING_TRANSMISSIONS as i32 {
            changed("minecraft:overworld", [x * 4, 70, 0]);
        }
        changed("minecraft:the_nether", [1, 70, 0]);
        CHANGED.with(|cell| {
            let pending = cell.borrow();
            assert!(pending["minecraft:overworld"].scan_loaded);
            assert!(pending["minecraft:overworld"].positions.is_empty());
            assert!(!pending["minecraft:the_nether"].scan_loaded);
            assert_eq!(pending["minecraft:the_nether"].positions.len(), 7);
        });
        reset();
        CHANGED.with(|cell| assert!(cell.borrow().is_empty()));
    }
    #[test]
    fn lifetime_cleanup_preserves_live_generation_and_drops_replaced_or_removed_owners() {
        let mut live = ModBlockEntity::default();
        let original = live.revision();
        let removed = ModBlockEntity::default();
        let replaced = ModBlockEntity::default();
        let replacement = replaced.clone();
        let mut states = Lifetimes::default();
        for (pos, owner) in [
            ([1, 0, 0], original),
            ([2, 0, 0], removed.revision()),
            ([3, 0, 0], replaced.revision()),
        ] {
            states.nodes.insert(
                pos,
                Lifetime {
                    owner: Some(owner),
                    generation: Some(32.0),
                    initialized_source: true,
                    ..Lifetime::default()
                },
            );
        }
        // A normal content revision is the same entity; a clone is a new one.
        live.nbt
            .insert("ScrollValue".into(), horizon_world::nbt::Tag::Int(64));
        states.maintain(20, |pos, owner| match pos[0] {
            1 => owner.same_entity(&live.revision()),
            3 => owner.same_entity(&replacement.revision()),
            _ => false,
        });
        assert_eq!(states.nodes.len(), 1);
        assert_eq!(
            states.nodes[&[1, 0, 0]].generation,
            Some(32.0),
            "live /data read must not turn into a setting callback"
        );
        assert!(states.nodes[&[1, 0, 0]].initialized_source);
        states.maintain(20, |_, _| panic!("frozen repeat must not rescan"));
        states.maintain(21, |_, _| panic!("cleanup must be at most once per20ticks"));
        states.maintain(40, |_, _| false);
        assert!(states.nodes.is_empty());
    }
}
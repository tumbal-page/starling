// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Stationary DrillBlockEntity and BlockBreakingKineticBlockEntity adapter.
use super::{Game, create_engine};
use crate::game::native_block_entity_draw::BlockEntityDraw;
use horizon_client::containers::ModBlockEntity;
use super::create_engine::{
    drilling::{Action, DrillCycle},
    placement::Direction,
};
use horizon_protocol::packets::play::{self, Clientbound};
use horizon_world::nbt::Tag;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicI32, Ordering},
};

const ID: &str = "create:mechanical_drill";
static NEXT_BREAKER: AtomicI32 = AtomicI32::new(-1);

struct Running {
    lazy: i32,
    dirty: bool,
    breaker: i32,
    seen: i64,
    target: [i32; 3],
}
#[derive(Default)]
struct DimensionState {
    drills: BTreeMap<[i32; 3], Running>,
    cleaned: Option<i64>,
}
#[derive(Default)]
struct State {
    game: Option<usize>,
    dimensions: BTreeMap<String, DimensionState>,
}
impl State {
    fn dimension(&mut self, game: usize, dimension: &str) -> &mut DimensionState {
        if self.game != Some(game) {
            self.game = Some(game);
            self.dimensions.clear();
        }
        self.dimensions.entry(dimension.to_owned()).or_default()
    }
}
thread_local! {static STATE: RefCell<State> = RefCell::new(State::default());}

pub(super) fn reset(reload: bool) {
    if !reload {
        STATE.with(|state| *state.borrow_mut() = State::default());
    }
}

/// DrillBlock.neighborChanged: wakes progress after one intervening tick.
/// Called by the existing block-change hook; only registered drill neighbours
/// are touched, so world generation does not create an unbounded change log.
pub(super) fn changed(dimension: &str, pos: [i32; 3]) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(dimension) = state.dimensions.get_mut(dimension) {
            for offset in [
                [0, 0, 0],
                [1, 0, 0],
                [-1, 0, 0],
                [0, 1, 0],
                [0, -1, 0],
                [0, 0, 1],
                [0, 0, -1],
            ] {
                let neighbor = std::array::from_fn(|i| pos[i] + offset[i]);
                if let Some(drill) = dimension.drills.get_mut(&neighbor) {
                    drill.dirty = true;
                }
            }
        }
    });
}

/// Clear destroyed or unloaded drill cracks once per server tick. This also
/// bounds transient state without scanning every drill for every drill tick.
pub(super) fn maintenance(game: &mut Game<'_>) {
    let removed = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.dimension(std::ptr::from_ref(game) as usize, game.dim.id());
        let time = game.world.game_time;
        if state.cleaned == Some(time) {
            return Vec::new();
        }
        state.cleaned = Some(time);
        let mut removed = Vec::new();
        state.drills.retain(|pos, value| {
            let live =
                time >= value.seen && time - value.seen < 400 && game.native_block(*pos) == ID;
            if !live {
                removed.push((value.breaker, value.target));
            }
            live
        });
        removed
    });
    for (breaker, target) in removed {
        crack(game, breaker, target, -1);
    }
}

fn facing(game: &Game<'_>, pos: [i32; 3]) -> Direction {
    let name = game.native_block_property(pos, "facing");
    Direction::ALL
        .into_iter()
        .find(|d| Some(d.name()) == name.as_deref())
        .unwrap_or(Direction::North)
}

fn crack(game: &mut Game<'_>, breaker: i32, target: [i32; 3], progress: i32) {
    game.destroy_block_progress(breaker, target, progress);
    if let Some(host) = game.lan.as_ref() {
        host.hub.broadcast_near(
            target.map(|v| f64::from(v) + 0.5),
            64.0,
            &Clientbound::BlockDestruction(play::BlockDestruction {
                id: breaker,
                pos: target,
                progress: progress as u8,
            }),
            None,
        );
    }
}

/// Called after the network has written effective RPM, including overstress.
pub(super) fn tick(
    game: &mut Game<'_>,
    pos: [i32; 3],
    entity: &mut ModBlockEntity,
    rpm: f32,
) -> bool {
    if !matches!(entity.id.as_str(), ID | "create:drill") {
        return false;
    }
    let target = {
        let delta = facing(game, pos).vector();
        std::array::from_fn(|i| pos[i] + delta[i])
    };
    let time = game.world.game_time;
    let (lazy, dirty, breaker) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.dimension(std::ptr::from_ref(game) as usize, game.dim.id());
        let fresh = !state.drills.contains_key(&pos);
        let state = state.drills.entry(pos).or_insert_with(|| Running {
            lazy: 10,
            dirty: false,
            breaker: NEXT_BREAKER.fetch_sub(1, Ordering::Relaxed),
            seen: time,
            target,
        });
        let lazy = fresh || state.lazy <= 0;
        if state.lazy <= 0 {
            state.lazy = 10;
        } else {
            state.lazy -= 1;
        }
        state.seen = time;
        state.target = target;
        (lazy, std::mem::take(&mut state.dirty), state.breaker)
    });
    let mut cycle = DrillCycle {
        progress: crate::game::create_port::integer(&entity.nbt, "Progress"),
        next_tick: crate::game::create_port::integer(&entity.nbt, "NextTick"),
    };
    if lazy {
        cycle.lazy_tick();
    }
    if dirty {
        cycle.wake();
    }
    if rpm != 0.0 {
        entity
            .nbt
            .insert("Breaking".into(), Tag::IntArray(target.to_vec()));
    }
    let state_id = game.world.block(target[0], target[1], target[2]);
    let facts = game.res.facts().get(state_id);
    let hardness = facts.hardness;
    let air = matches!(
        facts.name.as_ref(),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    );
    // A waterlogged solid is not a liquid block. DrillBlock also accepts a
    // waterlogged machine casing, independently of the fluid it contains.
    let liquid = matches!(facts.name.as_ref(), "minecraft:water" | "minecraft:lava");
    let non_breakable = game
        .pack
        .assets
        .registry
        .state(state_id)
        .is_some_and(|state| {
            game.native_server_block_in_tag(state, "create:non_breakable")
                .unwrap_or(false)
        });
    let hit = facts.sounds.hit.to_string();
    let action = cycle.tick(
        rpm,
        hardness,
        !air && !liquid && hardness != -1.0 && !non_breakable,
    );
    if matches!(action, Action::Hit | Action::Break) {
        game.with_level(|_, level, _| {
            level.native_play_sound(&hit, pos.map(|v| f64::from(v) + 0.5), 4, 0.25, 1.0);
        });
    }
    match action {
        Action::Hit => crack(game, breaker, target, cycle.progress),
        Action::Clear => crack(game, breaker, target, -1),
        Action::Break => {
            // The normal Level destruction path rolls block loot with no
            // player and an empty tool, preserves fluid, spills block entities,
            // honours block_drops and sends destroy particles to all clients.
            // It deliberately never calls the acting player's mining path.
            if let Err(error) = game.destroy_block_dropping(target) {
                log::warn!("Create drill could not destroy {target:?}: {error:?}");
            }
            crack(game, breaker, target, -1);
        }
        Action::None => {}
    }
    let before_progress = crate::game::create_port::integer(&entity.nbt, "Progress");
    entity
        .nbt
        .insert("Progress".into(), Tag::Int(cycle.progress));
    entity
        .nbt
        .insert("NextTick".into(), Tag::Int(cycle.next_tick));
    before_progress != cycle.progress || action == Action::Break
}

pub(super) fn draw(
    game: &mut Game<'_>,
    pos: [i32; 3],
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) -> bool {
    if !matches!(entity.id.as_str(), ID | "create:drill") {
        return false;
    }
    let direction = facing(game, pos);
    let axis = direction.axis();
    let angle = super::create_engine::visual::angle(
        draw.game_time as f64 + f64::from(draw.partial),
        crate::game::create_port::number(&entity.nbt, "Speed"),
        super::create_engine::visual::rotation_offset(pos, axis, false),
    );
    draw.push();
    draw.rotate_about(super::create_engine::visual::positive_axis(axis), angle, [0.5; 3]);
    crate::game::create_port::face_south(draw, direction);
    game.native_draw_model(draw, "create:block/mechanical_drill/head", 0xFFFFFF);
    draw.pop();
    true
}

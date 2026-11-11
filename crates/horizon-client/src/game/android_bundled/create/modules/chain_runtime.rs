// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// ChainDriveBlock: native state/shape/wrench boundary. Simulation is in engine.
// Adapted for Android bundled port.

use super::{Game, GameLevel};
use android_bundled::create::crates::engine::{
    chain_drive::{self, Part, State},
    kinetics::Axis,
    placement::Direction,
};
use horizon_world::{
    level::PlacementContext,
    registry::{BlockRegistry, BlockStateId},
};

const ID: &str = "create:encased_chain_drive";

pub(super) fn read(registry: &BlockRegistry, id: BlockStateId) -> Option<State> {
    let state = registry.state(id)?;
    if state.name != ID {
        return None;
    }
    Some(State {
        axis: match state.properties.get("axis")?.as_str() {
            "x" => Axis::X,
            "y" => Axis::Y,
            "z" => Axis::Z,
            _ => return None,
        },
        first: state.properties.get("axis_along_first")? == "true",
        part: match state.properties.get("part")?.as_str() {
            "start" => Part::Start,
            "middle" => Part::Middle,
            "end" => Part::End,
            "none" => Part::None,
            _ => return None,
        },
    })
}

fn encode(registry: &BlockRegistry, id: BlockStateId, value: State) -> Option<BlockStateId> {
    let mut state = registry.state(id)?.clone();
    state.properties.insert(
        "axis".into(),
        match value.axis {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
        .into(),
    );
    state.properties.insert(
        "axis_along_first".into(),
        if value.first { "true" } else { "false" }.into(),
    );
    state.properties.insert(
        "part".into(),
        match value.part {
            Part::Start => "start",
            Part::Middle => "middle",
            Part::End => "end",
            Part::None => "none",
        }
        .into(),
    );
    registry.id(&state)
}

fn neighbors(
    registry: &BlockRegistry,
    pos: [i32; 3],
    mut block: impl FnMut([i32; 3]) -> BlockStateId,
) -> [Option<State>; 6] {
    Direction::ALL.map(|direction| {
        let delta = direction.vector();
        read(registry, block(std::array::from_fn(|i| pos[i] + delta[i])))
    })
}

pub(super) fn placement(
    game: &GameLevel<'_>,
    state: BlockStateId,
    context: &PlacementContext,
) -> Option<BlockStateId> {
    let pos = [context.pos.x, context.pos.y, context.pos.z];
    let value = chain_drive::placement(
        crate::game::create_port::nearest(context.yaw, context.pitch),
        context.sneaking,
        crate::game::create_port_placement::level_ports(game, pos),
        neighbors(game.registry, pos, |p| game.world.block(p[0], p[1], p[2])),
    );
    encode(game.registry, state, value)
}

pub(super) fn shape(
    game: &GameLevel<'_>,
    state: BlockStateId,
    direction: horizon_core::geometry::Direction,
    neighbor: BlockStateId,
    new: &mut Option<BlockStateId>,
) {
    let Some(value) = read(game.registry, state) else {
        return;
    };
    let direction = match direction {
        horizon_core::geometry::Direction::Down => Direction::Down,
        horizon_core::geometry::Direction::Up => Direction::Up,
        horizon_core::geometry::Direction::North => Direction::North,
        horizon_core::geometry::Direction::South => Direction::South,
        horizon_core::geometry::Direction::West => Direction::West,
        horizon_core::geometry::Direction::East => Direction::East,
    };
    *new = Some(
        encode(
            game.registry,
            state,
            value.update(direction, read(game.registry, neighbor)),
        )
        .unwrap_or(state),
    );
}

pub(super) fn wrench(game: &mut Game<'_>, pos: [i32; 3], face: Direction) -> bool {
    let registry = game.pack.assets.registry;
    let state = game.world.block(pos[0], pos[1], pos[2]);
    let Some(value) = read(registry, state) else {
        return false;
    };
    let value = value.wrenched(
        face,
        neighbors(registry, pos, |p| game.world.block(p[0], p[1], p[2])),
    );
    let Some(new) = encode(registry, state, value) else {
        return false;
    };
    // One ordinary Level.setBlock, with the complete rebuilt state. The same
    // block retains its entity; do not expose intermediate property writes.
    let dimension = game.dim.id().to_owned();
    game.native_set_block(&dimension, pos, new, 3)
}
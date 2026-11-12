// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Vanilla placement remains responsible for obstruction, permissions and consumption.
use super::{Game, GameLevel};
use horizon_client::items::ItemStack;
use super::create_engine::{
    kinetics::Axis,
    placement::{self, Direction},
};
use horizon_core::{
    geometry::{Direction as D, Vec3},
    position::BlockPos,
};
use horizon_world::{
    level::{PlaceContext, PlaceOutcome, PlacementContext},
    registry::BlockStateId,
};

fn direction(d: D) -> Direction {
    match d {
        D::Down => Direction::Down,
        D::Up => Direction::Up,
        D::North => Direction::North,
        D::South => Direction::South,
        D::West => Direction::West,
        D::East => Direction::East,
    }
}
fn ports(game: &Game<'_>, pos: [i32; 3]) -> [bool; 6] {
    Direction::ALL.map(|d| {
        let delta = d.vector();
        super::create_port::node(game, std::array::from_fn(|i| pos[i] + delta[i]))
            .is_some_and(|n| n.shaft_port(delta.map(|v| -v)))
    })
}
fn horizontal(yaw: f32) -> Direction {
    [
        Direction::South,
        Direction::West,
        Direction::North,
        Direction::East,
    ][((f64::from(yaw) / 90.0 + 0.5).floor() as i64 & 3) as usize]
}
pub(super) fn state(
    game: &GameLevel<'_>,
    mut state: BlockStateId,
    ctx: &PlacementContext,
    answer: &mut Option<Option<BlockStateId>>,
) {
    let block = game.native_state_block(state).unwrap_or("");
    if block == "create:encased_chain_drive" {
        *answer = Some(super::create_port_chain::placement(game, state, ctx));
        return;
    }
    if block == "create:blaze_burner" {
        // BlazeBurnerBlock.getStateForPlacement reads the actual BlockItem,
        // including the empty item alias. Other placement callers get the
        // block default unchanged, as in the original instanceof guard.
        let heat = match ctx.item.as_str() {
            "create:blaze_burner" => "smouldering",
            "create:empty_blaze_burner" => "none",
            _ => {
                *answer = Some(Some(state));
                return;
            }
        };
        state = game
            .native_state_with_property(state, "blaze", heat)
            .unwrap_or(state);
        state = game
            .native_state_with_property(state, "facing", horizontal(ctx.yaw).opposite().name())
            .unwrap_or(state);
        *answer = Some(Some(state));
        return;
    }
    if !matches!(
        block,
        "create:speedometer"
            | "create:encased_fan"
            | "create:mechanical_drill"
            | "create:stressometer"
            | "create:water_wheel"
            | "create:large_water_wheel"
    ) {
        return;
    }
    let pos = [ctx.pos.x, ctx.pos.y, ctx.pos.z];
    let face = direction(ctx.face);
    let look = super::create_port::nearest(ctx.yaw, ctx.pitch);
    let ports = level_ports(game, pos);
    let mut set = |name: &str, value: &str| {
        if let Some(s) = game.native_state_with_property(state, name, value) {
            state = s;
        }
    };
    if matches!(block, "create:encased_fan" | "create:mechanical_drill") {
        let preferred = placement::preferred(ports).unwrap_or(look);
        set(
            "facing",
            if ctx.sneaking {
                preferred.name()
            } else {
                preferred.opposite().name()
            },
        );
    } else if block.ends_with("ometer") {
        let attached = ports[Direction::ALL
            .iter()
            .position(|d| *d == face.opposite())
            .unwrap()];
        let (facing, first) = placement::gauge(face, look, horizontal(ctx.yaw), ports, attached);
        set("facing", facing.name());
        set("axis_along_first", if first { "true" } else { "false" });
    } else {
        let facing = if !ctx.sneaking {
            placement::preferred(ports)
                .map(Direction::opposite)
                .unwrap_or(look.opposite())
        } else {
            look
        };
        let axis = if block == "create:water_wheel" {
            facing.axis()
        } else {
            placement::shaft(look, face, ctx.sneaking, ports)
        };
        if block == "create:large_water_wheel" {
            if !super::create_engine::waterwheel::structure(axis)
                .into_iter()
                .all(|delta| {
                    let p: [i32; 3] = std::array::from_fn(|i| pos[i] + delta[i]);
                    game.facts
                        .get(game.world.block(p[0], p[1], p[2]))
                        .replaceable
                })
            {
                if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
                    log::info!("Create placement refused: {:?} {:?}", pos, axis);
                }
                *answer = Some(None);
                return;
            }
            if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
                log::info!("Create placement accepted: {:?} {:?}", pos, axis);
            }
            set(
                "axis",
                match axis {
                    Axis::X => "x",
                    Axis::Y => "y",
                    Axis::Z => "z",
                },
            );
        } else {
            for d in Direction::ALL {
                let delta = d.vector();
                let p = std::array::from_fn(|i| pos[i] + delta[i]);
                if game
                    .facts
                    .get(game.world.block(p[0], p[1], p[2]))
                    .name
                    .as_ref()
                    == "create:water_wheel"
                    && (level_axis(game, p) != axis || d.axis() != axis)
                {
                    *answer = Some(None);
                    return;
                }
            }
            set("facing", facing.name());
        }
    }
    *answer = Some(Some(state));
}

pub(super) fn alias(game: &mut Game<'_>, stack: &ItemStack, pos: [i32; 3], face: [i32; 3]) -> bool {
    if !game.native_on("create")
        || !matches!(
            stack.item.as_ref(),
            "create:vertical_gearbox" | "create:large_water_wheel"
        )
    {
        return false;
    }
    if !game.may_build() && !game.can_place_on_in_adventure(pos) {
        return true;
    }
    let Some(d) = Direction::from_vector(face) else {
        return true;
    };
    let vertical = stack.item.as_ref() == "create:vertical_gearbox";
    let mut ctx = PlaceContext {
        clicked_pos: BlockPos::new(pos[0], pos[1], pos[2]),
        face: match d {
            Direction::Down => D::Down,
            Direction::Up => D::Up,
            Direction::North => D::North,
            Direction::South => D::South,
            Direction::West => D::West,
            Direction::East => D::East,
        },
        hit: Vec3::new(
            f64::from(pos[0]) + 0.5 + f64::from(face[0]) * 0.5,
            f64::from(pos[1]) + 0.5 + f64::from(face[1]) * 0.5,
            f64::from(pos[2]) + 0.5 + f64::from(face[2]) * 0.5,
        ),
        inside: false,
        yaw: game.yaw,
        pitch: game.pitch,
        sneaking: game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft),
        item: if vertical {
            "create:gearbox".into()
        } else {
            stack.item.to_string()
        },
        can_use_game_master_blocks: game.can_use_game_master_blocks(),
    };
    let mut outcome = game.with_level(|b, l, t| b.place_block(l, t, &ctx));
    if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
        log::info!("Create placement initial: {outcome:?}");
    }
    if !vertical && matches!(outcome, PlaceOutcome::Failed) {
        let p = if super::create_port_generators::replaceable(game, pos) {
            pos
        } else {
            std::array::from_fn(|i| pos[i] + face[i])
        };
        let axis = placement::shaft(
            super::create_port::nearest(game.yaw, game.pitch),
            d,
            ctx.sneaking,
            ports(game, p),
        );
        if axis != d.axis() {
            ctx.clicked_pos = BlockPos::new(p[0] + face[0], p[1] + face[1], p[2] + face[2]);
            ctx.hit = Vec3::new(
                ctx.hit.x + f64::from(face[0]),
                ctx.hit.y + f64::from(face[1]),
                ctx.hit.z + f64::from(face[2]),
            );
            outcome = game.with_level(|b, l, t| b.place_block(l, t, &ctx));
        }
    }
    if let PlaceOutcome::Placed { pos, state } = outcome {
        let target = [pos.x, pos.y, pos.z];
        if let Some(entity) = game.new_block_entity(if vertical {
            "create:gearbox"
        } else {
            "create:large_water_wheel"
        }) {
            game.world
                .block_entities
                .insert((pos.x, pos.y, pos.z), entity);
        }
        if vertical {
            let axis = placement::vertical_gearbox(horizontal(game.yaw), ports(game, target));
            game.native_set_block_property(target, "axis", if axis == Axis::X { "x" } else { "z" });
        }
        game.award_stat("used", 1);
        game.trigger_block("minecraft:placed_block", target, stack);
        game.native_place_block(stack, target, state);
        game.consume_held();
        super::create_port_kinetic_runtime::invalidate();
    } else {
        game.use_failed = true;
    }
    if let Err(error) = game.process_level_events() {
        log::warn!("Create placement: {error}");
    }
    true
}

pub(super) fn destroy(game: &mut Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    if !game.native_on("create") {
        return;
    }
    let block = game.native_block(pos);
    if !matches!(
        block.as_str(),
        "create:water_wheel_structure" | "create:large_water_wheel"
    ) {
        return;
    }
    let Some(master) = super::create_port_generators::master(game, pos) else {
        return;
    };
    let axis = super::create_port::node(game, master).map_or(Axis::X, |n| n.axis);
    let dim = game.dim.id().to_owned();
    let air = game
        .pack
        .assets
        .registry
        .default_state("minecraft:air")
        .unwrap();
    for delta in super::create_engine::waterwheel::structure(axis) {
        let p = std::array::from_fn(|i| master[i] + delta[i]);
        if game.native_block(p) == "create:water_wheel_structure" {
            game.native_set_block(&dim, p, air, 3);
        }
    }
    game.native_set_block(&dim, master, air, 3);
    if !game.creative {
        game.spawn_item(
            ItemStack::new("create:large_water_wheel", 1),
            master.map(|v| f64::from(v) + 0.5),
            [0.0; 3],
            10,
        );
    }
    super::create_port_kinetic_runtime::invalidate();
    *canceled = true;
}

fn level_axis(game: &GameLevel<'_>, p: [i32; 3]) -> Axis {
    let state = game
        .registry
        .state(game.world.block(p[0], p[1], p[2]))
        .unwrap();
    state_axis(state)
}

fn state_axis(state: &horizon_world::chunk::BlockState) -> Axis {
    let prop = |key: &str| state.properties.get(key).map(String::as_str).unwrap_or("");
    let facing = Direction::ALL
        .into_iter()
        .find(|d| d.name() == prop("facing"))
        .unwrap_or(Direction::Up);
    if state.name.ends_with("ometer") {
        return placement::gauge_axis(facing, prop("axis_along_first") == "true");
    }
    if state.name == "create:belt" {
        return if prop("slope") == "sideways" {
            Axis::Y
        } else if facing.axis() == Axis::X {
            Axis::Z
        } else {
            Axis::X
        };
    }
    match prop("axis") {
        "x" => Axis::X,
        "z" => Axis::Z,
        "y" => Axis::Y,
        _ => facing.axis(),
    }
}

// IRotate.hasShaftTowards for implemented blocks. Ownership does not imply a shaft.
fn state_shaft_port(state: &horizon_world::chunk::BlockState, face: Direction) -> bool {
    let facing = || {
        state.properties.get("facing").and_then(|value| {
            Direction::ALL
                .into_iter()
                .find(|direction| direction.name() == value)
        })
    };
    match state.name.as_str() {
        "create:creative_motor" => facing() == Some(face),
        "create:hand_crank" | "create:encased_fan" | "create:mechanical_drill" => {
            facing() == Some(face.opposite())
        }
        "create:millstone" => face == Direction::Down,
        "create:gearbox" => state_axis(state) != face.axis(),
        "create:belt" => {
            state
                .properties
                .get("part")
                .is_some_and(|part| part != "middle")
                && state_axis(state) == face.axis()
        }
        "create:shaft"
        | "create:andesite_encased_shaft"
        | "create:brass_encased_shaft"
        | "create:encased_chain_drive"
        | "create:cogwheel"
        | "create:large_cogwheel"
        | "create:clutch"
        | "create:gearshift"
        | "create:mechanical_press"
        | "create:speedometer"
        | "create:stressometer"
        | "create:water_wheel"
        | "create:large_water_wheel" => state_axis(state) == face.axis(),
        _ => false,
    }
}

pub(super) fn level_ports(game: &GameLevel<'_>, pos: [i32; 3]) -> [bool; 6] {
    Direction::ALL.map(|d| {
        let delta = d.vector();
        let p: [i32; 3] = std::array::from_fn(|i| pos[i] + delta[i]);
        let state = game
            .registry
            .state(game.world.block(p[0], p[1], p[2]))
            .unwrap();
        state_shaft_port(state, d.opposite())
    })
}

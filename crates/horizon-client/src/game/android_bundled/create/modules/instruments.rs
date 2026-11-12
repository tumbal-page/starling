// Copyright (c) The Create Team / The Creators of Create
// SPDX-License-Identifier: MIT
// GaugeRenderer's visible faces, dial pivot and comparator readout.
use super::{Game, create_engine};
use crate::game::native_block_entity_draw::BlockEntityDraw;
use horizon_client::containers::ModBlockEntity;
use super::create_engine::{kinetics::Axis, placement::Direction};
thread_local! {static DIRTY:std::cell::RefCell<std::collections::BTreeSet<(String,[i32;3])>>=const{std::cell::RefCell::new(std::collections::BTreeSet::new())};}
pub(super) fn changed(game: &Game<'_>, pos: [i32; 3]) {
    DIRTY.with(|d| {
        d.borrow_mut().insert((game.dim.id().to_owned(), pos));
    });
}
pub(super) fn flush(game: &mut Game<'_>) {
    let entries = DIRTY.with(|d| std::mem::take(&mut *d.borrow_mut()));
    for (dim, p) in entries {
        game.native_update_output_neighbors(&dim, p);
    }
}
pub(super) fn draw(
    game: &mut Game<'_>,
    pos: [i32; 3],
    _: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
    axis: Axis,
) {
    let facing = game
        .native_block_property(pos, "facing")
        .and_then(|n| Direction::ALL.into_iter().find(|d| d.name() == n))
        .unwrap_or(Direction::North);
    let value = crate::game::create_port_presentation::dial(game, pos, draw.partial);
    let model = if game.native_block(pos) == "create:speedometer" {
        "create:block/gauge/speedometer/head"
    } else {
        "create:block/gauge/stressometer/head"
    };
    for face in [
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ] {
        if face == facing.opposite() || face.axis() == axis || (axis == Axis::Y && face != facing) {
            continue;
        }
        let yaw = match face {
            Direction::South => -90.0,
            Direction::West => -180.0,
            Direction::North => -270.0,
            _ => -360.0,
        };
        draw.push();
        draw.rotate_about([0.0, 1.0, 0.0], yaw, [0.5; 3]);
        game.native_draw_model(draw, model, 0xFFFFFF);
        let p = 5.75 / 16.0;
        draw.translate([0.0, p, p]);
        draw.rotate_about([1.0, 0.0, 0.0], -90.0 * value, [0.0; 3]);
        draw.translate([0.0, -p, -p]);
        game.native_draw_model(draw, "create:block/gauge/dial", 0xFFFFFF);
        draw.pop();
    }
}

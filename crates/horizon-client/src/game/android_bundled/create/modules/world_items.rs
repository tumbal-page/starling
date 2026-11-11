// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// SharedDepotBlockMethods.onLanded and MechanicalPressBlockEntity world input.
use super::Game;
use crate::{containers::ModBlockEntity, items::ItemStack};
pub(super) fn catch(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    if entity.items.iter().any(Option::is_some) {
        return;
    }
    let dim = game.dim.id().to_owned();
    let min = [
        f64::from(pos[0]),
        f64::from(pos[1]) + 13.0 / 16.0 - 0.01,
        f64::from(pos[2]),
    ];
    let max = [min[0] + 1.0, f64::from(pos[1]) + 1.0, min[2] + 1.0];
    for dropped in game.native_item_entities(&dim, min, max) {
        if !game
            .dropped
            .iter()
            .any(|e| e.id == dropped.id && e.on_ground)
        {
            continue;
        }
        if dropped.position[1] < min[1]
            || dropped.position[0] < min[0]
            || dropped.position[0] > max[0]
            || dropped.position[2] < min[2]
            || dropped.position[2] > max[2]
        {
            continue;
        }
        let mut accepted = dropped.stack.clone();
        accepted.count = accepted
            .count
            .min(game.native_max_stack_size(&accepted.item).unwrap_or(64));
        let mut rest = dropped.stack;
        rest.count -= accepted.count;
        if game.native_set_item_entity(&dim, dropped.id, rest) {
            entity.items[0] = Some(accepted);
            crate::game::create_port_sounds::land(game, pos);
            break;
        }
    }
}
#[derive(Clone)]
pub(super) struct Input {
    pub id: u32,
    pub position: [f64; 3],
    pub stack: ItemStack,
}
pub(super) fn inputs(game: &Game<'_>, press: [i32; 3]) -> Vec<Input> {
    let min = [
        f64::from(press[0]),
        f64::from(press[1] - 1),
        f64::from(press[2]),
    ];
    game.native_item_entities(game.dim.id(), min, min.map(|v| v + 1.0))
        .into_iter()
        .filter(|item| game.dropped.iter().any(|e| e.id == item.id && e.on_ground))
        .map(|item| Input {
            id: item.id,
            position: item.position,
            stack: item.stack,
        })
        .collect()
}
pub(super) fn press(game: &mut Game<'_>, input: Input, output: ItemStack) {
    let dim = game.dim.id().to_owned();
    if !game
        .dropped
        .iter()
        .any(|e| e.id == input.id && e.stack == input.stack && e.on_ground)
    {
        return;
    }
    if input.stack.count == 1 {
        game.native_set_item_entity(&dim, input.id, output);
    } else {
        let mut rest = input.stack;
        rest.count -= 1;
        if game.native_set_item_entity(&dim, input.id, rest) {
            game.spawn_item(output, input.position, [0.0, 0.05, 0.0], 10);
        }
    }
}

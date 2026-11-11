// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create 6.0.10 BasinBlock/MillstoneBlock.updateEntityAfterFallOn. Called by
// the owned ItemEntity movement callback, after the ordinary collision response.
use super::Game;
use crate::{
    containers::{BlockEntity, ModBlockEntity},
    items::ItemStack,
};
use horizon_world::registry::BlockStateId;

const BASIN: &str = "create:basin";
const MILLSTONE: &str = "create:millstone";
type Pos = [i32; 3];

#[derive(Clone, Copy)]
enum Inventory {
    Basin,
    Millstone,
}
impl Inventory {
    fn id(self) -> &'static str {
        match self {
            Self::Basin => BASIN,
            Self::Millstone => MILLSTONE,
        }
    }
    fn size(self) -> usize {
        match self {
            Self::Basin => 18,
            Self::Millstone => 10,
        }
    }
    fn inputs(self) -> usize {
        match self {
            Self::Basin => 9,
            Self::Millstone => 1,
        }
    }
    fn keys(self) -> [&'static str; 2] {
        match self {
            Self::Basin => ["InputItems", "OutputItems"],
            Self::Millstone => ["InputInventory", "OutputInventory"],
        }
    }
}

/// Stored inventories must already have passed the shared early decoder. Only
/// a new native constructor with neither canonical half has implicit empty slots.
fn live_or_fresh(entity: &ModBlockEntity, inventory: Inventory) -> bool {
    entity.id == inventory.id()
        && crate::game::create_port::inventory_supported(entity)
        && (entity.items.len() == inventory.size()
            || entity.items.is_empty()
                && inventory
                    .keys()
                    .iter()
                    .all(|key| !entity.nbt.contains_key(&(*key).into())))
}

fn entity_at<'a>(game: &'a Game<'_>, pos: Pos, inventory: Inventory) -> Option<&'a ModBlockEntity> {
    match game.world.block_entities.get(&(pos[0], pos[1], pos[2]))? {
        BlockEntity::Mod(entity) if entity.id == inventory.id() => Some(entity),
        _ => None,
    }
}

/// A read-only admission pass avoids native_with_mod_block_entity's unconditional
/// dirty/revision work on refusal. No entity, inventory or opaque NBT is cloned.
fn admission(
    game: &Game<'_>,
    pos: Pos,
    inventory: Inventory,
    stack: &ItemStack,
) -> Option<(usize, u32)> {
    let entity = entity_at(game, pos, inventory)?;
    if !live_or_fresh(entity, inventory) {
        return None;
    }
    let maximum = crate::game::create_port_basin::max_stack(game, stack).min(64);
    let empty = std::array::from_fn::<_, 9, _>(|_| None);
    let slots = if entity.items.is_empty() {
        &empty[..inventory.inputs()]
    } else {
        &entity.items[..inventory.inputs()]
    };
    let slot = match inventory {
        Inventory::Basin => {
            (0..9).find(|&i| crate::game::create_port_basin::can_place(slots, i, stack, maximum))?
        }
        Inventory::Millstone => {
            if !crate::game::create_port_millstone::can_process(game, stack) {
                return None;
            }
            0
        }
    };
    let count = match slots[slot].as_ref().filter(|s| s.count > 0) {
        Some(held) if held.same_item_same_components(stack) => held.count,
        Some(_) => return None,
        None => 0,
    };
    let moved = maximum.saturating_sub(count).min(stack.count);
    (moved > 0).then_some((slot, moved))
}

fn insert(
    game: &mut Game<'_>,
    pos: Pos,
    inventory: Inventory,
    client: bool,
    stack: &mut ItemStack,
    discard: &mut bool,
) {
    let Some((slot, moved)) = admission(game, pos, inventory, stack) else {
        return;
    };
    // The Game is exclusively borrowed throughout this synchronous callback;
    // no world callback runs between admission and this inventory mutation.
    if game
        .native_with_mod_block_entity(pos, |entity| {
            if entity.items.is_empty() {
                entity.items.resize(inventory.size(), None);
            }
            match entity.items[slot].as_mut().filter(|s| s.count > 0) {
                Some(held) => held.count += moved,
                None => {
                    let mut part = stack.clone();
                    part.count = moved;
                    entity.items[slot] = Some(part);
                }
            }
            match inventory {
                Inventory::Basin => {
                    crate::game::create_port_basin::data(entity);
                }
                Inventory::Millstone => {
                    crate::game::create_port_millstone::data(entity);
                }
            }
        })
        .is_none()
    {
        return;
    }
    stack.count -= moved;
    *discard |= stack.count == 0;
    if matches!(inventory, Inventory::Basin) {
        crate::game::create_port_basin::notify_contents(game, pos);
    }
    if !client {
        game.native_block_entity_sync(pos);
    }
}

pub(super) fn after_vertical_collision(
    game: &mut Game<'_>,
    dimension: &str,
    client: bool,
    state: BlockStateId,
    item_pos: [f64; 3],
    stack: &mut ItemStack,
    discard: &mut bool,
) {
    if *discard
        || dimension != game.dim.id()
        || !game.native_on("create")
        || stack.count == 0
        || stack.item.as_ref() == "minecraft:air"
    {
        return;
    }
    let pos = item_pos.map(|v| v.floor() as i32);
    match game.res.facts().get(state).name.as_ref() {
        BASIN => {
            // BasinBlock checks the entity's containing block as well as the
            // support callback: touching the rim from the block above is not intake.
            if game.native_block(pos) == BASIN {
                insert(game, pos, Inventory::Basin, client, stack, discard);
            }
        }
        MILLSTONE if !client => {
            // Iterate.hereAndBelow chooses the first Millstone BE, even when
            // that inventory subsequently refuses the item.
            let below = [pos[0], pos[1].wrapping_sub(1), pos[2]];
            let target = [pos, below]
                .into_iter()
                .find(|p| entity_at(game, *p, Inventory::Millstone).is_some());
            if let Some(pos) = target {
                insert(game, pos, Inventory::Millstone, false, stack, discard);
            }
        }
        _ => {}
    }
}

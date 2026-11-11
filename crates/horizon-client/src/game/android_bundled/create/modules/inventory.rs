// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Game-stack adapter for DepotBehaviour and BeltPressingCallbacks.
// Adapted for Android bundled port.

use crate::{containers::ModBlockEntity, items::ItemStack};

pub(super) fn insert(
    slots: &mut [Option<ItemStack>],
    mut incoming: ItemStack,
    max: u32,
) -> Option<ItemStack> {
    for existing in slots.iter_mut().flatten() {
        let (mut a, mut b) = (existing.clone(), incoming.clone());
        a.count = 1;
        b.count = 1;
        if a == b {
            let moved = incoming.count.min(max.saturating_sub(existing.count));
            existing.count += moved;
            incoming.count -= moved;
            if incoming.count == 0 {
                return None;
            }
        }
    }
    for slot in slots.iter_mut().filter(|s| s.is_none()) {
        let mut part = incoming.clone();
        part.count = part.count.min(max);
        incoming.count -= part.count;
        *slot = Some(part);
        if incoming.count == 0 {
            return None;
        }
    }
    Some(incoming)
}

pub(super) fn press(
    entity: &mut ModBlockEntity,
    input: ItemStack,
    mut output: ItemStack,
    max: u32,
) -> Option<Vec<ItemStack>> {
    if entity.id != "create:depot" || entity.items.first() != Some(&Some(input.clone())) {
        return None;
    }
    // Create CRecipes.bulkPressing defaults to false.
    let outcome = android_bundled::create::crates::engine::depot::press(input.count, false)?;
    output.count = output.count.checked_mul(outcome.processed)?;
    // BeltPressingCallbacks.whenItemHeld clears fan processing before copying
    // either the held result or the unpressed remainder into transported stacks.
    if let Some(horizon_world::nbt::Tag::Compound(held)) = entity.nbt.get_mut(&"HeldItem".into()) {
        held.remove(&"FanProcessingType".into());
        held.remove(&"FanProcessingTime".into());
    }
    let overflow = if outcome.output_held {
        entity.items[0] = Some(output);
        None
    } else {
        let mut remaining = input;
        remaining.count = outcome.input_left;
        entity.items[0] = Some(remaining);
        insert(&mut entity.items[1..], output, max)
    };
    crate::game::create_port::depot_data(entity);
    Some(overflow.into_iter().collect())
}
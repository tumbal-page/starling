// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// BeltPressingCallbacks adapter, pinned Create6.0.10. Ordinary pressing recipes
// and default bulkPressing=false; no sequenced-assembly or basin compression.
// Adapted for Android bundled port.

use super::{Game, create_port as port, create_port_belt as belt};
use crate::game::mod_api::ModBlockEntityExt;
use horizon_client::{containers::BlockEntity, items::ItemStack};
use super::create_engine::{
    pressing::{self, BeltDecision, BeltPressContext, BeltPressState, PressCycle},
    transport::{ProcessingResult, ProcessingUpdate, Stack, Transported},
};
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_world::nbt::{Compound, Tag};
use std::cell::RefCell;

type Pos = [i32; 3];
thread_local! {
    // Create.RANDOM is independent of TransportedItemStack.R and level.random.
    static CREATE_RANDOM: RefCell<LegacyRandom> = RefCell::new(LegacyRandom::new(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos() as i64)
    ));
}
fn decision(value: BeltDecision) -> ProcessingResult {
    match value {
        BeltDecision::Pass => ProcessingResult::Pass,
        BeltDecision::Hold => ProcessingResult::Hold,
    }
}
pub(super) fn available(game: &Game<'_>, pos: Pos) -> bool {
    game.native_block(pos) == "create:mechanical_press"
        && matches!(game.world.block_entities.get(&(pos[0],pos[1],pos[2])),
            Some(BlockEntity::Mod(e)) if e.id == "create:mechanical_press")
}
fn state(game: &Game<'_>, pos: Pos) -> Option<BeltPressState> {
    if !available(game, pos) {
        return None;
    }
    let BlockEntity::Mod(entity) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))? else {
        return None;
    };
    Some(BeltPressState {
        running: port::integer(&entity.nbt, "Running") != 0,
        ticks: port::integer(&entity.nbt, "Ticks"),
        rpm: port::running_speed(game, &entity.nbt),
        bulk: false,
    })
}
pub(super) fn received(
    game: &mut Game<'_>,
    pos: Pos,
    item: &mut Transported<ItemStack>,
) -> ProcessingUpdate<ItemStack> {
    let Some(state) = state(game, pos) else {
        return ProcessingResult::Pass.into();
    };
    let mut cycle = PressCycle {
        running: state.running,
        ticks: state.ticks,
        applied: false,
    };
    let entry = pressing::belt_received(&mut cycle, state.rpm, || {
        port::result(game, &belt::game_stack(&item.stack)).is_some()
    });
    if entry.started {
        game.native_with_mod_block_entity(pos, |entity| {
            entity.nbt.insert("Running".into(), Tag::Byte(1));
            entity.nbt.insert("Ticks".into(), Tag::Int(0));
            entity.nbt.insert("Mode".into(), Tag::Int(1));
            entity.nbt.insert("Applied".into(), Tag::Int(0));
            entity.clear_client_update_fields();
            entity.nbt.remove(&"ParticleItems".into());
        });
        game.native_block_entity_sync(pos);
    }
    decision(entry.decision).into()
}
struct Context<'a, 'b> {
    game: &'a mut Game<'b>,
    pos: Pos,
}
impl BeltPressContext<ItemStack> for Context<'_, '_> {
    fn clear_particles(&mut self) {
        self.game.native_with_mod_block_entity(self.pos, |entity| {
            entity.clear_client_update_fields();
            entity.nbt.remove(&"ParticleItems".into());
        });
    }
    fn process(&mut self, input: &Transported<ItemStack>) -> Option<Vec<Stack<ItemStack>>> {
        let original = belt::game_stack(&input.stack);
        let output = port::result(self.game, &original)?;
        self.game.native_with_mod_block_entity(self.pos, |entity| {
            entity.set_client_update_fields();
        });
        Some(vec![belt::core_stack(self.game, output)])
    }
    fn upright(&mut self, stack: &Stack<ItemStack>) -> bool {
        belt::upright(self.game, &stack.payload)
    }
    fn create_angle(&mut self) -> i32 {
        CREATE_RANDOM.with(|random| random.borrow_mut().next_int_bound(360).unwrap_or(0))
    }
    fn on_source_copy(&mut self, input: &Stack<ItemStack>) {
        let _ = belt::angle(self.game, &input.payload);
    }
}
pub(super) fn held(
    game: &mut Game<'_>,
    pos: Pos,
    item: &mut Transported<ItemStack>,
) -> ProcessingUpdate<ItemStack> {
    let Some(state) = state(game, pos) else {
        return ProcessingResult::Pass.into();
    };
    let output = pressing::belt_held(item, state, &mut Context { game, pos });
    if output.send_data {
        game.native_block_entity_sync(pos);
    }
    ProcessingUpdate {
        decision: decision(output.decision),
        conversion: Some(output.conversion),
    }
}

pub(super) fn tick(_game: &mut Game<'_>, _pos: [i32; 3], _entity: &mut horizon_client::containers::ModBlockEntity) {}
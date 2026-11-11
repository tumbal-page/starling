// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// MechanicalPressBlockEntity's BasinOperating/PressingBehaviour server boundary,
// Create ac0c444d9828da3453ae8cc65338e8de063286fb (6.0.10).
use super::{
    Game, create_port_basin as basin,
    create_port_basin_recipes::{self as recipes, Operator, Recipe},
};
use crate::containers::{ModBlockEntity, ModRevision};
use create_engine::{
    pressing::{Action, PressCycle},
    transport::SessionCache,
};
use horizon_world::nbt::{Compound, Tag};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

const ID: &str = "create:mechanical_press";
const BASIN_MODE: i32 = 2;
type Pos = [i32; 3];

#[derive(Default)]
struct Running {
    owner: Option<ModRevision>,
    recipe: Option<Arc<Recipe>>,
    tick: Option<i64>,
    removed: bool,
    handled: bool,
}
thread_local! {
    static WORLDS: RefCell<SessionCache<BTreeMap<Pos, Running>>> = RefCell::new(SessionCache::default());
}
fn with_world<R>(game: &Game<'_>, action: impl FnOnce(&mut BTreeMap<Pos, Running>) -> R) -> R {
    WORLDS.with(|cache| {
        action(
            cache
                .borrow_mut()
                .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id()),
        )
    })
}
pub(super) fn reset(reload: bool) {
    if !reload {
        WORLDS.with(|cache| cache.borrow_mut().clear());
    }
}
pub(super) fn maintenance(game: &Game<'_>) {
    with_world(game, |world| {
        world.retain(|pos, _| game.world.loaded(pos[0], pos[2]) && game.native_block(*pos) == ID)
    });
}
pub(super) fn schedule(game: &mut Game<'_>, pos: Pos) {
    if game.native_block(pos) == ID {
        game.native_with_mod_block_entity(pos, |entity| {
            if entity.id == ID {
                entity.nbt.insert("NeedsUpdate".into(), Tag::Byte(1));
            }
        });
    }
}
pub(super) fn on_speed_changed(entity: &mut ModBlockEntity, previous: f32, current: f32) {
    if entity.id == ID && previous != current {
        entity.nbt.insert("NeedsUpdate".into(), Tag::Byte(1));
    }
}
pub(super) fn basin_removed(game: &Game<'_>, pos: Pos) {
    let Some(crate::containers::BlockEntity::Mod(entity)) =
        game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    if entity.id != ID || game.native_block(pos) != ID {
        return;
    }
    let owner = entity.revision();
    with_world(game, |world| {
        let state = world.entry(pos).or_default();
        if state.owner.is_some_and(|old| !old.same_entity(owner)) {
            *state = Running::default();
        }
        state.owner = Some(owner);
        state.removed = true;
    });
}
fn start(entity: &mut ModBlockEntity, cycle: &mut PressCycle) {
    *cycle = PressCycle {
        running: true,
        ticks: 0,
        applied: false,
    };
    entity.nbt.insert("Mode".into(), Tag::Int(BASIN_MODE));
    // PressingBehaviour.start does not reset Finished.
    entity.clear_client_update_fields();
    entity.nbt.remove(&"ParticleItems".into());
}

/// `None` leaves WORLD/BELT processing to its existing adapter. `Some` consumes
/// this tick and says whether the ordinary BE update should be synchronized.
/// Called after resolving actual kinetic speed, while this BE is detached.
pub(super) fn tick(
    game: &mut Game<'_>,
    pos: Pos,
    entity: &mut ModBlockEntity,
    rpm: f32,
) -> Option<bool> {
    if entity.id != ID {
        return None;
    }
    let integer = super::create_port::integer;
    for key in ["Running", "Finished", "NeedsUpdate"] {
        entity.nbt.entry(key.into()).or_insert(Tag::Byte(0));
    }
    for key in ["Ticks", "Mode"] {
        entity.nbt.entry(key.into()).or_insert(Tag::Int(0));
    }
    let now = game.world.game_time;
    let mut state = with_world(game, |world| world.remove(&pos).unwrap_or_default());
    let owner = entity.revision();
    if state.tick.is_some_and(|tick| tick > now)
        || state.owner.is_some_and(|old| !old.same_entity(owner))
    {
        state = Running::default();
    }
    state.owner = Some(owner);
    if state.tick == Some(now) {
        let handled = state.handled;
        with_world(game, |world| {
            world.insert(pos, state);
        });
        return handled.then_some(false);
    }
    let mut cycle = PressCycle {
        running: integer(&entity.nbt, "Running") != 0,
        ticks: integer(&entity.nbt, "Ticks"),
        applied: integer(&entity.nbt, "Applied") != 0,
    };
    let basin_pos = [pos[0], pos[1] - 2, pos[2]];
    let mut pending = integer(&entity.nbt, "NeedsUpdate") != 0;
    let mut sync = false;
    let mut handled = false;
    // BasinOperating.tick aborts before its ordinary behavior tick when an
    // actual removal callback raised the flag. A missing cold-loaded Basin
    // alone is not that callback and must not invent an abort.
    if state.removed {
        state.removed = false;
        cycle.running = false;
        cycle.ticks = 0;
        entity.clear_client_update_fields();
        entity.nbt.remove(&"ParticleItems".into());
        sync = true;
        handled = true;
    } else {
        if pending {
            pending = false;
            // Press has IRotate's NONE requirement: every nonzero speed can
            // start compacting, unlike the Mixer's MEDIUM threshold.
            if !cycle.running
                && rpm != 0.0
                && !rpm.is_nan()
                && let Some(mut basin) = basin::read(game, basin_pos)
                && let Some(recipe) =
                    recipes::choose(game, basin_pos, &mut basin, Operator::Compacting)
            {
                state.recipe = Some(recipe);
                start(entity, &mut cycle);
                sync = true;
            }
        }
        if cycle.running && integer(&entity.nbt, "Mode") == BASIN_MODE {
            handled = true;
            // A Deferral start above runs before this behavior's tick, so it
            // advances immediately. A completion restart below does not.
            match cycle.tick(rpm, false) {
                Action::Press => {
                    let _ = recipes::apply(game, basin_pos, state.recipe.as_deref());
                    let particles = basin::read(game, basin_pos).map_or_else(Vec::new, |basin| {
                        basin
                            .input
                            .iter()
                            .flatten()
                            .filter(|stack| stack.count > 0)
                            .map(|stack| {
                                horizon_items::item_nbt::item_to_nbt(&basin::native_stack(stack))
                            })
                            .collect()
                    });
                    // tryProcessInBasin always returns true and records inputs
                    // remaining AFTER apply, even when no currentRecipe exists.
                    entity.set_client_update_fields(Compound::from([(
                        "ParticleItems".into(),
                        Tag::List(particles),
                    )]));
                    super::create_port_sounds::press(game, pos, rpm);
                    sync = true;
                }
                Action::Finished => {
                    entity.nbt.insert("Finished".into(), Tag::Byte(1));
                    entity.clear_client_update_fields();
                    entity.nbt.remove(&"ParticleItems".into());
                    let repeat = state.recipe.as_deref().is_some_and(|recipe| {
                        basin::read(game, basin_pos).is_some_and(|mut basin| {
                            recipes::matching(game, basin_pos, &mut basin, recipe)
                                && basin.can_continue_processing()
                        })
                    });
                    if repeat {
                        start(entity, &mut cycle);
                    } else {
                        pending = true;
                    }
                    sync = true;
                }
                Action::None => {}
                Action::Started => {
                    unreachable!("an existing Basin cycle does not scan WORLD input")
                }
            }
        } else if !cycle.running && game.native_block(basin_pos) == "create:basin" {
            // PressingBehaviour's idle WORLD scan checks the Basin block, not
            // just whether its current block entity/recipe happens to resolve.
            handled = true;
        }
    }
    entity
        .nbt
        .insert("NeedsUpdate".into(), Tag::Byte(i8::from(pending)));
    if handled {
        entity
            .nbt
            .insert("Running".into(), Tag::Byte(i8::from(cycle.running)));
        entity.nbt.insert("Ticks".into(), Tag::Int(cycle.ticks));
        entity
            .nbt
            .insert("Applied".into(), Tag::Int(i32::from(cycle.applied)));
        entity.nbt.insert("LastTick".into(), Tag::Long(now));
    }
    state.tick = Some(now);
    state.handled = handled;
    with_world(game, |world| {
        world.insert(pos, state);
    });
    handled.then_some(sync)
}

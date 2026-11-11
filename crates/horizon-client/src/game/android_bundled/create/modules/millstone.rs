// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// MillstoneBlock/BlockEntity: processing, filtered capability, falling items,
// empty-hand collection and original NBT. No vanilla container screen is used.
// Adapted for Android bundled port.

use super::{Game, native_block_entity_draw::BlockEntityDraw};
use crate::{
    containers::{ModBlockEntity, ModContainer},
    items::ItemStack,
};
use android_bundled::create::crates::engine::{
    kinetics::Axis,
    milling::{self, Action, MillCycle},
};
use horizon_core::random::RandomSource;
use horizon_world::nbt::{Compound, Tag};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const ID: &str = "create:millstone";

thread_local! {
    // Containers::can_place has the item registry but no recipe table at API
    // 359. Its query uses the latest server recipe ingredients compiled here.
    static INPUTS: RefCell<Inputs> = const {RefCell::new(Inputs {
        stamp: None, sources: Vec::new(), accepted: BTreeSet::new(),
        recipes: Vec::new(), last: BTreeMap::new(),
    })};
}

struct Inputs {
    stamp: Option<(usize, i64)>,
    sources: Vec<Arc<serde_json::Value>>,
    accepted: BTreeSet<String>,
    recipes: Vec<CompiledRecipe>,
    last: BTreeMap<(String, [i32; 3]), usize>,
}

pub(super) fn invalidate_recipes() {
    INPUTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.stamp = None;
        cache.sources.clear();
        cache.accepted.clear();
        cache.recipes.clear();
        cache.last.clear();
    });
}

pub(super) fn refresh_recipes(game: &Game<'_>) {
    INPUTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let stamp = (std::ptr::from_ref(game) as usize, game.world.game_time);
        if cache.stamp == Some(stamp) {
            return;
        }
        if cache
            .stamp
            .is_some_and(|previous| previous.0 != stamp.0 || previous.1 > stamp.1)
        {
            cache.sources.clear();
            cache.last.clear();
            cache.recipes.clear();
            cache.accepted.clear();
        }
        cache.stamp = Some(stamp);
        let recipes = game.native_recipes_of_type("create:milling");
        if recipes.len() == cache.sources.len()
            && recipes
                .iter()
                .zip(&cache.sources)
                .all(|(r, old)| Arc::ptr_eq(&r.json, old))
        {
            return;
        }
        cache.recipes = recipes
            .iter()
            .filter_map(|r| compile_recipe(game, &r.json))
            .collect();
        cache.accepted = cache
            .recipes
            .iter()
            .flat_map(|r| r.input.order.iter().cloned())
            .collect();
        cache.last.clear();
        cache.sources = recipes.iter().map(|r| r.json.clone()).collect();
    });
}

pub(super) fn automation_accepts(stack: &ItemStack) -> bool {
    stack.count > 0 && INPUTS.with(|cache| cache.borrow().accepted.contains(stack.item.as_ref()))
}

/// MillstoneInventoryHandler.canProcess: evaluate the current compiled
/// ingredient predicate, without changing the processing cycle's last recipe.
pub(super) fn can_process(game: &Game<'_>, stack: &ItemStack) -> bool {
    refresh_recipes(game);
    stack.count > 0
        && INPUTS.with(|cache| {
            cache
                .borrow()
                .recipes
                .iter()
                .any(|recipe| recipe.input.test(stack))
        })
}

#[derive(Clone)]
struct Recipe {
    duration: i32,
    outputs: Vec<(ItemStack, f32)>,
}

struct CompiledRecipe {
    input: horizon_client::crafting::Ingredient,
    recipe: Recipe,
}

fn compile_recipe(game: &Game<'_>, json: &serde_json::Value) -> Option<CompiledRecipe> {
    let input = json["ingredients"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(crate::game::create_port::ingredient)
        .and_then(|i| game.native_recipe_ingredient(&i))?;
    let outputs: Option<Vec<_>> = json["results"]
        .as_array()?
        .iter()
        .map(|output| {
            let mut output = output.clone();
            // Read both ProcessingOutput codecs supported by Create 6.
            if output.get("id").is_none() {
                output["id"] = output["item"].clone();
            }
            Some((
                game.native_recipe_result(&output)?,
                output["chance"].as_f64().unwrap_or(1.0) as f32,
            ))
        })
        .collect();
    Some(CompiledRecipe {
        input,
        recipe: Recipe {
            duration: json["processing_time"]
                .as_i64()
                .or_else(|| json["processingTime"].as_i64())
                .unwrap_or(0) as i32,
            outputs: outputs?,
        },
    })
}

pub(super) fn forget(pos: [i32; 3]) {
    INPUTS.with(|cache| cache.borrow_mut().last.retain(|(_, p), _| *p != pos));
}

fn recipe(game: &Game<'_>, pos: [i32; 3], stack: &ItemStack) -> Option<Recipe> {
    INPUTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (game.dim.id().to_owned(), pos);
        let index = cache
            .last
            .get(&key)
            .copied()
            .filter(|i| cache.recipes.get(*i).is_some_and(|r| r.input.test(stack)))
            .or_else(|| cache.recipes.iter().position(|r| r.input.test(stack)))?;
        cache.last.insert(key, index);
        Some(cache.recipes[index].recipe.clone())
    })
}

pub(super) fn container(block: &str, container: &mut Option<ModContainer>) {
    if block == ID && container.is_none() {
        *container = Some(ModContainer {
            slots: milling::INPUT_SLOTS + milling::OUTPUT_SLOTS,
            faces: std::array::from_fn(|_| (0..10).collect()),
            inputs: vec![0],
            outputs: (1..10).collect(),
            menu: None,
        });
    }
}

fn inventory(slots: &[Option<ItemStack>]) -> Tag {
    let items = slots
        .iter()
        .enumerate()
        .filter_map(|(slot, item)| {
            let Tag::Compound(mut tag) = horizon_items::item_nbt::item_to_nbt(item.as_ref()?)
            else {
                return None;
            };
            tag.insert("Slot".into(), Tag::Int(slot as i32));
            Some(Tag::Compound(tag))
        })
        .collect();
    Tag::Compound(Compound::from([
        ("Size".into(), Tag::Int(slots.len() as i32)),
        ("Items".into(), Tag::List(items)),
    ]))
}

fn restore(entity: &mut ModBlockEntity) {
    // Canonical inventory is decoded before publication. Never re-read an old
    // bridge snapshot after ordinary automation has already changed live slots.
    if crate::game::create_port::inventory_supported(entity) {
        entity.items.resize(10, None);
    }
}

pub(super) fn data(entity: &mut ModBlockEntity) -> bool {
    if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
        return false;
    }
    entity.items.resize(10, None);
    let mut changed = false;
    for (key, range) in [("InputInventory", 0..1), ("OutputInventory", 1..10)] {
        let Tag::Compound(updated) = inventory(&entity.items[range]) else {
            unreachable!();
        };
        let mut value = match entity.nbt.get(&key.into()) {
            Some(Tag::Compound(existing)) => existing.clone(),
            _ => Compound::new(),
        };
        value.extend(updated);
        let value = Tag::Compound(value);
        let key_nbt = key.into();
        changed |= entity.nbt.get(&key_nbt) != Some(&value);
        entity.nbt.insert(key_nbt, value.clone());
    }
    changed
}

fn max_stack(game: &Game<'_>, stack: &ItemStack) -> u32 {
    super::create_port::stack_maximum(game, stack)
}

/// Called after the kinetic network has resolved the current signed RPM.
pub(super) fn tick(
    game: &mut Game<'_>,
    pos: [i32; 3],
    entity: &mut ModBlockEntity,
    rpm: f32,
) -> bool {
    if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
        return false;
    }
    refresh_recipes(game);
    restore(entity);
    let input = entity.items[0].clone().filter(|s| s.count > 0);
    let mut cycle = MillCycle {
        timer: crate::game::create_port::integer(&entity.nbt, "Timer"),
    };
    let blocked = rpm == 0.0
        || entity.items[1..]
            .iter()
            .flatten()
            .any(|s| s.count == milling::SLOT_LIMIT);
    let matching = if cycle.timer <= 0 && !blocked {
        input.as_ref().and_then(|input| recipe(game, pos, input))
    } else {
        None
    };
    let action = cycle.tick(
        rpm,
        entity.items[1..]
            .iter()
            .map(|s| s.as_ref().map_or(0, |s| s.count)),
        input.is_some(),
        matching.as_ref().map(|r| r.duration),
    );
    entity.nbt.insert("Timer".into(), Tag::Int(cycle.timer));
    if action == Action::Process
        && let Some(input) = input
        && let Some(recipe) = matching.or_else(|| recipe(game, pos, &input))
    {
        let remainder = game
            .items()
            .get(&input.item)
            .and_then(|item| item.crafting_remainder.clone());
        entity.items[0] = if input.count > 1 {
            let mut stack = input;
            stack.count -= 1;
            Some(stack)
        } else {
            None
        };
        for (mut output, chance) in recipe.outputs {
            output.count =
                milling::roll_count(output.count, chance, || game.world.random.next_float());
            if output.count == 0 {
                continue;
            }
            let max = max_stack(game, &output).min(milling::SLOT_LIMIT);
            // Original insertItemStacked ignores remainder when the buffer
            // cannot fit a complete roll; preserve that behavior.
            let _ = crate::game::create_port_inventory::insert(&mut entity.items[1..], output, max);
        }
        if let Some(remainder) = remainder {
            let stack = ItemStack::new(&remainder, 1);
            let max = max_stack(game, &stack).min(milling::SLOT_LIMIT);
            let _ = crate::game::create_port_inventory::insert(&mut entity.items[1..], stack, max);
        }
    }
    let inventory_changed = data(entity);
    inventory_changed || matches!(action, Action::Armed | Action::Process)
}

pub(super) fn reacts(game: &Game<'_>, pos: [i32; 3]) -> bool {
    game.native_on("create")
        && game.native_block(pos) == ID
        && game
            .held()
            .is_none_or(|s| s.count == 0 || s.item.as_ref() == "minecraft:air")
}

pub(super) fn right_click(game: &mut Game<'_>, pos: [i32; 3]) -> bool {
    if crate::game::create_port::inventory_blocked(game, pos) {
        return true;
    }
    if !reacts(game, pos) {
        return false;
    }
    let Some(stacks) = game.native_with_mod_block_entity(pos, |entity| {
        restore(entity);
        let take_input = milling::collect_input(
            entity.items[1..]
                .iter()
                .map(|s| s.as_ref().map_or(0, |s| s.count)),
        );
        let stacks = if take_input {
            entity.items[..1]
                .iter_mut()
                .filter_map(Option::take)
                .collect::<Vec<_>>()
        } else {
            entity.items[1..]
                .iter_mut()
                .filter_map(Option::take)
                .collect::<Vec<_>>()
        };
        data(entity);
        stacks
    }) else {
        return false;
    };
    let uuid = game.native_acting_id();
    for stack in stacks {
        if let Some(rest) = game.native_give_stack(uuid, stack)
            && let Some(player) = game.native_player()
        {
            game.spawn_item(
                rest,
                [player.x, player.y + 1.0, player.z],
                [0.0, 0.2, 0.0],
                10,
            );
        }
    }
    game.native_block_entity_sync(pos);
    true
}

pub(super) fn draw(
    game: &Game<'_>,
    pos: [i32; 3],
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) -> bool {
    if entity.id != ID {
        return false;
    }
    let angle = create_engine::visual::angle(
        draw.game_time as f64 + f64::from(draw.partial),
        crate::game::create_port::number(&entity.nbt, "Speed"),
        create_engine::visual::rotation_offset(pos, Axis::Y, false),
    );
    draw.push();
    draw.rotate_about([0.0, 1.0, 0.0], angle, [0.5; 3]);
    game.native_draw_model(
        draw,
        "create:block/millstone/inner",
        crate::game::create_port_presentation::kinetic_tint(game, pos),
    );
    draw.pop();
    true
}

/// Client-only counterpart of spawnParticles. Run once per client game tick,
/// rather than per rendered frame, so frame rate never changes particle rate.
pub(super) fn client_tick(game: &mut Game<'_>) {
    if !game.native_window() || !game.native_on("create") {
        return;
    }
    let entries: Vec<_> = game
        .world
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), e)| {
            let crate::containers::BlockEntity::Mod(e) = e else {
                return None;
            };
            if e.id != ID {
                return None;
            }
            let rpm = crate::game::create_port::number(&e.nbt, "Speed");
            let timer = crate::game::create_port::integer(&e.nbt, "Timer");
            let outputs_full = e
                .nbt
                .get(&"OutputInventory".into())
                .and_then(|v| v.get("Items"))
                .is_some_and(|v| match v {
                    Tag::List(items) => items
                        .iter()
                        .filter_map(horizon_items::item_nbt::item_from_nbt)
                        .any(|s| s.count == milling::SLOT_LIMIT),
                    _ => false,
                });
            if rpm == 0.0 || timer <= 0 || outputs_full {
                return None;
            }
            let input = e
                .nbt
                .get(&"InputInventory".into())
                .and_then(|v| v.get("Items"))
                .and_then(|v| match v {
                    Tag::List(items) => items.first(),
                    _ => None,
                })
                .and_then(horizon_items::item_nbt::item_from_nbt)?;
            Some(([x, y, z], rpm, input))
        })
        .collect();
    for (pos, rpm, input) in entries {
        let angle = f64::from(game.world.random.next_float() * 360.0).to_radians();
        let next = angle + (if rpm > 0.0 { 25.0f64 } else { -25.0f64 }).to_radians();
        let offset = [angle.sin() * 0.5, 0.0, angle.cos() * 0.5];
        let target = [next.sin() * 0.5, 0.0, next.cos() * 0.5];
        let velocity = std::array::from_fn(|i| {
            target[i] - offset[i] + (game.world.random.next_double() - 0.5) / 64.0
        });
        let center = std::array::from_fn(|i| f64::from(pos[i]) + 0.5 + offset[i]);
        let options = game.item_particle(&input);
        game.particles.add(&options, center, velocity);
    }
}
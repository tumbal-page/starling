// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// AirCurrent / FanProcessing adapter. No item components are used as entity data.
// Adapted for Android bundled port.

use super::{Game, create_engine};
use crate::game::native_block_entity_draw::BlockEntityDraw;
use horizon_client::{containers::ModBlockEntity, items::ItemStack};
use super::create_engine::{
    fan::{self, Processing, ProcessingType as Type, Segment},
    placement::Direction,
};
use horizon_core::{geometry::{Aabb, Vec3}, random::RandomSource};
use horizon_world::nbt::{Compound, Tag};
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct Current {
    facing: Option<Direction>,
    rpm: f32,
    cooldown: i32,
    search: i32,
    distance: f32,
    segments: Vec<Segment>,
    caught: Vec<u32>,
    handlers: Vec<([i32; 3], Type)>,
}
#[derive(Default)]
struct Cache {
    cleanup: BTreeMap<String, i64>,
    currents: BTreeMap<(String, [i32; 3]), Current>,
    client_origins: std::collections::BTreeSet<(String, [i32; 3])>,
    client_game: Option<usize>,
    client_tick: Option<(String, i64)>,
    recipes: BTreeMap<Type, BTreeMap<String, Vec<(ItemStack, f32)>>>,
    recipes_ready: bool,
}
thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache::default());}
pub(super) fn reset(reload: bool) {
    super::create_port_fan_entities::reset(reload);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !reload {
            *cache = Cache::default();
        } else {
            cache.recipes_ready = false;
        }
    });
}
pub(super) fn changed(dimension: &str, pos: [i32; 3]) {
    super::create_port_fan_entities::clear(dimension, pos);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache
            .currents
            .retain(|(dim, origin), _| dim != dimension || *origin != pos);
        cache
            .client_origins
            .retain(|(dim, origin)| dim != dimension || *origin != pos);
        for ((dim, origin), current) in &mut cache.currents {
            if dim != dimension {
                continue;
            }
            if origin
                .iter()
                .zip(pos)
                .map(|(a, b)| (a - b).abs())
                .sum::<i32>()
                <= 1
            {
                current.cooldown = 0;
            }
        }
    });
}
pub(super) fn maintenance(game: &Game<'_>) {
    super::create_port_fan_entities::maintenance(game);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.cleanup.get(game.dim.id()) == Some(&game.world.game_time) {
            return;
        }
        cache
            .cleanup
            .insert(game.dim.id().into(), game.world.game_time);
        if game.world.game_time.rem_euclid(20) != 0 {
            return;
        }
        cache.currents.retain(|(dim, pos), _| {
            dim != game.dim.id() || game.native_block(*pos) == "create:encased_fan"
        });
    });
}
fn facing(game: &Game<'_>, pos: [i32; 3]) -> Direction {
    let value = game.native_block_property(pos, "facing");
    Direction::ALL
        .into_iter()
        .find(|d| Some(d.name()) == value.as_deref())
        .unwrap_or(Direction::Down)
}
fn add(pos: [i32; 3], d: Direction, n: i32) -> [i32; 3] {
    let v = d.vector();
    std::array::from_fn(|i| pos[i] + v[i] * n)
}
fn block_tag(game: &Game<'_>, pos: [i32; 3], tag: &str) -> bool {
    game.pack
        .assets
        .registry
        .state(game.world.block(pos[0], pos[1], pos[2]))
        .is_some_and(|s| game.native_server_block_in_tag(s, tag).unwrap_or(false))
}
fn media(game: &Game<'_>, pos: [i32; 3]) -> Option<Type> {
    let name = game.native_block(pos);
    let water = name == "minecraft:water"
        || name == "minecraft:bubble_column"
        || game.native_block_property(pos, "waterlogged").as_deref() == Some("true");
    fan::Medium {
        // The bounded native fluid registry currently exposes vanilla water
        // and lava; custom fluid-tag registration is a separate capability.
        fluid: [name == "minecraft:lava", false, false, water],
        block: [
            "create:fan_processing_catalysts/blasting",
            "create:fan_processing_catalysts/haunting",
            "create:fan_processing_catalysts/smoking",
            "create:fan_processing_catalysts/splashing",
        ]
        .map(|tag| block_tag(game, pos, tag)),
        campfire: block_tag(game, pos, "minecraft:campfires"),
        lit: game.native_block_property(pos, "lit").map(|v| v == "true"),
        flame_soul: game
            .native_block_property(pos, "flame_type")
            .map(|v| v == "soul"),
        heat: game
            .native_block_property(pos, "blaze")
            .and_then(|v| super::create_port_burner::heat(&v)),
    }
    .processing()
}

fn flow_limit(game: &Game<'_>, pos: [i32; 3], d: Direction, max: f32) -> f32 {
    for i in 0..max.ceil() as i32 {
        let p = add(pos, d, i + 1);
        if !game.world.loaded(p[0], p[2]) {
            return i as f32;
        }
        if block_tag(game, p, "create:fan_transparent") {
            continue;
        }
        let boxes = game.native_block_collision(p);
        if boxes.is_empty() {
            continue;
        }
        let full_block = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        if boxes.len() == 1 && boxes[0] == full_block {
            return i as f32;
        }
        let boxes_array: Vec<[f64; 6]> = boxes.iter().map(|b| {
            let mn = b.min_position();
            let mx = b.max_position();
            [mn.x, mn.y, mn.z, mx.x, mx.y, mx.z]
        }).collect();
        let depth = fan::shape_depth(&boxes_array, d);
        if depth.is_finite() {
            return ((f64::from(i) + depth + 1.0 / 32.0) as f32).min(max);
        }
    }
    max
}
fn bounds(pos: [i32; 3], d: Direction, distance: f32) -> ([f64; 3], [f64; 3]) {
    let mut min = pos.map(f64::from);
    let mut max = min.map(|v| v + 1.0);
    let axis = d.axis().index();
    if d.vector()[axis] > 0 {
        min[axis] += 1.0;
        max[axis] = min[axis] + f64::from(distance);
    } else {
        max[axis] -= 1.0;
        min[axis] = max[axis] - f64::from(distance);
    }
    (min, max)
}
fn refresh(game: &Game<'_>) {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.recipes_ready {
            return;
        }
        cache.recipes.clear();
        for kind in [Type::Splashing, Type::Haunting] {
            let mut recipes = BTreeMap::new();
            for r in game.native_recipes_of_type(kind.id()) {
                let Some(input) = r.json["ingredients"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(super::create_port::ingredient)
                    .and_then(|i| game.native_recipe_ingredient(&i))
                else {
                    continue;
                };
                let Some(results) = r.json["results"].as_array() else {
                    continue;
                };
                let output: Option<Vec<_>> = results
                    .iter()
                    .map(|j| {
                        let mut j = j.clone();
                        if j.get("id").is_none() {
                            j["id"] = j["item"].clone();
                        }
                        Some((
                            game.native_recipe_result(&j)?,
                            j["chance"].as_f64().unwrap_or(1.0) as f32,
                        ))
                    })
                    .collect();
                let Some(output) = output else {
                    continue;
                };
                for item in input.0 {
                    recipes.entry(item).or_insert_with(|| output.clone());
                }
            }
            cache.recipes.insert(kind, recipes);
        }
        cache.recipes_ready = true;
    });
}
/// Create 6.0.10 tests the effective 1.21.1 FIRE_RESISTANT unit component,
/// not whether an item would survive one particular damage source. The target
/// version replaced it with a selector: only its canonical full-fire tag maps
/// to that unit flag here. Explicit removal wins over a prototype or value.
/// Recognized direct partial/non-fire selectors override the prototype without
/// granting the old full-fire flag. Other tags/values stay unsupported; callers
/// must not destroy such a stack merely because its selector cannot be mapped.
fn blasting_fire_resistance(stack: &ItemStack, prototype: bool) -> Option<bool> {
    let Some(data) = stack.data() else {
        return Some(prototype);
    };
    if data.other("!minecraft:damage_resistant").is_some() {
        return Some(false);
    }
    let Some(value) = data.other("minecraft:damage_resistant") else {
        return Some(prototype);
    };
    let partial = |value: &Tag| {
        let Tag::String(id) = value else { return false };
        let Ok(id) = id.to_string() else { return false };
        matches!(
            id.as_str(),
            "minecraft:lava"
                | "minecraft:in_fire"
                | "minecraft:on_fire"
                | "minecraft:cactus"
                | "minecraft:explosion"
                | "minecraft:player_explosion"
        )
    };
    match value.get("types") {
        Some(Tag::String(id)) if id.to_string().ok().as_deref() == Some("#minecraft:is_fire") => {
            Some(true)
        }
        Some(selector) if partial(selector) => Some(false),
        Some(Tag::List(selectors)) if selectors.iter().all(partial) => Some(false),
        _ => None,
    }
}
fn plan(game: &Game<'_>, stack: &ItemStack, kind: Type) -> Option<Vec<(ItemStack, f32)>> {
    match kind {
        Type::Haunting | Type::Splashing => CACHE.with(|cache| {
            cache
                .borrow()
                .recipes
                .get(&kind)?
                .get(stack.item.as_ref())
                .cloned()
        }),
        Type::Smoking => game
            .recipes()
            .cook("minecraft:smoking", stack)
            .filter(|r| !r.id.ends_with("_manual_only"))
            .map(|r| vec![(r.result.clone(), 1.0)]),
        Type::Blasting => {
            let recipe = game
                .recipes()
                .cook("minecraft:smelting", stack)
                .filter(|r| !r.id.ends_with("_manual_only"))
                .or_else(|| {
                    game.recipes()
                        .cook("minecraft:blasting", stack)
                        .filter(|r| !r.id.ends_with("_manual_only"))
                });
            if let Some(recipe) = recipe
                && game
                    .recipes()
                    .cook("minecraft:smoking", stack)
                    .filter(|r| !r.id.ends_with("_manual_only"))
                    .is_none_or(|smoke| smoke.result.item != recipe.result.item)
            {
                return Some(vec![(recipe.result.clone(), 1.0)]);
            }
            // Blasting destroys smokeable food and non-resistant items with no furnace recipe.
            if recipe.is_some()
                || !blasting_fire_resistance(
                    stack,
                    game.items()
                        .get(&stack.item)
                        .is_some_and(|i| i.fire_resistant),
                )?
            {
                Some(Vec::new())
            } else {
                None
            }
        }
    }
}
fn roll(
    game: &mut Game<'_>,
    stack: &ItemStack,
    outputs: &[(ItemStack, f32)],
    remainder: bool,
) -> Vec<ItemStack> {
    let mut result = Vec::new();
    for _ in 0..stack.count {
        for (output, chance) in outputs {
            let mut output = output.clone();
            output.count = super::create_engine::milling::roll_count(output.count, *chance, || {
                game.world.random.next_float()
            });
            if output.count > 0 {
                let max = super::create_port::stack_maximum(game, &output);
                if let Some(rest) = super::create_port_inventory::insert(&mut result, output, max) {
                    result.push(Some(rest));
                }
            }
        }
        if remainder
            && let Some(item) = game
                .items()
                .get(&stack.item)
                .and_then(|i| i.remainder.clone())
        {
            let output = ItemStack::new(item.as_str(), 1);
            let max = super::create_port::stack_maximum(game, &output);
            if let Some(rest) = super::create_port_inventory::insert(&mut result, output, max) {
                result.push(Some(rest));
            }
        }
    }
    result.into_iter().flatten().collect()
}
fn held_state(nbt: &Compound) -> Processing {
    let kind = match nbt.get(&"FanProcessingType".into()) {
        Some(Tag::String(s)) => s.to_string().ok().as_deref().and_then(Type::parse),
        _ => None,
    };
    Processing {
        kind,
        time: super::create_port::integer(nbt, "FanProcessingTime"),
    }
}
fn dropped_state(game: &Game<'_>, id: u32) -> Processing {
    let tag = game
        .persist
        .raw
        .get(&id)
        .and_then(|t| t.get("NeoForgeData"))
        .and_then(|t| t.get("CreateData"))
        .and_then(|t| t.get("Processing"));
    let Some(Tag::Compound(tag)) = tag else {
        return Processing::default();
    };
    let kind = match tag.get(&"Type".into()) {
        Some(Tag::String(s)) => s.to_string().ok().as_deref().and_then(Type::parse),
        _ => None,
    };
    Processing {
        kind,
        time: super::create_port::integer(tag, "Time"),
    }
}
fn store_dropped(game: &mut Game<'_>, id: u32, state: Processing) {
    let Some(kind) = state.kind else {
        return;
    };
    // Horizon already preserves unknown entity NBT in persist.raw. Use the
    // original NeoForge entity namespace; item components and stacking stay intact.
    let mut tag = game
        .persist
        .raw
        .entry(id)
        .or_insert_with(|| Tag::Compound(Compound::new()));
    for key in ["NeoForgeData", "CreateData", "Processing"] {
        if !matches!(tag, Tag::Compound(_)) {
            *tag = Tag::Compound(Compound::new());
        }
        let Tag::Compound(compound) = tag else {
            unreachable!()
        };
        tag = compound
            .entry(key.into())
            .or_insert_with(|| Tag::Compound(Compound::new()));
    }
    if !matches!(tag, Tag::Compound(_)) {
        *tag = Tag::Compound(Compound::new());
    }
    let Tag::Compound(tag) = tag else {
        unreachable!()
    };
    tag.insert("Type".into(), Tag::String(kind.id().into()));
    tag.insert("Time".into(), Tag::Int(state.time));
}
fn store_held(nbt: &mut Compound, state: Processing) {
    if let Some(kind) = state.kind {
        nbt.insert("FanProcessingType".into(), Tag::String(kind.id().into()));
        nbt.insert("FanProcessingTime".into(), Tag::Int(state.time));
    }
}
fn process_depot(game: &mut Game<'_>, pos: [i32; 3], kind: Type) {
    if super::create_port::inventory_blocked(game, pos) {
        return;
    }
    let Some((stack, mut state)) = game
        .native_with_mod_block_entity(pos, |e| {
            Some((
                e.items.first()?.clone()?,
                held_state(match e.nbt.get(&"HeldItem".into()) {
                    Some(Tag::Compound(tag)) => tag,
                    _ => return None,
                }),
            ))
        })
        .flatten()
    else {
        return;
    };
    let output = plan(game, &stack, kind);
    let convert = state.transported(kind, stack.count, output.is_some());
    let mut results = if convert {
        output.as_ref().map(|o| {
            roll(
                game,
                &stack,
                o,
                matches!(kind, Type::Splashing | Type::Haunting),
            )
        })
    } else {
        None
    };
    let maxima: Vec<_> = results
        .as_ref()
        .into_iter()
        .flatten()
        .map(|s| super::create_port::stack_maximum(game, s))
        .collect();
    let overflow = game
        .native_with_mod_block_entity(pos, |e| {
            if e.items.first() != Some(&Some(stack.clone())) {
                return Vec::new();
            }
            let mut overflow = Vec::new();
            if let Some(ref mut results) = results {
                e.items[0] = None;
                for (i, result) in results.drain(..).enumerate() {
                    if i == 0 {
                        e.items[0] = Some(result);
                    } else if let Some(rest) =
                        super::create_port_inventory::insert(&mut e.items[1..], result, maxima[i])
                    {
                        overflow.push(rest);
                    }
                }
            }
            super::create_port::depot_data(e);
            if let Some(Tag::Compound(tag)) = e.nbt.get_mut(&"HeldItem".into()) {
                store_held(tag, state);
            }
            overflow
        })
        .unwrap_or_default();
    for stack in overflow {
        game.spawn_item(
            stack,
            [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 1.0,
                f64::from(pos[2]) + 0.5,
            ],
            [0.0; 3],
            10,
        );
    }
    if convert {
        game.native_block_entity_sync(pos);
    }
}
struct BeltContext<'a, 'b> {
    game: &'a mut Game<'b>,
}
impl fan::TransportedContext<ItemStack> for BeltContext<'_, '_> {
    fn can_process(
        &mut self,
        stack: &super::create_engine::transport::Stack<ItemStack>,
        kind: Type,
    ) -> bool {
        plan(self.game, &super::create_port_belt::game_stack(stack), kind).is_some()
    }
    fn process(
        &mut self,
        stack: &super::create_engine::transport::Stack<ItemStack>,
        kind: Type,
    ) -> Option<Vec<super::create_engine::transport::Stack<ItemStack>>> {
        let input = super::create_port_belt::game_stack(stack);
        let outputs = plan(self.game, &input, kind)?;
        Some(
            roll(
                self.game,
                &input,
                &outputs,
                matches!(kind, Type::Splashing | Type::Haunting),
            )
            .into_iter()
            .map(|output| super::create_port_belt::core_stack(self.game, output))
            .collect(),
        )
    }
    fn similar_angle(&mut self, input: &super::create_engine::transport::Stack<ItemStack>) -> i32 {
        super::create_port_belt::angle(self.game, &input.payload)
    }
}
fn process_belt(game: &mut Game<'_>, pos: [i32; 3], kind: Type) {
    // AirCurrent uses handleProcessingOnAllItems(.51f). The belt can be stopped:
    // processing is owned by the active fan, independently of belt movement.
    super::create_port_belt::apply_to_each_within(game, pos, 0.51, |game, item| {
        Some(fan::process_transported(
            item,
            kind,
            &mut BeltContext { game },
        ))
    });
}
pub(super) fn tick(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity, rpm: f32) {
    refresh(game);
    let dim = game.dim.id().to_owned();
    let key = (dim.clone(), pos);
    let mut current =
        CACHE.with(|cache| cache.borrow_mut().currents.remove(&key).unwrap_or_default());
    let facing = facing(game, pos);
    let push = fan::pushing(rpm, facing);
    let rebuild = current.facing != Some(facing)
        || current.rpm != rpm
        || current.cooldown <= 0
        || entity.data.first() != Some(&AIR_CURRENT_VERSION);
    current.cooldown -= 1;
    if rebuild {
        current.facing = Some(facing);
        current.rpm = rpm;
        current.cooldown = 30;
        current.distance = if rpm == 0.0 {
            0.0
        } else {
            flow_limit(game, pos, facing, fan::max_distance(rpm))
        };
        current.segments =
            fan::segments(current.distance, push, |i| media(game, add(pos, facing, i)));
        current.handlers.clear();
        for i in 1..=current.distance.ceil() as i32 {
            let kind = fan::type_at(current.distance, push, &current.segments, (i - 1) as f32);
            for offset in 0..=i32::from(facing.axis() != super::create_engine::kinetics::Axis::Y) {
                let mut p = add(pos, facing, i);
                p[1] -= offset;
                let block = game.native_block(p);
                if matches!(block.as_str(), "create:depot" | "create:belt")
                    && matches!(game.world.block_entities.get(&(p[0], p[1], p[2])),
                        Some(horizon_client::containers::BlockEntity::Mod(e)) if e.id == block)
                    && let Some(kind) = media(game, p).or(kind)
                {
                    current.handlers.push((p, kind));
                }
            }
        }
    }
    if rebuild {
        let data = encode_current(
            facing,
            rpm,
            current.distance,
            &current.segments,
            &current.handlers,
        );
        entity.data = data;
        // The transport payload is added by update_tag, never to saved BE NBT.
        game.native_block_entity_sync(pos);
    }
    if rpm != 0.0 && current.distance >= 0.25 {
        let (min, max) = bounds(pos, facing, current.distance);
        if rebuild || current.search <= 0 {
            current.caught = game
                .native_item_entities(&dim, min, max)
                .iter()
                .map(|e| e.id)
                .collect();
            current.search = 5;
        } else {
            current.search -= 1;
        }
        let flow = if push {
            facing.vector()
        } else {
            facing.opposite().vector()
        };
        let origin = pos.map(|v| f64::from(v) + 0.5);
        let caught = std::mem::take(&mut current.caught);
        for id in &caught {
            let Some(index) = game
                .dropped
                .iter()
                .position(|e| e.id == *id && e.stack.count > 0)
            else {
                continue;
            };
            let e = &mut game.dropped[index];
            let p = [e.position.x, e.position.y, e.position.z];
            if (0..3).any(|i| {
                p[i] + if i == 1 { 0.25 } else { 0.125 } <= min[i]
                    || p[i] - if i == 1 { 0.0 } else { 0.125 } >= max[i]
            }) {
                continue;
            }
            current.caught.push(*id);
            let velocity = fan::accelerated(
                [e.velocity.x, e.velocity.y, e.velocity.z],
                p,
                origin,
                flow,
                rpm,
                current.distance,
                false,
            );
            e.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
            e.needs_sync = true;
            let stack = e.stack.clone();
            let axis = facing.axis().index();
            let offset = if facing.vector()[axis] > 0 {
                p[axis] - f64::from(pos[axis]) - 1.0
            } else {
                f64::from(pos[axis]) - p[axis]
            };
            let Some(kind) = fan::type_at(current.distance, push, &current.segments, offset as f32)
            else {
                continue;
            };
            let output = plan(game, &stack, kind);
            let mut processing = dropped_state(game, *id);
            let convert = processing.dropped(kind, stack.count, output.is_some());
            store_dropped(game, *id, processing);
            if convert && let Some(output) = output {
                let mut results = roll(
                    game,
                    &stack,
                    &output,
                    matches!(kind, Type::Splashing | Type::Haunting),
                )
                .into_iter();
                let first = results
                    .next()
                    .unwrap_or_else(|| ItemStack::new("minecraft:air", 0));
                if game.native_set_item_entity(&dim, *id, first) {
                    for stack in results {
                        game.spawn_item(stack, p, velocity, 0);
                    }
                }
            }
        }
        for (p, kind) in &current.handlers {
            match game.native_block(*p).as_str() {
                "create:belt" => process_belt(game, *p, *kind),
                "create:depot" => process_depot(game, *p, *kind),
                _ => {}
            }
        }
    }
    super::create_port_fan_entities::server_tick(
        game,
        pos,
        facing,
        rpm,
        current.distance,
        &current.segments,
    );
    CACHE.with(|cache| cache.borrow_mut().currents.insert(key, current));
}

const AIR_CURRENT_VERSION: i32 = 2;
const AIR_CURRENT_KEY: &str = "CreateHorizonAirCurrent";

/// Protocol-only state for the native client adapter, not canonical Create NBT.
/// API359 does not retain server block tags on a client. Sending the resolved
/// current keeps datapack media/obstructions authoritative across that boundary.
#[derive(Clone, Debug)]
pub(super) struct SyncedCurrent {
    pub facing: Direction,
    pub rpm: f32,
    pub distance: f32,
    pub segments: Vec<Segment>,
    pub handlers: Vec<([i32; 3], Type)>,
}
fn encode_current(
    facing: Direction,
    rpm: f32,
    distance: f32,
    segments: &[Segment],
    handlers: &[([i32; 3], Type)],
) -> Vec<i32> {
    let mut data = vec![
        AIR_CURRENT_VERSION,
        Direction::ALL.iter().position(|d| *d == facing).unwrap() as i32,
        rpm.to_bits() as i32,
        distance.to_bits() as i32,
        segments.len() as i32,
    ];
    for segment in segments {
        data.push(segment.end);
        data.push(match segment.kind {
            None => 0,
            Some(Type::Blasting) => 1,
            Some(Type::Haunting) => 2,
            Some(Type::Smoking) => 3,
            Some(Type::Splashing) => 4,
        });
    }
    data.push(handlers.len() as i32);
    for (pos, kind) in handlers {
        data.extend_from_slice(pos);
        data.push(match kind {
            Type::Blasting => 1,
            Type::Haunting => 2,
            Type::Smoking => 3,
            Type::Splashing => 4,
        });
    }
    data
}
fn decode_current(data: &[i32]) -> Option<SyncedCurrent> {
    if data.len() < 5 || data[0] != AIR_CURRENT_VERSION {
        return None;
    }
    let facing = *Direction::ALL.get(usize::try_from(data[1]).ok()?)?;
    let rpm = f32::from_bits(data[2] as u32);
    let distance = f32::from_bits(data[3] as u32);
    if !rpm.is_finite()
        || !distance.is_finite()
        || distance < 0.0
        || distance > fan::max_distance(rpm)
    {
        return None;
    }
    let count = usize::try_from(data[4]).ok()?;
    let limit = distance.ceil() as i32;
    if count > limit as usize || data.len() < 6 + 2 * count {
        return None;
    }
    let mut segments = Vec::with_capacity(count);
    let push = fan::pushing(rpm, facing);
    for pair in data[5..5 + 2 * count].as_chunks::<2>().0 {
        let end = pair[0];
        if !(0..=limit).contains(&end)
            || segments
                .last()
                .is_some_and(|s: &Segment| if push { end <= s.end } else { end >= s.end })
        {
            return None;
        }
        let kind = match pair[1] {
            0 => None,
            1 => Some(Type::Blasting),
            2 => Some(Type::Haunting),
            3 => Some(Type::Smoking),
            4 => Some(Type::Splashing),
            _ => return None,
        };
        segments.push(Segment { end, kind });
    }
    if distance > 0.0
        && segments
            .last()
            .is_none_or(|s| s.end != if push { limit } else { 0 })
    {
        return None;
    }
    let handler_count = usize::try_from(data[5 + 2 * count]).ok()?;
    if handler_count > (limit as usize) * 2 || data.len() != 6 + 2 * count + 4 * handler_count {
        return None;
    }
    let mut handlers = Vec::with_capacity(handler_count);
    for row in data[6 + 2 * count..].as_chunks::<4>().0 {
        let kind = match row[3] {
            1 => Type::Blasting,
            2 => Type::Haunting,
            3 => Type::Smoking,
            4 => Type::Splashing,
            _ => return None,
        };
        handlers.push(([row[0], row[1], row[2]], kind));
    }
    Some(SyncedCurrent {
        facing,
        rpm,
        distance,
        segments,
        handlers,
    })
}
pub(super) fn update_tag(entity: &ModBlockEntity, tag: &mut Option<Compound>) {
    if entity.id != "create:encased_fan" {
        return;
    }
    // ModBlockEntity.data is explicitly unsaved ContainerData. Fans have no
    // container/menu, so it holds this compact current only until serialization.
    let base = &entity.data;
    let data = if decode_current(base).is_some() {
        base.to_vec()
    } else {
        Vec::new()
    };
    tag.get_or_insert_with(|| entity.nbt.clone())
        .insert(AIR_CURRENT_KEY.into(), Tag::IntArray(data));
}
pub(super) fn synced_current(entity: &ModBlockEntity) -> Option<SyncedCurrent> {
    let Tag::IntArray(data) = entity.nbt.get(&AIR_CURRENT_KEY.into())? else {
        return None;
    };
    let current = decode_current(data)?;
    (current.rpm == super::create_port::number(&entity.nbt, "Speed")).then_some(current)
}

/// Clients consume only validated authoritative flow. An absent/stale payload
/// applies no force; neither thread-local server state nor default client tags
/// can supply an assumed current before the first synchronization.
pub(super) fn client_tick(game: &mut Game<'_>) {
    if game.remote.is_none() || !game.native_on("create") {
        return;
    }
    let dimension = game.dim.id().to_owned();
    let time = game.world.game_time;
    let run = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let id = std::ptr::from_ref(game) as usize;
        if cache.client_game != Some(id)
            || cache
                .client_tick
                .as_ref()
                .is_some_and(|(_, previous)| *previous > time)
        {
            cache.client_origins.clear();
            cache.client_tick = None;
            cache.client_game = Some(id);
            super::create_port_fan_entities::reset(false);
        }
        let stamp = (dimension.clone(), time);
        if cache.client_tick.as_ref() == Some(&stamp) {
            return false;
        }
        cache.client_tick = Some(stamp);
        true
    });
    if !run {
        return;
    }
    super::create_port_fan_entities::maintenance(game);
    let fans: Vec<_> = game
        .world
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), e)| {
            let horizon_client::containers::BlockEntity::Mod(e) = e else {
                return None;
            };
            (e.id == "create:encased_fan").then(|| ([x, y, z], synced_current(e)))
        })
        .collect();
    let live: std::collections::BTreeSet<_> = fans.iter().map(|(pos, _)| *pos).collect();
    let removed = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let mut removed = Vec::new();
        cache.client_origins.retain(|(dim, pos)| {
            let keep = dim == &dimension && live.contains(pos);
            if !keep {
                removed.push((dim.clone(), *pos));
            }
            keep
        });
        removed
    });
    for (dim, pos) in removed {
        super::create_port_fan_entities::clear(&dim, pos);
    }
    for (pos, current) in fans {
        let Some(current) = current.filter(|c| c.facing == facing(game, pos)) else {
            super::create_port_fan_entities::clear(&dimension, pos);
            continue;
        };
        super::create_port_fan_entities::client_tick(
            game,
            pos,
            current.facing,
            current.rpm,
            current.distance,
            &current.segments,
        );
        CACHE.with(|cache| {
            cache
                .borrow_mut()
                .client_origins
                .insert((dimension.clone(), pos))
        });
    }
}
pub(super) fn draw(
    game: &mut Game<'_>,
    pos: [i32; 3],
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) {
    let facing = facing(game, pos);
    let axis = facing.axis();
    let speed = super::create_port::number(&entity.nbt, "Speed");
    let time = draw.game_time as f64 + f64::from(draw.partial);
    for (model, angle) in [
        (
            "create:block/shaft_half",
            super::create_engine::visual::angle(
                time,
                speed,
                super::create_engine::visual::rotation_offset(pos, axis, false),
            ),
        ),
        (
            "create:block/encased_fan/propeller",
            super::create_engine::visual::angle(time, fan::propeller_speed(speed), 0.0),
        ),
    ] {
        draw.push();
        draw.rotate_about(super::create_engine::visual::positive_axis(axis), angle, [0.5; 3]);
        super::create_port::face_south(draw, facing.opposite());
        game.native_draw_model(draw, model, 0xFFFFFF);
        draw.pop();
    }
}
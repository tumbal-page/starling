// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create 6.0.10 BasinBlockEntity/BasinInventory/BasinRenderer adapter.
// Heat-aware, unfiltered, downward basin; two canonical water/lava tank halves.
// Unknown tanks, filters and spouts remain unsupported; never discard them.
// Adapted for Android bundled port.

use super::{
    Game, GameLevel, create_port_fluid_codec as fluids, native_block_entity_draw::BlockEntityDraw,
};
use crate::{
    containers::{BlockEntity, ModBlockEntity, ModContainer, ModRevision},
    items::ItemStack,
};
use android_bundled::create::crates::engine::{basin as rules, transport::SessionCache};
use horizon_client::mod_api::{
    NativeFluidAction, NativeFluidDrain, NativeFluidRenderOptions, NativeFluidStack,
    NativeFluidTankView,
};
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_world::nbt::{Compound, Tag};
use std::{cell::RefCell, collections::BTreeMap};

const ID: &str = "create:basin";
type Pos = [i32; 3];

#[derive(Clone, Debug)]
pub(super) struct Key(pub ItemStack);
impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.0.same_item_same_components(&other.0)
    }
}
impl Eq for Key {}
pub(super) type Basin = rules::Basin<Key, fluids::Key>;

#[derive(Default)]
struct Motion {
    owner: Option<ModRevision>,
    tick: Option<i64>,
    previous: f32,
    angle: f32,
    speed: f32,
    lazy: i32,
    moving: bool,
    fluid_levels: [rules::FluidLevel; 4],
    received_fluids: [Option<fluids::Level>; 4],
    rendered_fluids: [Option<rules::Fluid<fluids::Key>>; 4],
}
struct Server {
    owner: Option<ModRevision>,
    initialized: bool,
    contents_changed: bool,
    tick: Option<i64>,
    lazy: i32,
    backup: i32,
    heat: rules::HeatCache,
}
impl Default for Server {
    fn default() -> Self {
        Self {
            owner: None,
            initialized: false,
            contents_changed: false,
            tick: None,
            lazy: 10,
            backup: 20,
            heat: rules::HeatCache::default(),
        }
    }
}
impl Server {
    fn bind(&mut self, owner: ModRevision) {
        if self.owner.is_some_and(|old| !old.same_entity(owner)) {
            *self = Self::default();
        }
        self.owner = Some(owner);
    }
}
thread_local! {
    static MOTION: RefCell<SessionCache<BTreeMap<Pos, Motion>>> = RefCell::new(SessionCache::default());
    static SERVER: RefCell<SessionCache<BTreeMap<Pos, Server>>> = RefCell::new(SessionCache::default());
}

pub(super) fn reset(reload: bool) {
    if !reload {
        MOTION.with(|m| m.borrow_mut().clear());
        SERVER.with(|m| m.borrow_mut().clear());
    }
}

/// BasinInventory.whenContentsChanged/notifyChangeOfContents only raises a
/// flag. The next ordinary Basin tick schedules its operator checker.
pub(super) fn notify_contents(game: &Game<'_>, pos: Pos) {
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
        return;
    }
    SERVER.with(|cache| {
        let mut cache = cache.borrow_mut();
        let server = cache
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .entry(pos)
            .or_default();
        server.bind(entity.revision());
        server.contents_changed = true;
    });
}

pub(super) fn container(block: &str, container: &mut Option<ModContainer>) {
    if block == ID && container.is_none() {
        *container = Some(ModContainer {
            slots: 18,
            faces: std::array::from_fn(|_| (0..18).collect()),
            inputs: (0..9).collect(),
            // CombinedInvWrapper exposes BOTH input and output to extraction.
            outputs: (0..18).collect(),
            menu: None,
        });
    }
}

fn inventory(slots: &[Option<ItemStack>]) -> Tag {
    Tag::Compound(Compound::from([
        ("Size".into(), Tag::Int(9)),
        (
            "Items".into(),
            Tag::List(
                slots
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| {
                        let stack = s.as_ref().filter(|s| s.count > 0)?;
                        let Tag::Compound(mut nbt) = horizon_items::item_nbt::item_to_nbt(stack)
                        else {
                            return None;
                        };
                        nbt.insert("Slot".into(), Tag::Int(i as i32));
                        Some(Tag::Compound(nbt))
                    })
                    .collect(),
            ),
        ),
    ]))
}
fn restore(entity: &mut ModBlockEntity) {
    // Canonical inventory is decoded before publication. Never re-read an old
    // bridge snapshot after ordinary automation has already changed live slots.
    if crate::game::create_port::inventory_supported(entity) {
        entity.items.resize(18, None);
    }
}
pub(super) fn data(entity: &mut ModBlockEntity) -> bool {
    if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
        return false;
    }
    entity.items.resize(18, None);
    let mut changed = false;
    for (key, range) in [("InputItems", 0..9), ("OutputItems", 9..18)] {
        let Tag::Compound(updated) = inventory(&entity.items[range]) else {
            unreachable!();
        };
        let mut value = match entity.nbt.get(&key.into()) {
            Some(Tag::Compound(existing)) => existing.clone(),
            _ => Compound::new(),
        };
        value.extend(updated);
        let value = Tag::Compound(value);
        changed |= entity.nbt.get(&key.into()) != Some(&value);
        entity.nbt.insert(key.into(), value.clone());
    }
    changed
}
fn empty_tanks() -> Tag {
    let segment = Tag::Compound(Compound::from([
        (
            "Level".into(),
            Tag::Compound(Compound::from([
                ("Force".into(), Tag::Byte(1)),
                ("Speed".into(), Tag::Float(0.25f32.to_bits())),
                ("Target".into(), Tag::Float(0.0f32.to_bits())),
                ("Value".into(), Tag::Float(0.0f32.to_bits())),
            ])),
        ),
        ("TankContent".into(), Tag::Compound(Compound::new())),
    ]));
    Tag::List(vec![segment.clone(), segment])
}
pub(super) fn initialize(entity: &mut ModBlockEntity) {
    if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
        return;
    }
    restore(entity);
    for key in ["InputTanks", "OutputTanks"] {
        entity.nbt.entry(key.into()).or_insert_with(empty_tanks);
    }
    for key in ["DisabledSpoutput", "Overflow", "FluidOverflow"] {
        entity
            .nbt
            .entry(key.into())
            .or_insert_with(|| Tag::List(Vec::new()));
    }
    entity
        .nbt
        .entry("Filter".into())
        .or_insert_with(|| Tag::Compound(Compound::new()));
    entity
        .nbt
        .entry("FilterAmount".into())
        .or_insert(Tag::Int(64));
    entity.nbt.entry("UpTo".into()).or_insert(Tag::Byte(1));
    data(entity);
}

fn nonempty(tag: Option<&Tag>) -> bool {
    match tag {
        None => false,
        Some(Tag::Compound(c)) => !c.is_empty(),
        Some(Tag::List(l)) => !l.is_empty(),
        _ => true,
    }
}
pub(super) fn unsupported(
    game: &Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
) -> Option<&'static str> {
    if !crate::game::create_port::inventory_supported(entity) {
        return Some("basin inventory sizes are not supported");
    }
    if game.native_block_property(pos, "facing").as_deref() != Some("down") {
        return Some("basin side spouts are not implemented");
    }
    if nonempty(entity.nbt.get(&"Filter".into())) {
        return Some("basin recipe filters are not implemented");
    }
    for key in ["InputTanks", "OutputTanks"] {
        if let Err(fluids::Unsupported(reason)) = read_tanks(entity, key) {
            return Some(reason);
        }
    }
    if [
        "Overflow",
        "FluidOverflow",
        "PreferredSpoutput",
        "DisabledSpoutput",
    ]
    .into_iter()
    .any(|key| nonempty(entity.nbt.get(&key.into())))
    {
        return Some("basin spout buffers are not implemented");
    }
    None
}
fn component_maximum(stack: &ItemStack) -> Option<u32> {
    super::create_port::stack_maximum_override(stack)
}
pub(super) fn max_stack(game: &Game<'_>, stack: &ItemStack) -> u32 {
    super::create_port::stack_maximum(game, stack)
}
pub(super) fn has_analog(
    game: &GameLevel<'_>,
    state: horizon_world::registry::BlockStateId,
    output: &mut bool,
) {
    if game.facts.get(state).name.as_ref() == ID {
        *output = true;
    }
}
pub(super) fn analog(
    game: &GameLevel<'_>,
    pos: horizon_core::position::BlockPos,
    state: horizon_world::registry::BlockStateId,
    output: &mut i32,
) {
    if game.facts.get(state).name.as_ref() != ID {
        return;
    }
    *output = 0;
    let Some(BlockEntity::Mod(e)) = game.world.block_entities.get(&(pos.x, pos.y, pos.z)) else {
        return;
    };
    // Unsupported native-created inventories may not have canonical slots yet.
    // Keep their payload opaque and fail closed before any comparator indexing.
    if !crate::game::create_port::inventory_supported(e) {
        return;
    }
    let mut fullness = 0.0f32;
    let mut occupied = false;
    for stack in e.items.iter().take(9).flatten().filter(|s| s.count > 0) {
        let maximum = component_maximum(stack)
            .unwrap_or_else(|| game.items.get(&stack.item).map_or(64, |i| i.max_stack))
            .min(64);
        fullness += stack.count as f32 / maximum as f32;
        occupied = true;
    }
    *output = (fullness / 9.0 * 14.0).floor() as i32 + i32::from(occupied);
}
pub(super) fn can_survive(
    game: &GameLevel<'_>,
    pos: horizon_core::position::BlockPos,
    state: horizon_world::registry::BlockStateId,
    survives: &mut Option<bool>,
) {
    let block = game.facts.get(state).name.as_ref();
    if block == "create:mechanical_mixer" {
        *survives = Some(
            !matches!(game.world.block_entities.get(&(pos.x,pos.y-1,pos.z)),Some(BlockEntity::Mod(e)) if e.id == ID),
        );
        return;
    }
    if block != ID {
        return;
    }
    *survives = Some(
        !matches!(game.world.block_entities.get(&(pos.x,pos.y+1,pos.z)),Some(BlockEntity::Mod(e)) if matches!(e.id.as_str(),"create:mechanical_mixer"|"create:mechanical_press")),
    );
}
pub(super) fn core_stack(game: &Game<'_>, stack: &ItemStack) -> rules::Item<Key> {
    let mut key = stack.clone();
    key.count = 1;
    rules::Item {
        key: Key(key),
        count: stack.count,
        maximum: max_stack(game, stack),
    }
}
pub(super) fn native_stack(item: &rules::Item<Key>) -> ItemStack {
    ItemStack {
        count: item.count,
        ..item.key.0.clone()
    }
}
pub(super) fn read(game: &Game<'_>, pos: Pos) -> Option<Basin> {
    let BlockEntity::Mod(entity) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))? else {
        return None;
    };
    if entity.id != ID || unsupported(game, pos, entity).is_some() {
        return None;
    }
    Some(Basin {
        input: std::array::from_fn(|i| {
            entity
                .items
                .get(i)
                .and_then(Option::as_ref)
                .filter(|s| s.count > 0)
                .map(|s| core_stack(game, s))
        }),
        output: std::array::from_fn(|i| {
            entity
                .items
                .get(9 + i)
                .and_then(Option::as_ref)
                .filter(|s| s.count > 0)
                .map(|s| core_stack(game, s))
        }),
        input_fluids: read_tanks(entity, "InputTanks").ok()?,
        output_fluids: read_tanks(entity, "OutputTanks").ok()?,
        ..Basin::default()
    })
}
pub(super) fn heat(game: &Game<'_>, pos: Pos) -> rules::Heat {
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return rules::Heat::None;
    };
    if entity.id != ID {
        return rules::Heat::None;
    }
    SERVER.with(|cache| {
        let mut cache = cache.borrow_mut();
        let server = cache
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .entry(pos)
            .or_default();
        server.bind(entity.revision());
        server.heat.get_or_read(|| {
            let state = game.world.block(pos[0], pos[1] - 1, pos[2]);
            let Some(state) = game.pack.assets.registry.state(state) else {
                return Some(rules::Heat::None);
            };
            let blaze = state
                .properties
                .get("blaze")
                .map(|value| match value.as_str() {
                    "smouldering" => rules::Heat::Smouldering,
                    "fading" => rules::Heat::Fading,
                    "kindled" => rules::Heat::Kindled,
                    "seething" => rules::Heat::Seething,
                    _ => rules::Heat::None,
                });
            // Same source priority and short-circuit: a real blaze level
            // wins before passive tags or campfire lit state are tested.
            if let Some(blaze) = blaze {
                return Some(blaze);
            }
            let passive = game
                .native_server_block_in_tag(state, "create:passive_boiler_heaters")
                .unwrap_or(false);
            let campfire = passive
                && game
                    .native_server_block_in_tag(state, "minecraft:campfires")
                    .unwrap_or(false);
            let lit = state.properties.get("lit").map(|v| v == "true");
            Some(rules::heat_for_block(blaze, passive, campfire, lit))
        })
    })
}
pub(super) fn write(game: &mut Game<'_>, pos: Pos, basin: &Basin) -> bool {
    let Some(changed) = game.native_with_mod_block_entity(pos, |entity| {
        if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
            return false;
        }
        // Preflight both canonical halves before any item/fluid write. Unsupported
        // original data is retained; it cannot become an empty tank by accident.
        let Ok((input, output)) = replacement_tanks(entity, basin) else {
            return false;
        };
        let fluid_changed = set_tanks(entity, input, output);
        entity.items = basin
            .input
            .iter()
            .chain(&basin.output)
            .map(|s| s.as_ref().map(native_stack))
            .collect();
        data(entity) || fluid_changed
    }) else {
        return false;
    };
    if changed {
        game.native_block_entity_sync(pos);
        notify_contents(game, pos);
    }
    changed
}

fn read_tanks(entity: &ModBlockEntity, key: &str) -> Result<fluids::Tanks, fluids::Unsupported> {
    // A fresh factory-created entity has not yet had its first initialization
    // tick. Reading/simulating that default must not write or dirty the entity.
    let empty = empty_tanks();
    fluids::read_half(Some(entity.nbt.get(&key.into()).unwrap_or(&empty)))
}
fn replacement_tanks(
    entity: &ModBlockEntity,
    basin: &Basin,
) -> Result<(Tag, Tag), fluids::Unsupported> {
    let empty = empty_tanks();
    Ok((
        fluids::write_half(
            Some(entity.nbt.get(&"InputTanks".into()).unwrap_or(&empty)),
            &basin.input_fluids,
        )?,
        fluids::write_half(
            Some(entity.nbt.get(&"OutputTanks".into()).unwrap_or(&empty)),
            &basin.output_fluids,
        )?,
    ))
}
fn set_tanks(entity: &mut ModBlockEntity, input: Tag, output: Tag) -> bool {
    let changed = entity.nbt.get(&"InputTanks".into()) != Some(&input)
        || entity.nbt.get(&"OutputTanks".into()) != Some(&output);
    if changed {
        entity.nbt.insert("InputTanks".into(), input);
        entity.nbt.insert("OutputTanks".into(), output);
    }
    changed
}
fn fluid_state(game: &Game<'_>, pos: Pos) -> Option<Basin> {
    let BlockEntity::Mod(entity) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))? else {
        return None;
    };
    if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
        return None;
    }
    Some(Basin {
        input_fluids: read_tanks(entity, "InputTanks").ok()?,
        output_fluids: read_tanks(entity, "OutputTanks").ok()?,
        ..Basin::default()
    })
}
fn commit_fluids(game: &mut Game<'_>, pos: Pos, state: &Basin) {
    let changed = game
        .native_with_mod_block_entity(pos, |entity| {
            if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
                return false;
            }
            let Ok((input, output)) = replacement_tanks(entity, state) else {
                return false;
            };
            set_tanks(entity, input, output)
        })
        .unwrap_or(false);
    if changed {
        game.native_block_entity_sync(pos);
        notify_contents(game, pos);
    }
}
fn core_fluid(stack: &NativeFluidStack) -> Option<rules::Fluid<fluids::Key>> {
    (stack.amount > 0 && matches!(stack.fluid.as_str(), "minecraft:water" | "minecraft:lava")).then(
        || rules::Fluid {
            key: fluids::Key {
                id: stack.fluid.clone(),
                components: stack.components.clone(),
            },
            amount: stack.amount as u32,
        },
    )
}
fn native_fluid(stack: &rules::Fluid<fluids::Key>) -> NativeFluidStack {
    NativeFluidStack {
        fluid: stack.key.id.clone(),
        amount: stack.amount as i32,
        components: stack.key.components.clone(),
    }
}
/// Generic API6 ownership: all source sides, including explicit unsided access.
pub(super) fn custom_fluids(block: &str, custom: &mut bool) {
    if block == ID {
        *custom = true;
    }
}
pub(super) fn fluid_tanks(
    game: &Game<'_>,
    pos: Pos,
    _side: Option<horizon_core::geometry::Direction>,
    out: &mut Option<Vec<NativeFluidTankView>>,
) {
    let Some(state) = fluid_state(game, pos) else {
        return;
    };
    *out = Some(
        (0..4)
            .map(|tank| NativeFluidTankView {
                tank,
                content: state.fluid(tank).map(native_fluid),
                capacity: rules::TANK_CAPACITY as i32,
                can_fill: tank >= 2,
                can_drain: true,
            })
            .collect(),
    );
}
pub(super) fn fluid_fill(
    game: &mut Game<'_>,
    pos: Pos,
    _side: Option<horizon_core::geometry::Direction>,
    resource: &NativeFluidStack,
    action: NativeFluidAction,
    out: &mut Option<i32>,
) {
    let Some(mut state) = fluid_state(game, pos) else {
        return;
    };
    *out = Some(0);
    let Some(resource) = core_fluid(resource) else {
        return;
    };
    // Injected into the defining crate today; reject future external variants.
    #[allow(unreachable_patterns)]
    let simulate = match action {
        NativeFluidAction::Simulate => true,
        NativeFluidAction::Execute => false,
        _ => return,
    };
    let amount = state.fill_input_fluid(&resource, simulate);
    if !simulate && amount > 0 {
        commit_fluids(game, pos, &state);
    }
    *out = Some(amount as i32);
}
pub(super) fn fluid_drain(
    game: &mut Game<'_>,
    pos: Pos,
    _side: Option<horizon_core::geometry::Direction>,
    request: &NativeFluidDrain,
    action: NativeFluidAction,
    out: &mut Option<NativeFluidStack>,
) {
    let Some(mut state) = fluid_state(game, pos) else {
        return;
    };
    let selected;
    let request = match request {
        NativeFluidDrain::Any(maximum) if *maximum > 0 => rules::FluidRequest::Any(*maximum as u32),
        NativeFluidDrain::Exact(stack) => {
            let Some(stack) = core_fluid(stack) else {
                return;
            };
            selected = stack;
            rules::FluidRequest::Exact(&selected)
        }
        _ => return,
    };
    // Injected into the defining crate today; reject future external variants.
    #[allow(unreachable_patterns)]
    let simulate = match action {
        NativeFluidAction::Simulate => true,
        NativeFluidAction::Execute => false,
        _ => return,
    };
    let report = state.drain_fluid(request, simulate);
    if !simulate && report.consumed > 0 {
        commit_fluids(game, pos, &state);
    }
    *out = report.returned.as_ref().map(native_fluid);
}
/// `BasinInventory.insertItem`: unique component-bearing stack per half and
/// only its first empty slot. Normal hopper transfers are one item at a time.
pub(super) fn can_place(
    slots: &[Option<ItemStack>],
    local: usize,
    stack: &ItemStack,
    maximum: u32,
) -> bool {
    let maximum = component_maximum(stack).unwrap_or(maximum);
    if local >= 9 || slots.len() < 9 || stack.count == 0 {
        return false;
    }
    let input = &slots[..9];
    if input.iter().enumerate().any(|(i, s)| {
        i != local
            && s.as_ref()
                .is_some_and(|s| s.count > 0 && s.same_item_same_components(stack))
    }) {
        return false;
    }
    match input[local].as_ref().filter(|s| s.count > 0) {
        Some(held) => held.count < maximum.min(64) && held.same_item_same_components(stack),
        None => {
            input
                .iter()
                .position(|s| s.as_ref().is_none_or(|s| s.count == 0))
                == Some(local)
        }
    }
}

pub(super) fn tick(game: &mut Game<'_>, pos: Pos, entity: &mut ModBlockEntity) -> bool {
    if entity.id != ID || !crate::game::create_port::inventory_supported(entity) {
        return false;
    }
    restore(entity);
    let changed = data(entity);
    initialize(entity);
    let owner = entity.revision();
    let already_ticked = SERVER.with(|cache| {
        let mut cache = cache.borrow_mut();
        let server = cache
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .entry(pos)
            .or_default();
        server.bind(owner);
        server.tick == Some(game.world.game_time)
    });
    if already_ticked {
        return changed;
    }
    SERVER.with(|cache| {
        cache
            .borrow_mut()
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .entry(pos)
            .or_default()
            .heat
            .reset()
    });
    for key in ["InputTanks", "OutputTanks"] {
        if let Some(tanks) = entity.nbt.get_mut(&key.into()) {
            let _ = fluids::tick_half(tanks);
        }
    }
    let scheduled = SERVER.with(|cache| {
        let mut cache = cache.borrow_mut();
        let s = cache
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .entry(pos)
            .or_default();
        if s.tick == Some(game.world.game_time) {
            return false;
        }
        if s.tick.is_some_and(|t| t > game.world.game_time) {
            *s = Server {
                owner: Some(owner),
                ..Server::default()
            };
        }
        // The duplicate-tick return above deliberately leaves this flag set.
        let mut scheduled = std::mem::take(&mut s.contents_changed) || !s.initialized;
        if !s.initialized {
            s.initialized = true;
            s.backup -= 1;
        }
        if s.lazy <= 0 {
            s.lazy = 10;
            if s.backup <= 0 {
                s.backup = 20;
                scheduled |= entity.items.iter().flatten().any(|s| s.count > 0)
                    || ["InputTanks", "OutputTanks"].into_iter().any(|key| {
                        read_tanks(entity, key)
                            .is_ok_and(|tanks| tanks.iter().flatten().any(|f| f.amount > 0))
                    });
            } else {
                s.backup -= 1;
            }
        } else {
            s.lazy -= 1;
        }
        s.tick = Some(game.world.game_time);
        scheduled
    });
    // Canonical serialization may normalize a supported loaded inventory's
    // order/numeric tags. Basin.read does not notify contents: only genuine
    // inventory/fluid events, initialization and the lazy backup wake it.
    if scheduled {
        crate::game::create_port_mixer::schedule(game, [pos[0], pos[1] + 2, pos[2]]);
        crate::game::create_port_press_basin::schedule(game, [pos[0], pos[1] + 2, pos[2]]);
    }
    if changed {
        let dim = game.dim.id().to_owned();
        game.native_update_output_neighbors(&dim, pos);
    }
    let reason = unsupported(game, pos, entity);
    match reason {
        Some(reason) => {
            entity.nbt.insert(
                "create_horizon:Unsupported".into(),
                Tag::String(reason.into()),
            );
        }
        None => {
            entity.nbt.remove(&"create_horizon:Unsupported".into());
        }
    }
    // Falling-item intake deliberately waits for the entity-sensitive basin
    // collision API; no guessed contact height or direct teleport is used.
    changed
}
pub(super) fn reacts(game: &Game<'_>, pos: Pos) -> bool {
    game.native_on("create")
        && game.native_block(pos) == ID
        && game.held().is_none_or(|s| {
            s.count == 0
                || matches!(
                    s.item.as_ref(),
                    "minecraft:air"
                        | "minecraft:water_bucket"
                        | "minecraft:lava_bucket"
                        | "minecraft:bucket"
                        | "minecraft:sponge"
                )
        })
}
fn give_container(game: &mut Game<'_>, pos: Pos, stack: ItemStack) {
    let player = game.native_acting_id();
    if let Some(rest) = game.native_give_stack(player, stack) {
        if let Some(eye) = game.eye_position() {
            game.throw_from_eyes(rest, eye, false);
        } else {
            game.spawn_item(rest, pos.map(|v| f64::from(v) + 0.5), [0.0, 0.2, 0.0], 10);
        }
    }
}
fn bucket_click(game: &mut Game<'_>, pos: Pos, held: &ItemStack) -> bool {
    let dim = game.dim.id().to_owned();
    match held.item.as_ref() {
        "minecraft:water_bucket" | "minecraft:lava_bucket" => {
            let Some((fluid, amount, remainder)) = game.native_item_fluid_drain(held, 1000) else {
                return true;
            };
            let fluid = NativeFluidStack {
                fluid,
                amount,
                components: Compound::new(),
            };
            if game.native_fluid_fill_stack(&dim, pos, None, &fluid, NativeFluidAction::Simulate)
                != amount
            {
                return true;
            }
            game.native_fluid_fill_stack(&dim, pos, None, &fluid, NativeFluidAction::Execute);
            if !game.creative {
                if held.count == 1 {
                    *game.held_mut() = Some(remainder);
                } else {
                    if let Some(s) = game.held_mut() {
                        s.count -= 1;
                    }
                    give_container(game, pos, remainder);
                }
            }
            true
        }
        "minecraft:bucket" => {
            let Some(tanks) = game.native_fluid_view(&dim, pos, None) else {
                return true;
            };
            for tank in tanks {
                let Some(mut fluid) = tank.content else {
                    continue;
                };
                // API item containers cannot carry an opaque fluid component
                // patch. Refuse it explicitly rather than flattening it.
                if !fluid.components.is_empty() || fluid.amount < 1000 {
                    continue;
                }
                let Some((amount, filled)) = game.native_item_fluid_fill(held, &fluid.fluid, 1000)
                else {
                    continue;
                };
                if amount != 1000 {
                    continue;
                }
                fluid.amount = amount;
                game.native_fluid_drain_stack(
                    &dim,
                    pos,
                    None,
                    &NativeFluidDrain::Exact(fluid),
                    NativeFluidAction::Execute,
                );
                if !game.creative {
                    game.consume_held();
                    give_container(game, pos, filled);
                }
                return true;
            }
            true // GenericItemFilling claims even a refused/insufficient bucket.
        }
        "minecraft:sponge" => game
            .native_fluid_drain_stack(
                &dim,
                pos,
                None,
                &NativeFluidDrain::Any(i32::MAX),
                NativeFluidAction::Execute,
            )
            .is_some(),
        _ => false,
    }
}
pub(super) fn right_click(game: &mut Game<'_>, pos: Pos) -> bool {
    if crate::game::create_port::inventory_blocked(game, pos) {
        return true;
    }
    if !reacts(game, pos) {
        return false;
    }
    if let Some(held) = game
        .held()
        .filter(|s| s.count > 0 && s.item.as_ref() != "minecraft:air")
        .cloned()
    {
        return bucket_click(game, pos, &held);
    }
    let stacks = game
        .native_with_mod_block_entity(pos, |entity| {
            restore(entity);
            let stacks = entity
                .items
                .iter_mut()
                .filter_map(Option::take)
                .collect::<Vec<_>>();
            data(entity);
            stacks
        })
        .unwrap_or_default();
    let player = game.native_acting_id();
    let success = !stacks.is_empty();
    for stack in stacks {
        if let Some(rest) = game.native_give_stack(player, stack) {
            if let Some(eye) = game.eye_position() {
                game.throw_from_eyes(rest, eye, false);
            } else {
                game.spawn_item(rest, pos.map(|v| f64::from(v) + 0.5), [0.0, 0.2, 0.0], 10);
            }
        }
    }
    if success {
        let pitch = 1.0 + game.world.random.next_float();
        game.with_level(|_, level, _| {
            level.native_play_sound(
                "minecraft:entity.item.pickup",
                pos.map(|v| f64::from(v) + 0.5),
                7,
                0.2,
                pitch,
            );
        });
    }
    game.native_block_entity_sync(pos);
    crate::game::create_port_mixer::basin_removed(game, [pos[0], pos[1] + 2, pos[2]]);
    crate::game::create_port_press_basin::basin_removed(game, [pos[0], pos[1] + 2, pos[2]]);
    true
}

pub(super) fn removed(game: &mut Game<'_>, pos: Pos) {
    SERVER.with(|cache| {
        cache
            .borrow_mut()
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .remove(&pos);
    });
    crate::game::create_port_mixer::basin_removed(game, [pos[0], pos[1] + 2, pos[2]]);
    crate::game::create_port_press_basin::basin_removed(game, [pos[0], pos[1] + 2, pos[2]]);
}
pub(super) fn maintenance(game: &mut Game<'_>) {
    SERVER.with(|cache| {
        cache
            .borrow_mut()
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .retain(|p, _| game.world.loaded(p[0], p[2]) && game.native_block(*p) == ID);
    });
}
pub(super) fn client_tick(game: &mut Game<'_>) {
    if !game.native_window() || game.remote.is_none() || !game.native_on("create") {
        return;
    }
    let time = game.world.game_time;
    let session = std::ptr::from_ref(&game.world) as usize;
    let dimension = game.dim.id().to_owned();
    let entries: Vec<_> = game
        .world
        .block_entities
        .iter()
        .filter_map(|(&(x, y, z), e)| match e {
            BlockEntity::Mod(e) if e.id == ID => Some(([x, y, z], e.revision())),
            _ => None,
        })
        .collect();
    let live: std::collections::BTreeSet<_> = entries.iter().map(|(pos, _)| *pos).collect();
    MOTION.with(|cache| {
        let mut cache = cache.borrow_mut();
        let cache = cache.dimension(session, &dimension);
        cache.retain(|p, _| live.contains(p));
        for (pos, owner) in entries {
            let m = cache.entry(pos).or_default();
            if m.tick.is_some_and(|t| t > time)
                || m.owner.is_some_and(|old| !old.same_entity(owner))
            {
                *m = Motion::default();
            }
            m.owner = Some(owner);
            if m.tick == Some(time) { continue; }
            // read(clientPacket) updates the chase target; the lazy tick is a
            // fallback for absent/removed mixers, as in BasinBlockEntity.
            let mixer = game.world.block_entities.get(&(pos[0],pos[1]+2,pos[2]));
            let moving = matches!(mixer, Some(BlockEntity::Mod(e)) if e.id == "create:mechanical_mixer" && crate::game::create_port::integer(&e.nbt,"Running") != 0 && crate::game::create_port::integer(&e.nbt,"Ticks") <= 20);
            if m.tick.is_none() || m.lazy <= 0 || m.moving != moving {
                m.moving = moving;
                m.lazy = 10;
            } else { m.lazy -= 1; }
            m.previous = m.angle;
            m.speed += ((if m.moving {20.0} else {0.0}) - m.speed) * 0.1;
            m.angle += m.speed;
            if let Some(BlockEntity::Mod(entity))=game.world.block_entities.get(&(pos[0],pos[1],pos[2])) {
                for (half,key) in ["InputTanks","OutputTanks"].into_iter().enumerate() {
                    let Ok(levels)=fluids::read_levels(entity.nbt.get(&key.into())) else {continue;};
                    let Ok(contents)=read_tanks(entity,key) else {continue;};
                    for (i,level) in levels.into_iter().enumerate() {
                        let index=half*2+i;
                        if let Some(fluid)=contents[i].as_ref(){m.rendered_fluids[index]=Some(fluid.clone());}
                        let chase=&mut m.fluid_levels[index];
                        if m.received_fluids[index]!=Some(level) {
                            if level.force {chase.value=level.value;chase.previous=level.value;}
                            chase.target=level.target;chase.speed=level.speed;
                            m.received_fluids[index]=Some(level);
                        }
                        chase.tick();
                    }
                }
            }
            m.tick = Some(time);
        }
    });
}
fn draw_fluids(
    game: &Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) -> f32 {
    let rendered = MOTION.with(|cache| {
        let mut cache = cache.borrow_mut();
        let motion = cache
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .get(&pos);
        std::array::from_fn::<_, 4, _>(|i| {
            if let Some(m) = motion {
                return (
                    m.rendered_fluids[i].clone(),
                    m.fluid_levels[i].at(draw.partial) * rules::TANK_CAPACITY as f32,
                );
            }
            let key = if i < 2 { "InputTanks" } else { "OutputTanks" };
            let fluid = read_tanks(entity, key).ok().and_then(|t| t[i % 2].clone());
            let units = fluids::read_levels(entity.nbt.get(&key.into()))
                .map_or(0.0, |l| l[i % 2].value * rules::TANK_CAPACITY as f32);
            (fluid, units)
        })
    });
    let total: f32 = rendered.iter().map(|(_, units)| units).sum();
    if total < 1.0 {
        return 0.0;
    }
    let ratio = (total / 2000.0).clamp(0.0, 1.0);
    let height = 2.0 / 16.0 + 12.0 / 16.0 * (1.0 - (1.0 - ratio) * (1.0 - ratio));
    let mut x = 2.0 / 16.0;
    for (fluid, units) in rendered {
        let Some(fluid) = fluid else {
            continue;
        };
        if units < 1.0 {
            continue;
        }
        let next = x + (units / total).clamp(0.0, 1.0) * 12.0 / 16.0;
        game.native_draw_fluid_box(
            draw,
            &native_fluid(&fluid),
            [x, 2.0 / 16.0, 2.0 / 16.0, next, height, 14.0 / 16.0],
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: false,
                // Actual source fixture's stitched blocks atlas is 2048x1024.
                // This is explicit because Horizon stores block sprites in arrays.
                uv_shrink_ratio: 4.0 / 2048.0,
            },
        );
        x = next;
    }
    height
}
pub(super) fn draw(
    game: &Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) -> bool {
    if entity.id != ID {
        return false;
    }
    // Retained motion belongs to this live BE, including ordinary NBT changes.
    MOTION.with(|cache| {
        let mut cache = cache.borrow_mut();
        let motions = cache.dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id());
        if motions.get(&pos).is_some_and(|m| {
            m.owner
                .is_some_and(|old| !old.same_entity(entity.revision()))
        }) {
            motions.remove(&pos);
        }
    });
    // restore() truncates supported inventories to18, while opaque unsupported
    // inventories retain every live slot. Borrow the same visible range.
    let end = if crate::game::create_port::inventory_supported(entity) {
        entity.items.len().min(18)
    } else {
        entity.items.len()
    };
    let items = &entity.items[..end];
    let fluid_height = draw_fluids(game, pos, entity, draw);
    let item_height = (fluid_height - 0.3).clamp(0.125, 0.6);
    let mut count = items.iter().flatten().filter(|s| s.count > 0).count();
    if count == 0 {
        return true;
    }
    let rotation = MOTION.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache
            .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
            .get(&pos)
            .map_or(0.0, |m| m.previous + (m.angle - m.previous) * draw.partial)
    });
    // BlockPos.hashCode and RandomSource.create preserve stable item pile offsets.
    let hash = pos[1]
        .wrapping_add(pos[2].wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos[0]);
    let mut random = LegacyRandom::new(i64::from(hash));
    let partition = 360.0 / count as f32;
    let radius = if count == 1 { 0.0 } else { 0.125 };
    draw.push();
    draw.translate([0.5, 0.2, 0.5]);
    draw.rotate_about([0.0, 1.0, 0.0], rotation, [0.0; 3]);
    for item in items.iter().flatten().filter(|s| s.count > 0) {
        let angle = partition * count as f32;
        let at =
            crate::game::create_port_mixer::rotate_y([radius, f64::from(item_height), 0.0], angle);
        draw.push();
        if fluid_height > 0.0 {
            let phase = (draw.game_time as f32 + draw.partial) / 12.0 + angle;
            let bob = (crate::game::create_port_mixer::sin(phase) + 1.5) / 32.0;
            draw.translate([0.0, bob, 0.0]);
        }
        draw.translate(at.map(|v| v as f32));
        draw.rotate_about([0.0, 1.0, 0.0], angle + 35.0, [0.0; 3]);
        draw.rotate_about([1.0, 0.0, 0.0], 65.0, [0.0; 3]);
        for _ in 0..=item.count / 8 {
            draw.push();
            draw.translate(std::array::from_fn(|_| {
                (random.next_float() - 0.5) * 2.0 / 16.0
            }));
            game.native_draw_item(
                draw,
                item,
                horizon_assets::item_model::DisplayContext::Ground,
                None,
                0,
            );
            draw.pop();
        }
        draw.pop();
        count -= 1;
    }
    draw.pop();
    true
}
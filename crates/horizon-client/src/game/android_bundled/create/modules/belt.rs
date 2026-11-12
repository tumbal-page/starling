// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create 6.0.10 BeltConnectorItem/BeltBlock/BeltBlockEntity/BeltInventory adapter.
// The portable transport module owns geometry and movement. This adapter owns
// normal world writes, component-preserving NBT, controller lifetime and drawing.
// Casings/dyes and passengers remain separate integrations. Processor callbacks
// use the portable transport result/queue contract.
use super::{Game, create_engine};
use crate::game::native_block_entity_draw::BlockEntityDraw;
use horizon_client::{
    containers::{BlockEntity, ModBlockEntity},
    items::ItemStack,
};
use super::create_engine::{
    fan::{Processing, ProcessingType},
    kinetics::{Axis, Pos},
    placement::Direction,
    transport::{
        self, Belt, Chain, Ending, Hooks, Inventory, Part, Pulley, Slope, Space, Stack, Transported,
    },
};
use horizon_assets::item_model::DisplayContext;
use horizon_core::{
    geometry::Direction as WorldDirection,
    random::{LegacyRandom, RandomSource},
};
use horizon_world::{
    nbt::{Compound, Tag},
    registry::BlockStateId,
};
use std::{cell::RefCell, collections::BTreeMap};

const ID: &str = "create:belt";
const CONNECTOR: &str = "create:belt_connector";
const FIRST: &str = "create:belt_first_shaft";
type Moving = Transported<ItemStack>;
type Contents = Inventory<ItemStack>;

#[derive(Clone)]
struct KnownChain {
    chain: Chain,
    inventory: Contents,
    rpm: f32,
}
#[derive(Clone)]
struct ProcessingOwner {
    // Source live records keep toRemove identities and toInsert outputs apart
    // until the controller ticks or its serialization boundary is reached.
    inventory: Contents,
    committed: Option<Tag>,
}
#[derive(Default)]
struct State {
    checked: Option<i64>,
    chains: BTreeMap<Pos, KnownChain>,
    processing: BTreeMap<Pos, ProcessingOwner>,
}
thread_local! { static STATE: RefCell<transport::SessionCache<State>> = RefCell::new(Default::default()); }
// TransportedItemStack owns a private java.util.Random, independent of level
// randomness. Keep that separation so item orientation never consumes world RNG.
thread_local! {
    static ITEM_RANDOM: RefCell<LegacyRandom> = RefCell::new(LegacyRandom::new(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos() as i64)
    ));
}
fn simulates(game: &Game<'_>) -> bool {
    game.serving.is_some() || game.options.dedicated.is_some()
}
fn with_state<R>(game: &Game<'_>, f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|cell| {
        let mut cache = cell.borrow_mut();
        // swap_level exchanges dimension contents in place. The address alone
        // identifies this Game's session, reset(false) ends it explicitly.
        let state = cache.dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id());
        f(state)
    })
}
pub(super) fn reset(reload: bool) {
    if !reload {
        STATE.with(|s| s.borrow_mut().clear());
    }
}
fn int(nbt: &Compound, key: &str) -> i32 {
    super::create_port::integer(nbt, key)
}
fn number(nbt: &Compound, key: &str) -> f32 {
    super::create_port::number(nbt, key)
}
fn boolean(nbt: &Compound, key: &str) -> bool {
    int(nbt, key) != 0
}
fn pos_tag(nbt: &Compound, key: &str) -> Option<Pos> {
    match nbt.get(&key.into()) {
        Some(Tag::IntArray(v)) if v.len() == 3 => Some([v[0], v[1], v[2]]),
        _ => None,
    }
}
fn direction(name: &str) -> Direction {
    Direction::ALL
        .into_iter()
        .find(|d| d.name() == name)
        .unwrap_or(Direction::North)
}
fn facing(game: &Game<'_>, pos: Pos) -> Direction {
    direction(
        game.native_block_property(pos, "facing")
            .as_deref()
            .unwrap_or("north"),
    )
}
fn slope_name(slope: Slope) -> &'static str {
    match slope {
        Slope::Horizontal => "horizontal",
        Slope::Upward => "upward",
        Slope::Downward => "downward",
        Slope::Vertical => "vertical",
        Slope::Sideways => "sideways",
    }
}
fn slope(game: &Game<'_>, pos: Pos) -> Slope {
    match game.native_block_property(pos, "slope").as_deref() {
        Some("upward") => Slope::Upward,
        Some("downward") => Slope::Downward,
        Some("vertical") => Slope::Vertical,
        Some("sideways") => Slope::Sideways,
        _ => Slope::Horizontal,
    }
}
fn part_name(part: Part) -> &'static str {
    match part {
        Part::Start => "start",
        Part::Middle => "middle",
        Part::End => "end",
        Part::Pulley => "pulley",
    }
}
fn part(game: &Game<'_>, pos: Pos) -> Part {
    match game.native_block_property(pos, "part").as_deref() {
        Some("start") => Part::Start,
        Some("end") => Part::End,
        Some("pulley") => Part::Pulley,
        _ => Part::Middle,
    }
}
fn axis(game: &Game<'_>, pos: Pos) -> Axis {
    if game.native_block(pos) == ID {
        if slope(game, pos) == Slope::Sideways {
            Axis::Y
        } else if facing(game, pos).axis() == Axis::X {
            Axis::Z
        } else {
            Axis::X
        }
    } else {
        match game.native_block_property(pos, "axis").as_deref() {
            Some("x") => Axis::X,
            Some("z") => Axis::Z,
            _ => Axis::Y,
        }
    }
}
fn world_direction(d: Direction) -> WorldDirection {
    match d {
        Direction::Down => WorldDirection::Down,
        Direction::Up => WorldDirection::Up,
        Direction::North => WorldDirection::North,
        Direction::South => WorldDirection::South,
        Direction::West => WorldDirection::West,
        Direction::East => WorldDirection::East,
    }
}
fn at(origin: Pos, facing: Direction, slope: Slope, index: i32) -> Pos {
    let v = if slope == Slope::Vertical {
        [
            0,
            if facing.vector().iter().any(|v| *v < 0) {
                -1
            } else {
                1
            },
            0,
        ]
    } else {
        let mut v = facing.vector();
        v[1] = match slope {
            Slope::Upward => 1,
            Slope::Downward => -1,
            _ => 0,
        };
        v
    };
    std::array::from_fn(|i| origin[i] + v[i] * index)
}
fn next(game: &Game<'_>, pos: Pos, forward: bool) -> Option<Pos> {
    if matches!(
        (part(game, pos), forward),
        (Part::Start, false) | (Part::End, true)
    ) {
        return None;
    }
    Some(at(
        pos,
        facing(game, pos),
        slope(game, pos),
        if forward { 1 } else { -1 },
    ))
}
fn metadata(e: &mut ModBlockEntity, controller: Pos, length: usize, index: usize) {
    e.nbt
        .insert("Controller".into(), Tag::IntArray(controller.to_vec()));
    e.nbt
        .insert("IsController".into(), Tag::Byte(i8::from(index == 0)));
    e.nbt.insert("Length".into(), Tag::Int(length as i32));
    e.nbt.insert("Index".into(), Tag::Int(index as i32));
    e.nbt
        .entry("Casing".into())
        .or_insert(Tag::String("NONE".into()));
    e.nbt.entry("Covered".into()).or_insert(Tag::Byte(0));
}
pub(super) fn node_data(game: &Game<'_>, pos: Pos) -> Option<(Pos, i32)> {
    if let Some(BlockEntity::Mod(e)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        return Some((
            pos_tag(&e.nbt, "Controller").unwrap_or(pos),
            int(&e.nbt, "Index"),
        ));
    }
    with_state(game, |s| {
        s.chains.iter().find_map(|(controller, known)| {
            known
                .chain
                .segments
                .iter()
                .position(|p| p.pos == pos)
                .map(|i| (*controller, i as i32))
        })
    })
}
fn belt(game: &Game<'_>, pos: Pos, e: &ModBlockEntity, rpm: f32) -> Belt {
    Belt {
        rpm,
        facing: facing(game, pos),
        slope: slope(game, pos),
        length: int(&e.nbt, "Length"),
    }
}
pub(super) fn game_stack(stack: &Stack<ItemStack>) -> ItemStack {
    ItemStack {
        count: stack.count,
        ..stack.payload.clone()
    }
}
pub(super) fn core_stack(game: &Game<'_>, stack: ItemStack) -> Stack<ItemStack> {
    let maximum = super::create_port::stack_maximum(game, &stack);
    Stack {
        count: stack.count,
        maximum,
        payload: ItemStack { count: 1, ..stack },
    }
}
fn return_to_player(game: &mut Game<'_>, pos: Pos, stack: ItemStack) {
    let player = game.native_acting_id();
    if let Some(rest) = game.native_give_stack(player, stack) {
        // Inventory.placeItemBackInInventory -> Player.drop(stack, false).
        if let Some(eye) = game.eye_position() {
            game.throw_from_eyes(rest, eye, false);
        } else {
            game.spawn_item(rest, pos.map(|v| f64::from(v) + 0.5), [0.0, 0.2, 0.0], 10);
        }
    }
}
fn read_item(game: &Game<'_>, tag: &Tag) -> Option<Moving> {
    let Tag::Compound(nbt) = tag else { return None };
    let stack = horizon_items::item_nbt::item_from_nbt(nbt.get(&"Item".into())?)?;
    let mut item = Moving::new(core_stack(game, stack), int(nbt, "Angle"));
    item.position = number(nbt, "Pos");
    item.previous_position = number(nbt, "PrevPos");
    item.side_offset = number(nbt, "Offset");
    item.previous_side_offset = number(nbt, "PrevOffset");
    item.inserted_at = int(nbt, "InSegment");
    item.inserted_from = Direction::ALL[int(nbt, "InDirection").rem_euclid(6) as usize];
    item.locked = boolean(nbt, "Locked");
    item.externally_locked = boolean(nbt, "LockedExternally");
    item.fan = Processing {
        kind: match nbt.get(&"FanProcessingType".into()) {
            Some(Tag::String(s)) => ProcessingType::parse(&s.to_string_lossy()),
            _ => None,
        },
        time: int(nbt, "FanProcessingTime"),
    };
    (item.position.is_finite()
        && item.previous_position.is_finite()
        && item.side_offset.is_finite()
        && item.previous_side_offset.is_finite())
    .then_some(item)
}
fn write_item(item: &Moving) -> Tag {
    let mut nbt = Compound::from([
        (
            "Item".into(),
            horizon_items::item_nbt::item_to_nbt(&game_stack(&item.stack)),
        ),
        ("Pos".into(), Tag::Float(item.position.to_bits())),
        (
            "PrevPos".into(),
            Tag::Float(item.previous_position.to_bits()),
        ),
        ("Offset".into(), Tag::Float(item.side_offset.to_bits())),
        (
            "PrevOffset".into(),
            Tag::Float(item.previous_side_offset.to_bits()),
        ),
        ("InSegment".into(), Tag::Int(item.inserted_at)),
        ("Angle".into(), Tag::Int(item.angle)),
        (
            "InDirection".into(),
            Tag::Int(
                Direction::ALL
                    .iter()
                    .position(|d| *d == item.inserted_from)
                    .unwrap_or(1) as i32,
            ),
        ),
    ]);
    if let Some(kind) = item.fan.kind {
        nbt.insert("FanProcessingType".into(), Tag::String(kind.id().into()));
        nbt.insert("FanProcessingTime".into(), Tag::Int(item.fan.time));
    }
    if item.locked {
        nbt.insert("Locked".into(), Tag::Byte(1));
    }
    if item.externally_locked {
        nbt.insert("LockedExternally".into(), Tag::Byte(1));
    }
    Tag::Compound(nbt)
}
fn read_inventory(game: &Game<'_>, e: &ModBlockEntity) -> Contents {
    let Some(Tag::Compound(nbt)) = e.nbt.get(&"Inventory".into()) else {
        return Contents::default();
    };
    let items = match nbt.get(&"Items".into()) {
        Some(Tag::List(items)) => items.iter().filter_map(|i| read_item(game, i)).collect(),
        _ => Vec::new(),
    };
    Contents {
        items,
        pending: Vec::new(),
        positive: boolean(nbt, "PositiveOrder"),
    }
}
fn write_inventory(e: &mut ModBlockEntity, inventory: &mut Contents) {
    // BeltInventory.write flushes insertion/removal queues before serialization.
    inventory.flush();
    let tag = Tag::Compound(Compound::from([
        (
            "Items".into(),
            Tag::List(inventory.items.iter().map(write_item).collect()),
        ),
        (
            "PositiveOrder".into(),
            Tag::Byte(i8::from(inventory.positive)),
        ),
    ]));
    if e.nbt.get(&"Inventory".into()) != Some(&tag) {
        e.nbt.insert("Inventory".into(), tag);
    }
}
fn remember(game: &Game<'_>, chain: Chain, inventory: Contents, rpm: f32) {
    if let Some(controller) = chain.segments.first().map(|first| first.pos) {
        with_state(game, |s| {
            s.chains.insert(
                controller,
                KnownChain {
                    chain,
                    inventory,
                    rpm,
                },
            );
        });
    }
}
fn initialize(game: &mut Game<'_>, pos: Pos, entity: &mut ModBlockEntity) -> bool {
    let mut controller = pos;
    for _ in 0..1000 {
        if game.native_block(controller) != ID {
            return false;
        }
        let Some(previous) = next(game, controller, false) else {
            break;
        };
        if !game.world.loaded(previous[0], previous[2]) {
            return false;
        }
        controller = previous;
    }
    let mut chain = Chain {
        facing: facing(game, controller),
        slope: slope(game, controller),
        segments: Vec::new(),
    };
    let mut current = controller;
    for _ in 0..1000 {
        if !game.world.loaded(current[0], current[2]) || game.native_block(current) != ID {
            return false;
        }
        chain.segments.push(transport::Segment {
            pos: current,
            part: part(game, current),
        });
        let Some(n) = next(game, current, true) else {
            break;
        };
        current = n;
    }
    if chain.segments.len() < 2 || chain.segments.last().is_none_or(|s| s.part != Part::End) {
        return false;
    }
    for (i, segment) in chain.segments.iter().enumerate() {
        if segment.pos == pos {
            metadata(entity, controller, chain.segments.len(), i);
        } else {
            game.native_with_mod_block_entity(segment.pos, |e| {
                metadata(e, controller, chain.segments.len(), i)
            });
        }
        game.native_block_entity_sync(segment.pos);
    }
    let inventory = if pos == controller {
        authoritative_inventory(game, controller, entity)
    } else {
        match game
            .world
            .block_entities
            .get(&(controller[0], controller[1], controller[2]))
        {
            Some(BlockEntity::Mod(e)) => authoritative_inventory(game, controller, e),
            _ => Contents::default(),
        }
    };
    remember(game, chain, inventory, 0.0);
    super::create_port_kinetic_runtime::invalidate();
    true
}
/// Called before the ordinary kinetic solve, while this entity is in hand.
pub(super) fn prepare(game: &mut Game<'_>, pos: Pos, entity: &mut ModBlockEntity) -> bool {
    if entity.id != ID {
        return false;
    }
    let controller = pos_tag(&entity.nbt, "Controller").unwrap_or(pos);
    if int(&entity.nbt, "Length") == 0 || !with_state(game, |s| s.chains.contains_key(&controller))
    {
        initialize(game, pos, entity);
    }
    true
}
fn state(game: &Game<'_>, block: &str, properties: &[(&str, &str)]) -> Option<BlockStateId> {
    let registry = game.pack.assets.registry;
    let default = registry.default_state(block)?;
    let mut descriptor = registry.state(default)?.clone();
    for (key, value) in properties {
        descriptor
            .properties
            .insert((*key).to_owned(), (*value).to_owned());
    }
    registry.id(&descriptor)
}
fn pulley(game: &Game<'_>, pos: Pos) -> Pulley {
    let rpm = super::create_port::node(game, pos)
        .map(|node| super::create_port_kinetic_runtime::network(game, node, 0).speed(pos))
        .unwrap_or(0.0);
    Pulley {
        pos,
        axis: axis(game, pos),
        rpm,
        loaded: game.world.loaded(pos[0], pos[2]),
        shaft: game.native_block(pos) == "create:shaft",
    }
}
pub(super) fn right_click(game: &mut Game<'_>, pos: Pos, _face: Pos) -> bool {
    if !game.native_on("create") {
        return false;
    }
    let held = game.held().cloned();
    let sneaking = game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
    if held.as_ref().is_some_and(|s| s.item.as_ref() == CONNECTOR) {
        if !simulates(game) {
            return true;
        }
        if sneaking {
            if let Some(s) = game.held_mut().as_mut() {
                s.data_mut().set_other(FIRST, None);
            }
            return true;
        }
        if game.native_block(pos) != "create:shaft" {
            return true;
        }
        let first = held
            .as_ref()
            .and_then(|s| s.data())
            .and_then(|d| d.other(FIRST))
            .and_then(|tag| match tag {
                Tag::IntArray(p) if p.len() == 3 => Some([p[0], p[1], p[2]]),
                _ => None,
            });
        let first = first.filter(|p| {
            game.native_block(*p) == "create:shaft"
                && p.iter()
                    .zip(pos)
                    .map(|(a, b)| f64::from(*a - b).powi(2))
                    .sum::<f64>()
                    < 1600.0
        });
        if let Some(first) = first {
            let chain = transport::connect(
                pulley(game, first),
                pulley(game, pos),
                transport::DEFAULT_MAX_LENGTH,
                |p| {
                    if game.native_block(p) == "create:shaft" {
                        Space::Shaft(axis(game, p))
                    } else if game
                        .res
                        .behaviours()
                        .is_replaceable(game.world.block(p[0], p[1], p[2]))
                    {
                        Space::Replaceable
                    } else {
                        Space::Blocked
                    }
                },
            );
            let Ok(chain) = chain else { return true };
            let plans = chain
                .segments
                .iter()
                .map(|segment| {
                    let wet = game
                        .native_block_property(segment.pos, "waterlogged")
                        .as_deref()
                        == Some("true")
                        || game.native_block(segment.pos) == "minecraft:water";
                    state(
                        game,
                        ID,
                        &[
                            ("facing", chain.facing.name()),
                            ("slope", slope_name(chain.slope)),
                            ("part", part_name(segment.part)),
                            ("casing", "false"),
                            ("waterlogged", if wet { "true" } else { "false" }),
                        ],
                    )
                    .map(|s| (segment.pos, s))
                })
                .collect::<Option<Vec<_>>>();
            let Some(plans) = plans else { return true };
            let dim = game.dim.id().to_owned();
            for (p, s) in plans {
                if !game.native_set_block(&dim, p, s, 3) {
                    return true;
                }
            }
            for (i, segment) in chain.segments.iter().enumerate() {
                game.native_with_mod_block_entity(segment.pos, |e| {
                    metadata(e, first, chain.segments.len(), i)
                });
                game.native_block_entity_sync(segment.pos);
            }
            let midpoint =
                std::array::from_fn(|i| (f64::from(first[i]) + f64::from(pos[i])) * 0.5 + 0.5);
            game.with_level(|_, level, _| {
                level.native_play_sound("minecraft:block.wool.place", midpoint, 4, 0.5, 1.0)
            });
            remember(game, chain, Contents::default(), 0.0);
            super::create_port_kinetic_runtime::invalidate();
            let creative = game.creative;
            if let Some(s) = game.held_mut().as_mut() {
                s.data_mut().set_other(FIRST, None);
                if !creative {
                    s.count = s.count.saturating_sub(1);
                }
            }
            if game.held().is_some_and(|s| s.count == 0) {
                game.held_mut().take();
            }
        } else if let Some(s) = game.held_mut().as_mut() {
            s.data_mut()
                .set_other(FIRST, Some(Tag::IntArray(pos.to_vec())));
        }
        game.add_cooldown(CONNECTOR.into(), 5);
        return true;
    }
    if game.native_block(pos) != ID || sneaking {
        return false;
    }
    if held.is_none() {
        let Some((controller, index)) = node_data(game, pos) else {
            return true;
        };
        let Some(mut inventory) = controller_inventory(game, controller) else {
            return true;
        };
        let mut taken = Vec::new();
        inventory.items.retain(|i| {
            if (index as f32 + 0.5 - i.position).abs() < 0.55 {
                taken.push(game_stack(&i.stack));
                false
            } else {
                true
            }
        });
        save_controller(game, controller, &mut inventory);
        if !taken.is_empty() {
            super::create_port_sounds::pickup(game, pos);
        }
        for stack in taken {
            return_to_player(game, pos, stack);
        }
        return true;
    }
    if held
        .as_ref()
        .is_some_and(|s| s.item.as_ref() == "create:shaft")
        && part(game, pos) == Part::Middle
    {
        game.native_set_block_property(pos, "part", "pulley");
        if !game.creative
            && let Some(s) = game.held_mut().as_mut()
        {
            s.count = s.count.saturating_sub(1);
        }
        super::create_port_kinetic_runtime::invalidate();
        return true;
    }
    if held
        .as_ref()
        .is_some_and(|s| s.item.as_ref() == "create:wrench")
    {
        if part(game, pos) == Part::Pulley {
            game.native_set_block_property(pos, "part", "middle");
            if !game.creative {
                return_to_player(game, pos, ItemStack::new("create:shaft", 1));
            }
            super::create_port_kinetic_runtime::invalidate();
        }
        return true;
    }
    false
}
fn controller_inventory(game: &Game<'_>, controller: Pos) -> Option<Contents> {
    match game
        .world
        .block_entities
        .get(&(controller[0], controller[1], controller[2]))
    {
        Some(BlockEntity::Mod(e)) if e.id == ID => {
            Some(authoritative_inventory(game, controller, e))
        }
        _ => None,
    }
}
fn authoritative_inventory(game: &Game<'_>, controller: Pos, entity: &ModBlockEntity) -> Contents {
    let live = with_state(game, |s| {
        if let Some(owner) = s.processing.get(&controller)
            && owner.committed.as_ref() == entity.nbt.get(&"Inventory".into())
        {
            return Some(owner.inventory.clone());
        }
        // Ordinary data merge/replacement is authoritative over stale cache.
        s.processing.remove(&controller);
        None
    });
    live.unwrap_or_else(|| read_inventory(game, entity))
}
fn save_controller(game: &mut Game<'_>, controller: Pos, inventory: &mut Contents) {
    game.native_with_mod_block_entity(controller, |e| write_inventory(e, inventory));
    with_state(game, |s| {
        s.processing.remove(&controller);
        if let Some(c) = s.chains.get_mut(&controller) {
            c.inventory = inventory.clone();
        }
    });
    game.native_block_entity_sync(controller);
}
fn stage_processing(game: &mut Game<'_>, controller: Pos, inventory: Contents) {
    // Canonical NBT is always save-safe. Its flushed clone is also what an
    // outbound tag must show, without exposing queued outputs to another
    // handler in this simulation pass.
    let mut saved = inventory.clone();
    let committed = game
        .native_with_mod_block_entity(controller, |entity| {
            if entity.id != ID {
                return None;
            }
            write_inventory(entity, &mut saved);
            Some(entity.nbt.get(&"Inventory".into()).cloned())
        })
        .flatten();
    let Some(committed) = committed else { return };
    with_state(game, |s| {
        if let Some(chain) = s.chains.get_mut(&controller) {
            chain.inventory = inventory.clone();
        }
        s.processing.insert(
            controller,
            ProcessingOwner {
                inventory,
                committed,
            },
        );
    });
}
/// The pinned tick_lan HEAD hook calls this before outgoing block-entity tags.
/// LevelTickPost runs after networking and would expose a stale frame instead.
pub(super) fn flush_processing(game: &mut Game<'_>) {
    if !simulates(game) {
        return;
    }
    let pending = with_state(game, |s| std::mem::take(&mut s.processing));
    for (controller, mut owner) in pending {
        if game.native_block(controller) != ID {
            continue;
        }
        let committed = game
            .native_with_mod_block_entity(controller, |entity| {
                if entity.id != ID {
                    return None;
                }
                if owner.committed.as_ref() == entity.nbt.get(&"Inventory".into()) {
                    write_inventory(entity, &mut owner.inventory);
                    Some(true)
                } else {
                    Some(false)
                }
            })
            .flatten();
        let current = match committed {
            Some(true) => Some(owner.inventory),
            Some(false) => controller_inventory(game, controller),
            None => None,
        };
        if let Some(current) = current {
            with_state(game, |s| {
                if let Some(chain) = s.chains.get_mut(&controller) {
                    chain.inventory = current;
                }
            });
        }
    }
}
/// TransportedItemStackHandlerBehaviour.applyToAllItems boundary for fan or
/// other machine adapters. Source results queue conversions/removals; equal
/// output stacks alone do not replace a transported record or its animation.
pub(super) fn apply_to_each_within(
    game: &mut Game<'_>,
    pos: Pos,
    maximum_distance: f32,
    mut apply: impl FnMut(
        &mut Game<'_>,
        &mut Transported<ItemStack>,
    ) -> Option<transport::TransportedResult<ItemStack>>,
) -> usize {
    if game.native_block(pos) != ID || !maximum_distance.is_finite() || maximum_distance <= 0.0 {
        return 0;
    }
    let Some((controller, index)) = node_data(game, pos) else {
        return 0;
    };
    let Some(mut inventory) = controller_inventory(game, controller) else {
        return 0;
    };
    let mut metadata_changed = false;
    let changed = inventory.apply_to_each_within(index as f32 + 0.5, maximum_distance, |item| {
        let before = item.clone();
        let result = apply(game, item);
        metadata_changed |= *item != before;
        result
    });
    if metadata_changed || changed > 0 {
        stage_processing(game, controller, inventory);
    }
    if changed > 0 {
        game.native_block_entity_sync(controller);
    }
    changed
}
pub(super) fn processing_positions(
    game: &Game<'_>,
    pos: Pos,
    maximum_distance: f32,
) -> Vec<[f64; 3]> {
    if game.native_block(pos) != ID || !maximum_distance.is_finite() || maximum_distance <= 0.0 {
        return Vec::new();
    }
    let Some((controller, index)) = node_data(game, pos) else {
        return Vec::new();
    };
    let Some(inventory) = controller_inventory(game, controller) else {
        return Vec::new();
    };
    let Some(BlockEntity::Mod(entity)) =
        game.world
            .block_entities
            .get(&(controller[0], controller[1], controller[2]))
    else {
        return Vec::new();
    };
    let shape = belt(game, controller, entity, 0.0);
    inventory
        .items
        .iter()
        .filter(|item| {
            !item.scheduled_removal
                && ((index as f32 + 0.5) - item.position).abs() < maximum_distance
        })
        .map(|item| shape.world_position(controller, item.position))
        .collect()
}
fn eject(game: &mut Game<'_>, controller: Pos, belt: Belt, item: Moving) {
    let mut at = belt.world_position(controller, item.position);
    let motion = belt.ejection_velocity();
    let length = motion.iter().map(|x| x * x).sum::<f64>().sqrt();
    if length > 0.0 {
        for i in 0..3 {
            at[i] += motion[i] / length * 0.001;
        }
    }
    at[1] += 6.0 / 16.0;
    game.spawn_item(game_stack(&item.stack), at, motion, 10);
}
fn dismantle(game: &mut Game<'_>, controller: Pos, known: KnownChain, removed: Option<Pos>) {
    let b = Belt {
        rpm: known.rpm,
        facing: known.chain.facing,
        slope: known.chain.slope,
        length: known.chain.segments.len() as i32,
    };
    let mut inventory = controller_inventory(game, controller).unwrap_or(known.inventory);
    inventory.flush();
    game.native_with_mod_block_entity(controller, |e| {
        e.nbt.remove(&"Inventory".into());
    });
    for item in inventory.items {
        eject(game, controller, b, item);
    }
    let dim = game.dim.id().to_owned();
    for segment in known.chain.segments {
        if Some(segment.pos) == removed || game.native_block(segment.pos) != ID {
            continue;
        }
        let axis = axis(game, segment.pos);
        let wet = game
            .native_block_property(segment.pos, "waterlogged")
            .as_deref()
            == Some("true");
        let replacement = if part(game, segment.pos) == Part::Middle {
            state(
                game,
                if wet {
                    "minecraft:water"
                } else {
                    "minecraft:air"
                },
                &[],
            )
        } else {
            state(
                game,
                "create:shaft",
                &[
                    (
                        "axis",
                        match axis {
                            Axis::X => "x",
                            Axis::Y => "y",
                            Axis::Z => "z",
                        },
                    ),
                    ("waterlogged", if wet { "true" } else { "false" }),
                ],
            )
        };
        if let Some(replacement) = replacement {
            game.native_set_block(&dim, segment.pos, replacement, 3);
        }
    }
    super::create_port_kinetic_runtime::invalidate();
}
/// Rebuild ownership from saved entities when no tick has populated the cache
/// (a chunk reload, or destruction before that dimension's first belt tick).
/// Do not follow guessed metadata into unloaded chunks or a crossing chain.
fn restored_chain(game: &Game<'_>, pos: Pos) -> Option<(Pos, KnownChain)> {
    let expected_facing = facing(game, pos);
    let expected_slope = slope(game, pos);
    let valid = |p: Pos| {
        game.world.loaded(p[0], p[2])
            && game.native_block(p) == ID
            && facing(game, p) == expected_facing
            && slope(game, p) == expected_slope
    };
    let mut controller = pos;
    let mut start_found = false;
    for _ in 0..1000 {
        if !valid(controller) {
            return None;
        }
        if part(game, controller) == Part::Start {
            start_found = true;
            break;
        }
        controller = at(controller, expected_facing, expected_slope, -1);
    }
    if !start_found {
        return None;
    }
    let mut chain = Chain {
        facing: expected_facing,
        slope: expected_slope,
        segments: Vec::new(),
    };
    for index in 0..1000 {
        let current = at(controller, expected_facing, expected_slope, index);
        if !valid(current) {
            return None;
        }
        let current_part = part(game, current);
        if (index == 0 && current_part != Part::Start)
            || (index != 0 && current_part == Part::Start)
        {
            return None;
        }
        chain.segments.push(transport::Segment {
            pos: current,
            part: current_part,
        });
        if current_part == Part::End {
            break;
        }
    }
    if chain.segments.len() < 2 || chain.segments.last()?.part != Part::End {
        return None;
    }
    for (index, segment) in chain.segments.iter().enumerate() {
        let Some(BlockEntity::Mod(entity)) =
            game.world
                .block_entities
                .get(&(segment.pos[0], segment.pos[1], segment.pos[2]))
        else {
            return None;
        };
        if entity.id != ID {
            return None;
        }
        if int(&entity.nbt, "Length") != 0
            && (int(&entity.nbt, "Length") != chain.segments.len() as i32
                || int(&entity.nbt, "Index") != index as i32
                || pos_tag(&entity.nbt, "Controller") != Some(controller))
        {
            return None;
        }
    }
    let Some(BlockEntity::Mod(entity)) =
        game.world
            .block_entities
            .get(&(controller[0], controller[1], controller[2]))
    else {
        return None;
    };
    Some((
        controller,
        KnownChain {
            inventory: authoritative_inventory(game, controller, entity),
            rpm: number(&entity.nbt, "Speed"),
            chain,
        },
    ))
}
fn eject_controller_without_chain(game: &mut Game<'_>, pos: Pos) {
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    if entity.id != ID
        || (!boolean(&entity.nbt, "IsController")
            && pos_tag(&entity.nbt, "Controller") != Some(pos))
    {
        return;
    }
    let mut inventory = authoritative_inventory(game, pos, entity);
    let b = belt(game, pos, entity, number(&entity.nbt, "Speed"));
    inventory.flush();
    game.native_with_mod_block_entity(pos, |entity| {
        entity.nbt.remove(&"Inventory".into());
    });
    // BeltBlockEntity.destroy ejects its OWN inventory even when neighbours
    // cannot be resolved. Never make item conservation depend on a cache hit
    // or on a different chunk being loaded.
    for item in inventory.items {
        eject(game, pos, b, item);
    }
}
/// Before ordinary player destruction: retain normal clicked-block loot, restore
/// the other pulleys and eject the controller's actual inventory exactly once.
pub(super) fn destroy(game: &mut Game<'_>, pos: Pos) {
    if game.native_block(pos) != ID {
        return;
    }
    let found = with_state(game, |s| {
        let controller = s
            .chains
            .iter()
            .find(|(_, v)| v.chain.segments.iter().any(|s| s.pos == pos))
            .map(|(p, _)| *p)?;
        s.chains.remove(&controller).map(|v| (controller, v))
    })
    .or_else(|| restored_chain(game, pos));
    if let Some((controller, known)) = found {
        dismantle(game, controller, known, Some(pos));
    } else {
        eject_controller_without_chain(game, pos);
    }
}
/// Catch non-player removal through normal world changes; never ticket chunks.
pub(super) fn maintenance(game: &mut Game<'_>) {
    if !simulates(game) {
        return;
    }
    let broken = with_state(game, |s| {
        if s.checked == Some(game.world.game_time) {
            return Vec::new();
        }
        s.checked = Some(game.world.game_time);
        s.processing.retain(|controller, _| {
            game.world.loaded(controller[0], controller[2]) && game.native_block(*controller) == ID
        });
        s.chains.retain(|_, c| {
            c.chain
                .segments
                .iter()
                .all(|p| game.world.loaded(p.pos[0], p.pos[2]))
        });
        let broken = s
            .chains
            .iter()
            .filter(|(_, c)| {
                c.chain
                    .segments
                    .iter()
                    .all(|p| game.world.loaded(p.pos[0], p.pos[2]))
                    && c.chain
                        .segments
                        .iter()
                        .any(|p| game.native_block(p.pos) != ID)
            })
            .map(|(p, _)| *p)
            .collect::<Vec<_>>();
        broken
            .into_iter()
            .filter_map(|p| s.chains.remove(&p).map(|c| (p, c)))
            .collect::<Vec<_>>()
    });
    for (controller, known) in broken {
        dismantle(game, controller, known, None);
    }
}

struct WorldHooks<'a, 'g> {
    game: &'a mut Game<'g>,
    origin: Pos,
    belt: Belt,
}
impl WorldHooks<'_, '_> {
    fn target(&self) -> Pos {
        let offset = if self.belt.speed() > 0.0 {
            self.belt.length
        } else {
            -1
        };
        let d = self.belt.facing.vector();
        let y = match self.belt.slope {
            Slope::Upward => 1,
            Slope::Downward => -1,
            _ => 0,
        };
        [
            self.origin[0] + d[0] * offset,
            self.origin[1] + offset.clamp(0, self.belt.length - 1) * y,
            self.origin[2] + d[2] * offset,
        ]
    }
}
impl Hooks<ItemStack> for WorldHooks<'_, '_> {
    fn processor(&mut self, segment: i32) -> Option<bool> {
        let p = at(self.origin, self.belt.facing, self.belt.slope, segment);
        Some(super::create_port_belt_press::available(
            self.game,
            [p[0], p[1] + 2, p[2]],
        ))
    }
    fn processing_blocked(&mut self, segment: i32) -> bool {
        let p = at(self.origin, self.belt.facing, self.belt.slope, segment);
        !self
            .game
            .native_block_collision([p[0], p[1] + 1, p[2]])
            .is_empty()
    }
    fn received_update(
        &mut self,
        item: &mut Moving,
        segment: i32,
    ) -> transport::ProcessingUpdate<ItemStack> {
        let p = at(self.origin, self.belt.facing, self.belt.slope, segment);
        super::create_port_belt_press::received(self.game, [p[0], p[1] + 2, p[2]], item)
    }
    fn held_update(
        &mut self,
        item: &mut Moving,
        segment: i32,
    ) -> transport::ProcessingUpdate<ItemStack> {
        let p = at(self.origin, self.belt.facing, self.belt.slope, segment);
        super::create_port_belt_press::held(self.game, [p[0], p[1] + 2, p[2]], item)
    }
    fn ending(&mut self, _positive: bool) -> Ending {
        let target = self.target();
        if matches!(
            self.game.native_block(target).as_str(),
            ID | "create:depot" | "create:millstone"
        ) {
            return Ending::Insert;
        }
        let state = self.game.world.block(target[0], target[1], target[2]);
        if self
            .game
            .res
            .behaviours()
            .is_face_sturdy(state, world_direction(self.belt.direction().opposite()))
        {
            Ending::Blocked
        } else {
            Ending::Eject
        }
    }
    fn insert_transported_end(&mut self, item: &Moving, from: Direction) -> Stack<ItemStack> {
        let stack = &item.stack;
        let target = self.target();
        if self.game.native_block(target) == ID {
            let Some((controller, index)) = node_data(self.game, target) else {
                return stack.clone();
            };
            let Some(mut contents) = controller_inventory(self.game, controller) else {
                return stack.clone();
            };
            let Some(BlockEntity::Mod(e)) =
                self.game
                    .world
                    .block_entities
                    .get(&(controller[0], controller[1], controller[2]))
            else {
                return stack.clone();
            };
            let b = belt(self.game, controller, e, number(&e.nbt, "Speed"));
            if contents.insert_direct(b, index, item, from, true, false) {
                save_controller(self.game, controller, &mut contents);
                return Stack {
                    count: 0,
                    ..stack.clone()
                };
            }
            stack.clone()
        } else if self.game.native_block(target) == "create:depot" {
            // DepotBehaviour's direct input refuses a held stack OR any output
            // buffer. It preserves the transported record and does not merge.
            let mut inserted = item.source_copy();
            inserted.position = if from.axis() == Axis::Y { 0.5 } else { 0.0 };
            inserted.previous_position = inserted.position;
            inserted.previous_side_offset = inserted.side_offset;
            inserted.inserted_from = from;
            inserted.stack.count = inserted.stack.count.min(inserted.stack.maximum);
            let moved = self
                .game
                .native_with_mod_block_entity(target, |e| {
                    if e.items.is_empty() || e.items.iter().any(Option::is_some) {
                        return false;
                    }
                    e.items[0] = Some(game_stack(&inserted.stack));
                    e.nbt.insert("HeldItem".into(), write_item(&inserted));
                    true
                })
                .unwrap_or(false);
            if moved {
                super::create_port_sounds::depot(self.game, target);
                self.game.native_block_entity_sync(target);
                Stack {
                    count: stack.count - inserted.stack.count,
                    ..stack.clone()
                }
            } else {
                stack.clone()
            }
        } else {
            // MillstoneBlockEntity has the default DirectBeltInputBehaviour.
            let remaining =
                self.game
                    .native_insert_item(target, world_direction(from), game_stack(stack));
            Stack {
                count: remaining.map_or(0, |s| s.count),
                ..stack.clone()
            }
        }
    }
}
pub(super) fn upright(game: &Game<'_>, stack: &ItemStack) -> bool {
    let tags = &game.res.items().item_tags;
    let in_tag = |tag: &str| {
        tags.get(tag)
            .is_some_and(|v| v.contains(stack.item.as_ref()))
    };
    (game.native_item_fluid(stack).is_some() || in_tag("create:upright_on_belt"))
        && !in_tag("create:not_upright_on_belt")
}
pub(super) fn angle(game: &Game<'_>, stack: &ItemStack) -> i32 {
    if upright(game, stack) {
        180
    } else {
        ITEM_RANDOM.with(|random| random.borrow_mut().next_int_bound(360).unwrap_or(0))
    }
}
fn capture(game: &mut Game<'_>, origin: Pos, belt: Belt, contents: &mut Contents) {
    if !belt.slope.transports() {
        return;
    }
    let dim = game.dim.id().to_owned();
    for index in 0..belt.length {
        let p = at(origin, belt.facing, belt.slope, index);
        let min = [f64::from(p[0]), f64::from(p[1]) + 0.5, f64::from(p[2])];
        let max = [min[0] + 1.0, f64::from(p[1]) + 1.25, min[2] + 1.0];
        for dropped in game.native_item_entities(&dim, min, max) {
            let falling = game
                .dropped
                .iter()
                .any(|e| e.id == dropped.id && e.velocity.y <= 0.0);
            if !falling || dropped.position[1] < min[1] {
                continue;
            }
            let input = core_stack(game, dropped.stack.clone());
            let remainder = contents.insert_automation(belt, index, input, 0, true);
            if remainder.count == dropped.stack.count {
                continue;
            }
            let rest = ItemStack {
                count: remainder.count,
                ..dropped.stack.clone()
            };
            if game.native_set_item_entity(&dim, dropped.id, rest) {
                contents.insert_automation(
                    belt,
                    index,
                    core_stack(game, dropped.stack.clone()),
                    angle(game, &dropped.stack),
                    false,
                );
            }
        }
    }
}
/// Called after effective RPM is written, including overstress.
pub(super) fn tick(game: &mut Game<'_>, pos: Pos, entity: &mut ModBlockEntity, rpm: f32) -> bool {
    if entity.id != ID {
        return false;
    }
    maintenance(game);
    if !boolean(&entity.nbt, "IsController") || int(&entity.nbt, "Length") < 2 {
        return false;
    }
    let old_inventory = entity.nbt.get(&"Inventory".into()).cloned();
    let belt = belt(game, pos, entity, rpm);
    let mut contents = authoritative_inventory(game, pos, entity);
    with_state(game, |s| {
        s.processing.remove(&pos);
    });
    if !belt.slope.transports() {
        for item in contents.items.drain(..) {
            eject(game, pos, belt, item);
        }
    } else {
        capture(game, pos, belt, &mut contents);
        let result = contents.tick(
            belt,
            &mut WorldHooks {
                game,
                origin: pos,
                belt,
            },
        );
        for item in result.ejected {
            eject(game, pos, belt, item);
        }
    }
    write_inventory(entity, &mut contents);
    with_state(game, |s| {
        if let Some(c) = s.chains.get_mut(&pos) {
            c.inventory = contents;
            c.rpm = rpm;
        }
    });
    // Controller inventory is not a generic container; send its full source NBT.
    // Client prediction is deliberately separate from server authority.
    let changed = entity.nbt.get(&"Inventory".into()) != old_inventory.as_ref();
    if changed {
        game.native_block_entity_sync(pos);
    }
    changed
}

pub(super) fn models(models: &mut Vec<String>) {
    for part in [
        "belt_pulley",
        "belt/start",
        "belt/middle",
        "belt/end",
        "belt/start_bottom",
        "belt/middle_bottom",
        "belt/end_bottom",
        "belt/diagonal_start",
        "belt/diagonal_middle",
        "belt/diagonal_end",
    ] {
        models.push(format!("create:block/{part}"));
    }
}
/// A rotated cuboid face remains a rectangle in UV space. Reorder its corners
/// cyclically so native_draw_quad can draw exactly the source 0/90/180/270 UVs.
fn rectangle(positions: [[f32; 3]; 4], uv: [[f32; 2]; 4]) -> Option<([[f32; 3]; 4], [f32; 4])> {
    for i in 0..4 {
        let p = [
            positions[i],
            positions[(i + 1) % 4],
            positions[(i + 2) % 4],
            positions[(i + 3) % 4],
        ];
        let t = [uv[i], uv[(i + 1) % 4], uv[(i + 2) % 4], uv[(i + 3) % 4]];
        if (t[0][0] - t[1][0]).abs() < 0.00001
            && (t[1][1] - t[2][1]).abs() < 0.00001
            && (t[2][0] - t[3][0]).abs() < 0.00001
            && (t[3][1] - t[0][1]).abs() < 0.00001
        {
            return Some((
                p,
                [
                    t[0][0] * 16.0,
                    t[0][1] * 16.0,
                    t[2][0] * 16.0,
                    t[2][1] * 16.0,
                ],
            ));
        }
    }
    None
}
fn moving_model(
    game: &mut Game<'_>,
    draw: &mut BlockEntityDraw,
    model: &str,
    scroll: f32,
    diagonal: bool,
    moving: bool,
) {
    if !moving || !super::create_port::original_assets() {
        game.native_draw_model(draw, model, 0xFFFFFF);
        return;
    }
    // create port stub: standalone model lookup not available on host
    let _ = (scroll, diagonal);
    return;
}
pub(super) fn render_bounds(
    game: &Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    bounds: &mut [f32; 6],
) {
    if entity.id != ID || !boolean(&entity.nbt, "IsController") {
        return;
    }
    let end = at(
        pos,
        facing(game, pos),
        slope(game, pos),
        int(&entity.nbt, "Length").saturating_sub(1),
    );
    for i in 0..3 {
        bounds[i] = ((end[i] - pos[i]).min(0) - 1) as f32;
        bounds[i + 3] = ((end[i] - pos[i]).max(0) + 2) as f32;
    }
}

// BeltRenderer uses Catnip AngleHelper.horizontalAngle, which negates the
// vanilla Y angle on the X axis. Using Direction.toYRot here reverses the
// slope of east/west partials and separates adjacent diagonal segments.
fn render_yaw(facing: Direction) -> f32 {
    match facing {
        Direction::South => 0.0,
        Direction::West => -90.0,
        Direction::North => 180.0,
        Direction::East => -270.0,
        _ => 0.0,
    }
}

pub(super) fn draw(
    game: &mut Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) -> bool {
    if entity.id != ID {
        return false;
    }
    let slope = slope(game, pos);
    let facing = facing(game, pos);
    let part = part(game, pos);
    let diagonal = matches!(slope, Slope::Upward | Slope::Downward);
    let downward = slope == Slope::Downward;
    let sideways = slope == Slope::Sideways;
    let along_x = facing.axis() == Axis::X;
    let negative = facing.vector().iter().any(|v| *v < 0);
    let mut start = part == Part::Start;
    let mut end = part == Part::End;
    if downward || slope == Slope::Vertical && !negative {
        std::mem::swap(&mut start, &mut end);
    }
    let rpm = number(&entity.nbt, "Speed");
    let time = draw.game_time as f32 + draw.partial;
    draw.push();
    let yaw = render_yaw(facing);
    draw.rotate_about(
        [0.0, 1.0, 0.0],
        yaw + if slope == Slope::Upward { 180.0 } else { 0.0 } + if sideways { 270.0 } else { 0.0 },
        [0.5; 3],
    );
    if sideways {
        draw.rotate_about([0.0, 0.0, 1.0], 90.0, [0.5; 3]);
    }
    if !diagonal && slope != Slope::Horizontal {
        draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.5; 3]);
    }
    for bottom in [true, false] {
        let model = if diagonal {
            if start {
                "diagonal_start"
            } else if end {
                "diagonal_end"
            } else {
                "diagonal_middle"
            }
        } else if bottom {
            if start {
                "start_bottom"
            } else if end {
                "end_bottom"
            } else {
                "middle_bottom"
            }
        } else if start {
            "start"
        } else if end {
            "end"
        } else {
            "middle"
        };
        let mut speed = rpm;
        if diagonal && (downward ^ along_x)
            || !sideways && !diagonal && along_x
            || sideways && negative
        {
            speed = -speed;
        }
        let scroll = f64::from(speed * (time * if negative { -1.0 } else { 1.0 })) / (31.5 * 16.0)
            + if bottom { 0.5 } else { 0.0 };
        let scroll = (scroll - scroll.floor()) as f32 * if diagonal { 3.0 / 8.0 } else { 0.5 };
        moving_model(
            game,
            draw,
            &format!("create:block/belt/{model}"),
            scroll,
            diagonal,
            rpm != 0.0,
        );
        if diagonal {
            break;
        }
    }
    draw.pop();
    if part != Part::Middle {
        let axis = axis(game, pos);
        draw.push();
        let rotation = super::create_engine::visual::angle(
            f64::from(time),
            rpm,
            super::create_engine::visual::rotation_offset(pos, axis, false),
        );
        draw.rotate_about(
            super::create_engine::visual::positive_axis(axis),
            rotation,
            [0.5; 3],
        );
        if axis == Axis::X {
            draw.rotate_about([0.0, 1.0, 0.0], 90.0, [0.5; 3]);
        }
        if axis == Axis::Y {
            draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.5; 3]);
        }
        draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.5; 3]);
        game.native_draw_model(draw, "create:block/belt_pulley", 0xFFFFFF);
        draw.pop();
    }
    if boolean(&entity.nbt, "IsController") && int(&entity.nbt, "Length") > 0 {
        let b = belt(game, pos, entity, rpm);
        let contents = read_inventory(game, entity);
        for item in &contents.items {
            draw_item(game, draw, pos, b, item);
        }
    }
    true
}
fn draw_item(game: &Game<'_>, draw: &mut BlockEntityDraw, origin: Pos, belt: Belt, item: &Moving) {
    let partial = if belt.rpm == 0.0 { 1.0 } else { draw.partial };
    let offset = item.previous_position + (item.position - item.previous_position) * partial;
    let mut side =
        item.previous_side_offset + (item.side_offset - item.previous_side_offset) * partial;
    let old_light = draw.light;
    let segment = offset.floor() as i32;
    let light_pos = at(origin, belt.facing, belt.slope, segment);
    let at = belt.world_position(origin, offset);
    let (block_light, sky_light) = game.native_light(light_pos);
    draw.light = horizon_client::block_entity_render::vertex_light((sky_light << 4) | block_light);
    let stack = game_stack(&item.stack);
    let upright = upright(game, &stack);
    // Legacy source gui3d is not a data property in modern item model format.
    // This block-registry classification covers the supported original fixtures;
    // custom item models need an explicit isGui3d compatibility contract.
    let block_item = game
        .pack
        .assets
        .registry
        .default_state(&stack.item)
        .is_some();
    let on_slope =
        belt.slope != Slope::Horizontal && offset >= 0.5 && offset <= belt.length as f32 - 0.5;
    let tilt = ((belt.slope == Slope::Downward) ^ belt.facing.vector().iter().any(|v| *v > 0))
        == (belt.facing.axis() == Axis::Z);
    let slope_angle = if on_slope {
        if tilt { -45.0 } else { 45.0 }
    } else {
        0.0
    };
    let axis = if belt.facing.axis() == Axis::X {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    draw.push();
    draw.translate(std::array::from_fn(|i| {
        (at[i] - f64::from(origin[i])) as f32 + if i == 1 { 7.0 / 16.0 } else { 0.0 }
    }));
    if belt.facing.axis() == Axis::X {
        side *= -1.0;
        draw.translate([0.0, 0.0, side]);
    } else {
        draw.translate([side, 0.0, 0.0]);
    }
    if !upright {
        draw.rotate_about(axis, slope_angle, [0.0; 3]);
    }
    if on_slope {
        draw.translate([0.0, 0.125, 0.0]);
    }
    if upright {
        if let Some(camera) = game.native_camera() {
            let yaw = ((at[0] - camera.position[0]).atan2(at[2] - camera.position[2])
                + std::f64::consts::PI)
                .to_degrees() as f32;
            draw.rotate_about([0.0, 1.0, 0.0], yaw, [0.0; 3]);
        }
        draw.translate([0.0, 3.0 / 32.0, 1.0 / 16.0]);
    }
    let near = game.native_player().is_some_and(|p| {
        (p.x - at[0]).powi(2) + (p.y - at[1]).powi(2) + (p.z - at[2]).powi(2) < 256.0
    });
    let count = if near {
        (31 - item.stack.count.max(1).leading_zeros()) / 2
    } else {
        0
    };
    let mut random = LegacyRandom::new(i64::from(item.angle));
    for i in 0..=count {
        draw.push();
        draw.rotate_about([0.0, 1.0, 0.0], item.angle as f32, [0.0; 3]);
        if !block_item && !upright {
            draw.translate([0.0, -0.09375, 0.0]);
            draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.0; 3]);
        }
        if block_item {
            draw.translate([
                random.next_float() * 0.0625 * i as f32,
                0.0,
                random.next_float() * 0.0625 * i as f32,
            ]);
        }
        draw.scale([0.5; 3]);
        game.native_draw_item(draw, &stack, DisplayContext::Fixed, None, item.angle);
        draw.pop();
        if !upright {
            if !block_item {
                draw.rotate_about([0.0, 1.0, 0.0], 10.0, [0.0; 3]);
            }
            draw.translate([0.0, if block_item { 1.0 / 64.0 } else { 1.0 / 16.0 }, 0.0]);
        } else {
            draw.translate([0.0, 0.0, -1.0 / 16.0]);
        }
    }
    draw.pop();
    draw.light = old_light;
}

// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! Create 6.0.10 belt geometry and authoritative transported-item state.
//! Source: BeltConnectorItem, BeltBlockEntity, BeltInventory,
//! ItemHandlerBeltSegment and TransportedItemStack, commit ac0c444d9828da3453ae8cc65338e8de063286fb.
//! World placement, item components, random angles, entities, NBT and rendering
//! stay in adapters. There is deliberately no automatic stack merging: source
//! belts preserve separate transported stacks and reserve insertion spacing.
use crate::{
    fan::Processing,
    kinetics::{Axis, Pos},
    placement::Direction,
};

pub const DEFAULT_MAX_LENGTH: f64 = 20.0;
/// Runtime ownership by world session and dimension. A host may swap several
/// dimensions through the same world object; visiting one must retain the others.
#[derive(Debug)]
pub struct SessionCache<T> {
    session: Option<usize>,
    dimensions: std::collections::BTreeMap<String, T>,
}
impl<T> Default for SessionCache<T> {
    fn default() -> Self {
        Self {
            session: None,
            dimensions: Default::default(),
        }
    }
}
impl<T: Default> SessionCache<T> {
    pub fn dimension(&mut self, session: usize, dimension: &str) -> &mut T {
        if self.session != Some(session) {
            self.session = Some(session);
            self.dimensions.clear();
        }
        self.dimensions.entry(dimension.to_owned()).or_default()
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slope {
    Horizontal,
    Upward,
    Downward,
    Vertical,
    Sideways,
}
impl Slope {
    pub fn transports(self) -> bool {
        !matches!(self, Self::Vertical | Self::Sideways)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Start,
    Middle,
    End,
    Pulley,
}
#[derive(Clone, Copy, Debug)]
pub struct Pulley {
    pub pos: Pos,
    pub axis: Axis,
    pub rpm: f32,
    pub loaded: bool,
    pub shaft: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Replaceable,
    Shaft(Axis),
    Blocked,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub pos: Pos,
    pub part: Part,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    pub slope: Slope,
    pub facing: Direction,
    pub segments: Vec<Segment>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionError {
    Endpoint,
    Distance,
    Alignment,
    Axis,
    OpposingRotation,
    Obstructed,
    Bounds,
}

/// Source connection geometry, including the strict Euclidean length limit.
/// A Y-axis pulley makes a sideways belt: it transmits rotation, not items.
pub fn connect(
    first: Pulley,
    second: Pulley,
    max_length: f64,
    mut space: impl FnMut(Pos) -> Space,
) -> Result<Chain, ConnectionError> {
    if !first.loaded || !second.loaded || !first.shaft || !second.shaft {
        return Err(ConnectionError::Endpoint);
    }
    if !first.rpm.is_finite()
        || !second.rpm.is_finite()
        || !max_length.is_finite()
        || max_length <= 0.0
    {
        return Err(ConnectionError::Bounds);
    }
    let delta = [0, 1, 2].map(|i| i64::from(second.pos[i]) - i64::from(first.pos[i]));
    let distance = delta.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>();
    if distance >= max_length * max_length {
        return Err(ConnectionError::Distance);
    }
    if delta[first.axis.index()] != 0 {
        return Err(ConnectionError::Alignment);
    }
    let [x, y, z] = delta;
    let a = delta.map(i64::abs);
    let sames = u8::from(a[0] == a[1]) + u8::from(a[1] == a[2]) + u8::from(a[2] == a[0]);
    if sames != 1 || first.axis == Axis::Y && x != 0 && z != 0 {
        return Err(ConnectionError::Alignment);
    }
    if first.axis != second.axis {
        return Err(ConnectionError::Axis);
    }
    if first.rpm != 0.0 && second.rpm != 0.0 && first.rpm.signum() != second.rpm.signum() {
        return Err(ConnectionError::OpposingRotation);
    }
    let steps = a.into_iter().max().unwrap_or(0);
    // The original loop has a 1000-step guard. Reject excessive custom-config
    // chains instead of constructing its discontinuous guard-limited result.
    if steps > 1000 {
        return Err(ConnectionError::Bounds);
    }
    let vertical = x == 0 && z == 0;
    let facing = if vertical {
        if first.axis == Axis::X {
            if y > 0 {
                Direction::South
            } else {
                Direction::North
            }
        } else if y > 0 {
            Direction::East
        } else {
            Direction::West
        }
    } else if x != 0 {
        if x > 0 {
            Direction::East
        } else {
            Direction::West
        }
    } else if z > 0 {
        Direction::South
    } else {
        Direction::North
    };
    if first.axis == Axis::Y && vertical {
        return Err(ConnectionError::Alignment);
    }
    let slope = if first.axis == Axis::Y {
        Slope::Sideways
    } else if vertical {
        Slope::Vertical
    } else if y > 0 {
        Slope::Upward
    } else if y < 0 {
        Slope::Downward
    } else {
        Slope::Horizontal
    };
    let step = delta.map(i64::signum);
    let mut segments = Vec::new();
    for i in 0..=steps {
        let coordinates = [0, 1, 2].map(|j| i64::from(first.pos[j]) + step[j] * i);
        let mut pos = [0; 3];
        for j in 0..3 {
            pos[j] = i32::try_from(coordinates[j]).map_err(|_| ConnectionError::Bounds)?;
        }
        let part = if i == 0 {
            Part::Start
        } else if i == steps {
            Part::End
        } else {
            match space(pos) {
                Space::Shaft(axis) if axis == first.axis => Part::Pulley,
                Space::Replaceable => Part::Middle,
                _ => return Err(ConnectionError::Obstructed),
            }
        };
        segments.push(Segment { pos, part });
    }
    Ok(Chain {
        slope,
        facing,
        segments,
    })
}

pub fn movement_speed(rpm: f32) -> f32 {
    rpm / 480.0
}
pub fn direction_aware_speed(rpm: f32, facing: Direction) -> f32 {
    let sign = if facing.vector().iter().any(|v| *v < 0) {
        -1.0
    } else {
        1.0
    };
    movement_speed(rpm) * sign * if facing.axis() == Axis::X { -1.0 } else { 1.0 }
}
pub fn movement_facing(rpm: f32, facing: Direction) -> Direction {
    let negative = (movement_speed(rpm) < 0.0) ^ (facing.axis() == Axis::X);
    match (facing.axis(), negative) {
        (Axis::X, true) => Direction::West,
        (Axis::X, false) => Direction::East,
        (_, true) => Direction::North,
        (_, false) => Direction::South,
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Belt {
    pub rpm: f32,
    pub facing: Direction,
    pub slope: Slope,
    pub length: i32,
}
impl Belt {
    pub fn speed(self) -> f32 {
        direction_aware_speed(self.rpm, self.facing)
    }
    pub fn direction(self) -> Direction {
        movement_facing(self.rpm, self.facing)
    }
    pub fn accepts_direct(self, side: Direction) -> bool {
        self.rpm != 0.0 && self.slope.transports() && self.direction() != side.opposite()
    }
    pub fn world_position(self, origin: Pos, offset: f32) -> [f64; 3] {
        let vertical = match self.slope {
            Slope::Downward => -1.0,
            Slope::Upward => 1.0,
            _ => 0.0,
        };
        let vertical = if offset < 0.5 { 0.0 } else { vertical }
            * (offset.min(self.length as f32 - 0.5) - 0.5);
        let direction = if self.slope == Slope::Vertical {
            [0; 3]
        } else {
            self.facing.vector()
        };
        [
            f64::from(origin[0]) + 0.5 + f64::from(direction[0]) * f64::from(offset - 0.5),
            f64::from(origin[1]) + 0.5 + f64::from(vertical),
            f64::from(origin[2]) + 0.5 + f64::from(direction[2]) * f64::from(offset - 0.5),
        ]
    }
    pub fn ejection_velocity(self) -> [f64; 3] {
        // getMovementDirection returns BlockPos.ZERO before computing slope.
        if self.rpm == 0.0 {
            return [0.0, 0.125, 0.0];
        }
        let direction = self.direction();
        let [x, _, z] = direction.vector();
        let y = if self.slope == Slope::Horizontal {
            0
        } else if self.slope
            == if direction == self.facing {
                Slope::Upward
            } else {
                Slope::Downward
            }
        {
            1
        } else {
            -1
        };
        let speed = f64::from(movement_speed(self.rpm).abs().max(0.125));
        [
            f64::from(x) * speed,
            f64::from(y) * speed + 0.125,
            f64::from(z) * speed,
        ]
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack<T> {
    pub payload: T,
    pub count: u32,
    pub maximum: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Transported<T> {
    pub stack: Stack<T>,
    pub position: f32,
    pub previous_position: f32,
    pub side_offset: f32,
    pub previous_side_offset: f32,
    pub angle: i32,
    pub inserted_at: i32,
    pub inserted_from: Direction,
    pub locked: bool,
    pub externally_locked: bool,
    pub fan: Processing,
    /// Ephemeral source toRemove identity; never part of transported-item NBT.
    pub scheduled_removal: bool,
}

/// TransportedItemStackHandlerBehaviour.TransportedResult. Conversions are
/// queued until the inventory's normal tick/write flush, preserving record
/// identity while several processors inspect one belt in the same cycle.
#[derive(Clone, Debug, PartialEq)]
pub enum TransportedResult<T> {
    DoNothing,
    Convert {
        outputs: Vec<Transported<T>>,
        held: Option<Transported<T>>,
    },
}
impl<T: PartialEq> TransportedResult<T> {
    pub fn unchanged_from(&self, before: &Stack<T>) -> bool {
        match self {
            Self::DoNothing => true,
            Self::Convert { outputs, held } => {
                held.is_none()
                    && outputs.len() == 1
                    && outputs[0].stack.count == before.count
                    && outputs[0].stack.payload == before.payload
            }
        }
    }
}

impl<T: Clone> Transported<T> {
    /// `angle` is supplied by the adapter's source-equivalent random/item rule.
    pub fn new(stack: Stack<T>, angle: i32) -> Self {
        let offset = (angle as f32 - 180.0) / (360.0 * 3.0);
        Self {
            stack,
            position: 0.0,
            previous_position: 0.0,
            side_offset: offset,
            previous_side_offset: offset,
            angle,
            inserted_at: 0,
            inserted_from: Direction::Up,
            locked: false,
            externally_locked: false,
            fan: Processing::default(),
            scheduled_removal: false,
        }
    }
    pub fn target_side_offset(&self) -> f32 {
        (self.angle as f32 - 180.0) / (360.0 * 3.0)
    }
    /// Source getSimilar keeps fan progress and old interpolation positions,
    /// chooses a fresh angle/offset, and clears both transient lock flags.
    pub fn similar(&self, stack: Stack<T>, angle: i32) -> Self {
        let mut copy = Self::new(stack, angle);
        copy.position = self.position;
        copy.previous_position = self.previous_position;
        copy.inserted_at = self.inserted_at;
        copy.inserted_from = self.inserted_from;
        copy.previous_side_offset = self.previous_side_offset;
        copy.fan = self.fan;
        copy
    }
    pub fn source_copy(&self) -> Self {
        let mut copy = self.similar(self.stack.clone(), self.angle);
        copy.side_offset = self.side_offset;
        copy
    }
    pub fn clear_fan_processing(&mut self) {
        self.fan = Processing::default();
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Eject,
    Insert,
    Funnel,
    Blocked,
}
impl Ending {
    fn margin(self) -> f32 {
        match self {
            Self::Eject => 0.0,
            Self::Insert => 0.25,
            Self::Funnel => 0.5,
            Self::Blocked => 0.45,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingResult {
    Pass,
    Hold,
    Remove,
}
/// A processor's flow decision and its separate transported-item conversion.
/// The latter follows the ordinary deferred handler queue, even for HOLD.
pub struct ProcessingUpdate<T> {
    pub decision: ProcessingResult,
    pub conversion: Option<TransportedResult<T>>,
}
impl<T> From<ProcessingResult> for ProcessingUpdate<T> {
    fn from(decision: ProcessingResult) -> Self {
        Self {
            decision,
            conversion: None,
        }
    }
}
/// Optional integration contracts, never substitutes for an unported machine.
pub trait Hooks<T: Clone> {
    fn ending(&mut self, _positive: bool) -> Ending {
        Ending::Eject
    }
    fn insert_end(&mut self, stack: &Stack<T>, _from: Direction) -> Stack<T> {
        stack.clone()
    }
    /// DirectBeltInputBehaviour receives the entire transported record, so a
    /// neighboring belt or depot can retain its angle, components and fan data.
    fn insert_transported_end(&mut self, item: &Transported<T>, from: Direction) -> Stack<T> {
        self.insert_end(&item.stack, from)
    }
    /// None: stack handler unavailable; Some(false): no processor;
    /// Some(true): the original processor + stack handler exist.
    fn processor(&mut self, _segment: i32) -> Option<bool> {
        Some(false)
    }
    fn processing_blocked(&mut self, _segment: i32) -> bool {
        false
    }
    fn received(&mut self, _item: &mut Transported<T>, _segment: i32) -> ProcessingResult {
        ProcessingResult::Pass
    }
    fn held(&mut self, _item: &mut Transported<T>, _segment: i32) -> ProcessingResult {
        ProcessingResult::Pass
    }
    fn received_update(&mut self, item: &mut Transported<T>, segment: i32) -> ProcessingUpdate<T> {
        self.received(item, segment).into()
    }
    fn held_update(&mut self, item: &mut Transported<T>, segment: i32) -> ProcessingUpdate<T> {
        self.held(item, segment).into()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct TickResult<T> {
    pub ejected: Vec<Transported<T>>,
    pub transferred: u32,
    pub removed_processing: usize,
}
impl<T> Default for TickResult<T> {
    fn default() -> Self {
        Self {
            ejected: Vec::new(),
            transferred: 0,
            removed_processing: 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Inventory<T> {
    pub items: Vec<Transported<T>>,
    pub pending: Vec<Transported<T>>,
    pub positive: bool,
}
impl<T> Default for Inventory<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            pending: Vec::new(),
            positive: false,
        }
    }
}
impl<T: Clone + PartialEq> Inventory<T> {
    /// BeltInventory.applyToEachWithin; the boundary is strict and excludes
    /// records already queued for removal. Callback mutations survive even
    /// when its result does nothing, as fan processing progress does in Java.
    pub fn apply_to_each_within(
        &mut self,
        position: f32,
        maximum_distance: f32,
        mut apply: impl FnMut(&mut Transported<T>) -> Option<TransportedResult<T>>,
    ) -> usize {
        let mut changed = 0;
        for item in &mut self.items {
            if item.scheduled_removal || (position - item.position).abs() >= maximum_distance {
                continue;
            }
            let before = item.stack.clone();
            let Some(result) = apply(item) else { continue };
            changed += usize::from(queue_conversion(
                item,
                &before,
                result,
                position,
                self.positive,
                &mut self.pending,
            ));
        }
        changed
    }

    pub fn flush(&mut self) {
        for new in self.pending.drain(..) {
            let index = self
                .items
                .iter()
                .position(|old| (old.position < new.position) == self.positive)
                .unwrap_or(self.items.len());
            self.items.insert(index, new);
        }
        self.items.retain(|item| !item.scheduled_removal);
    }
    pub fn can_insert(&self, belt: Belt, segment: i32, side: Direction) -> bool {
        let facing = belt.direction();
        if facing == side.opposite() {
            return false;
        }
        let segment_pos = segment as f32
            + if facing != side {
                0.5
            } else if !self.positive {
                1.0
            } else {
                0.0
            };
        !self.items.iter().chain(&self.pending).any(|stack| {
            stack.inserted_at == segment
                && stack.inserted_from == side
                && if self.positive {
                    stack.position <= segment_pos + 1.0
                } else {
                    stack.position >= segment_pos - 1.0
                }
        })
    }
    pub fn stack_at(&self, offset: i32) -> Option<usize> {
        self.items.iter().position(|item| {
            !item.scheduled_removal
                && item.stack.count > 0
                && item.position > offset as f32
                && item.position <= offset as f32 + 1.0
        })
    }
    /// IItemHandler's top input also works while stopped, as the source does.
    pub fn insert_automation(
        &mut self,
        belt: Belt,
        segment: i32,
        stack: Stack<T>,
        angle: i32,
        simulate: bool,
    ) -> Stack<T> {
        if !belt.slope.transports() || !self.can_insert(belt, segment, Direction::Up) {
            return stack;
        }
        let accepted = stack.count.min(stack.maximum);
        let mut remainder = stack.clone();
        remainder.count -= accepted;
        if !simulate && accepted > 0 {
            let mut item = Transported::new(
                Stack {
                    count: accepted,
                    ..stack
                },
                angle,
            );
            item.inserted_at = segment;
            item.position = segment as f32 + 0.5 + if self.positive { -1.0 } else { 1.0 } / 16.0;
            item.previous_position = item.position;
            self.pending.push(item);
        }
        remainder
    }
    /// DirectBeltInput accepts a whole transported stack, preserving components.
    pub fn insert_direct(
        &mut self,
        belt: Belt,
        segment: i32,
        item: &Transported<T>,
        side: Direction,
        from_previous_belt: bool,
        simulate: bool,
    ) -> bool {
        if !belt.accepts_direct(side) || !self.can_insert(belt, segment, side) {
            return false;
        }
        if simulate {
            return true;
        }
        let mut item = item.source_copy();
        item.position = segment as f32 + 0.5 - belt.speed().signum() / 16.0;
        if side.axis() != Axis::Y {
            if belt.direction() != side {
                let sign = if side.vector().iter().any(|v| *v < 0) {
                    -1.0
                } else {
                    1.0
                };
                item.side_offset = sign * 0.675 * if side.axis() == Axis::X { -1.0 } else { 1.0 };
            } else {
                let extra = if item.previous_position != 0.0 && from_previous_belt {
                    0.26
                } else {
                    0.0
                };
                item.position = if belt.speed() > 0.0 {
                    segment as f32 - extra
                } else {
                    segment as f32 + 1.0 + extra
                };
            }
        }
        item.previous_side_offset = item.side_offset;
        item.inserted_at = segment;
        item.inserted_from = side;
        item.previous_position = item.position;
        self.pending.push(item);
        true
    }
    pub fn extract(&mut self, segment: i32, amount: u32, simulate: bool) -> Option<Stack<T>> {
        let index = self.stack_at(segment)?;
        let item = &mut self.items[index];
        let count = amount.min(item.stack.count);
        let extracted = Stack {
            count,
            ..item.stack.clone()
        };
        if !simulate {
            item.stack.count -= count;
            if item.stack.count == 0 {
                item.scheduled_removal = true;
            }
        }
        Some(extracted)
    }
    pub fn tick(&mut self, belt: Belt, hooks: &mut impl Hooks<T>) -> TickResult<T> {
        self.flush();
        let mut result = TickResult::default();
        if belt.rpm == 0.0 || !belt.slope.transports() {
            return result;
        }
        let speed = belt.speed();
        if self.positive != (speed > 0.0) {
            self.positive = !self.positive;
            self.items.reverse();
        }
        let mut front: Option<f32> = None;
        let mut ending = None;
        let mut i = 0;
        while i < self.items.len() {
            let item = &mut self.items[i];
            item.previous_position = item.position;
            item.previous_side_offset = item.side_offset;
            if item.stack.count == 0 {
                self.items.remove(i);
                front = None;
                continue;
            }
            if item.externally_locked {
                item.externally_locked = false;
                front = Some(item.position);
                i += 1;
                continue;
            }
            let mut movement = speed;
            let mut no_movement = false;
            if let Some(position) = front {
                let diff = position - item.position;
                if diff.abs() <= 1.0 {
                    no_movement = true;
                }
                movement = if self.positive {
                    movement.min(diff - 1.0)
                } else {
                    movement.max(diff + 1.0)
                };
            }
            let mut to_end = if self.positive {
                belt.length as f32 - item.position
            } else {
                -item.position
            };
            if to_end.abs() < movement.abs() + 1.0 {
                let margin = ending
                    .get_or_insert_with(|| hooks.ending(self.positive))
                    .margin();
                to_end += if self.positive { -margin } else { margin };
            }
            let limited = if self.positive {
                movement.min(to_end)
            } else {
                movement.max(to_end)
            };
            let next = item.position + limited;
            if belt.slope == Slope::Horizontal {
                let removed = process(
                    item,
                    next,
                    no_movement,
                    self.positive,
                    hooks,
                    &mut self.pending,
                );
                if removed {
                    front = Some(item.position);
                    self.items.remove(i);
                    result.removed_processing += 1;
                    continue;
                }
                if item.locked {
                    front = Some(item.position);
                    i += 1;
                    continue;
                }
            }
            if no_movement {
                front = Some(item.position);
                i += 1;
                continue;
            }
            item.position += limited;
            let diff = item.target_side_offset() - item.side_offset;
            item.side_offset += (diff * limited.abs() * 6.0).clamp(-diff.abs(), diff.abs());
            front = Some(item.position);
            if limited == movement {
                i += 1;
                continue;
            }
            match ending {
                Some(Ending::Insert) => {
                    let remainder = hooks.insert_transported_end(item, belt.direction());
                    if remainder != item.stack {
                        result.transferred += item.stack.count.saturating_sub(remainder.count);
                        item.stack = remainder;
                    }
                    if item.stack.count == 0 {
                        self.items.remove(i);
                    } else {
                        i += 1;
                    }
                }
                Some(Ending::Eject) => {
                    result.ejected.push(self.items.remove(i));
                }
                Some(Ending::Blocked | Ending::Funnel) | None => {
                    i += 1;
                }
            }
        }
        result
    }
}
fn queue_conversion<T: PartialEq>(
    item: &mut Transported<T>,
    before: &Stack<T>,
    result: TransportedResult<T>,
    position: f32,
    positive: bool,
    pending: &mut Vec<Transported<T>>,
) -> bool {
    if result.unchanged_from(before) {
        return false;
    }
    if let TransportedResult::Convert { outputs, held } = result {
        if let Some(mut held) = held {
            held.position =
                position as i32 as f32 + 0.5 - if positive { 1.0 / 512.0 } else { -1.0 / 512.0 };
            pending.push(held);
        }
        pending.extend(outputs);
        item.scheduled_removal = true;
        return true;
    }
    false
}

fn process<T: Clone + PartialEq>(
    item: &mut Transported<T>,
    next: f32,
    no_movement: bool,
    positive: bool,
    hooks: &mut impl Hooks<T>,
    pending: &mut Vec<Transported<T>>,
) -> bool {
    let segment = item.position as i32;
    if item.locked {
        match hooks.processor(segment) {
            None => return false,
            Some(false) => item.locked = false,
            Some(true) => {
                let before = item.stack.clone();
                let update = hooks.held_update(item, segment);
                if let Some(result) = update.conversion {
                    queue_conversion(
                        item,
                        &before,
                        result,
                        segment as f32 + 0.5,
                        positive,
                        pending,
                    );
                }
                match update.decision {
                    ProcessingResult::Remove => return true,
                    ProcessingResult::Hold => return false,
                    ProcessingResult::Pass => item.locked = false,
                }
            }
        }
        return false;
    }
    if no_movement {
        return false;
    }
    if item.position > 0.5 || positive {
        let mut segment = (item.position + if positive { 0.5 } else { -0.5 }) as i32;
        let step = if positive { 1 } else { -1 };
        while if positive {
            segment as f32 + 0.5 <= next
        } else {
            segment as f32 + 0.5 >= next
        } {
            if hooks.processor(segment) == Some(true) && !hooks.processing_blocked(segment) {
                let before = item.stack.clone();
                let update = hooks.received_update(item, segment);
                if let Some(result) = update.conversion {
                    queue_conversion(
                        item,
                        &before,
                        result,
                        segment as f32 + 0.5,
                        positive,
                        pending,
                    );
                }
                match update.decision {
                    ProcessingResult::Remove => return true,
                    ProcessingResult::Hold => {
                        item.position = segment as f32
                            + 0.5
                            + if positive { 1.0 / 512.0 } else { -1.0 / 512.0 };
                        item.locked = true;
                        return false;
                    }
                    ProcessingResult::Pass => {}
                }
            }
            segment += step;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pulley(pos: Pos, axis: Axis) -> Pulley {
        Pulley {
            pos,
            axis,
            rpm: 16.0,
            loaded: true,
            shaft: true,
        }
    }
    fn belt(rpm: f32) -> Belt {
        Belt {
            rpm,
            facing: Direction::South,
            slope: Slope::Horizontal,
            length: 5,
        }
    }
    fn item(position: f32, count: u32) -> Transported<&'static str> {
        let mut i = Transported::new(
            Stack {
                payload: "components-key",
                count,
                maximum: 64,
            },
            180,
        );
        i.position = position;
        i
    }
    struct Effects {
        ending: Ending,
        room: u32,
        processor: i32,
        hold: bool,
    }
    impl Hooks<&'static str> for Effects {
        fn ending(&mut self, _: bool) -> Ending {
            self.ending
        }
        fn insert_end(&mut self, s: &Stack<&'static str>, _: Direction) -> Stack<&'static str> {
            let accepted = s.count.min(self.room);
            self.room -= accepted;
            Stack {
                count: s.count - accepted,
                ..s.clone()
            }
        }
        fn processor(&mut self, segment: i32) -> Option<bool> {
            Some(segment == self.processor)
        }
        fn received(&mut self, _: &mut Transported<&'static str>, _: i32) -> ProcessingResult {
            if self.hold {
                ProcessingResult::Hold
            } else {
                ProcessingResult::Pass
            }
        }
        fn held(&mut self, _: &mut Transported<&'static str>, _: i32) -> ProcessingResult {
            if self.hold {
                ProcessingResult::Hold
            } else {
                ProcessingResult::Pass
            }
        }
    }
    fn effects(ending: Ending) -> Effects {
        Effects {
            ending,
            room: 0,
            processor: -100,
            hold: false,
        }
    }
    #[test]
    fn dimension_cache_retains_identical_coordinates_until_session_reset() {
        let mut cache: SessionCache<std::collections::BTreeMap<Pos, u32>> = SessionCache::default();
        cache
            .dimension(7, "minecraft:overworld")
            .insert([0, 64, 0], 11);
        cache
            .dimension(7, "minecraft:the_nether")
            .insert([0, 64, 0], 29);
        assert_eq!(
            cache.dimension(7, "minecraft:overworld").get(&[0, 64, 0]),
            Some(&11)
        );
        assert_eq!(
            cache.dimension(7, "minecraft:the_nether").get(&[0, 64, 0]),
            Some(&29)
        );
        assert!(cache.dimension(8, "minecraft:overworld").is_empty());
        assert!(cache.dimension(8, "minecraft:the_nether").is_empty());
        cache
            .dimension(8, "minecraft:overworld")
            .insert([0, 64, 0], 17);
        cache.clear();
        assert!(cache.dimension(8, "minecraft:overworld").is_empty());
    }
    #[test]
    fn stopped_dismantling_ejects_vertically_for_every_facing_and_slope() {
        for facing in [
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            for slope in [
                Slope::Horizontal,
                Slope::Upward,
                Slope::Downward,
                Slope::Vertical,
                Slope::Sideways,
            ] {
                let b = Belt {
                    rpm: 0.0,
                    facing,
                    slope,
                    length: 6,
                };
                assert_eq!(b.ejection_velocity(), [0.0, 0.125, 0.0]);
                assert_eq!(
                    Belt { rpm: -0.0, ..b }.ejection_velocity(),
                    [0.0, 0.125, 0.0]
                );
            }
        }
    }
    #[test]
    fn direct_chain_handoff_retains_complete_transport_state_and_conserves_count() {
        struct NextBelt {
            target: Inventory<&'static str>,
            belt: Belt,
        }
        impl Hooks<&'static str> for NextBelt {
            fn ending(&mut self, _: bool) -> Ending {
                Ending::Insert
            }
            fn insert_transported_end(
                &mut self,
                item: &Transported<&'static str>,
                from: Direction,
            ) -> Stack<&'static str> {
                let accepted = self
                    .target
                    .insert_direct(self.belt, 0, item, from, true, false);
                Stack {
                    count: if accepted { 0 } else { item.stack.count },
                    ..item.stack.clone()
                }
            }
        }
        let b = Belt {
            length: 2,
            ..belt(256.0)
        };
        let mut moving = item(1.6, 7);
        moving.angle = 37;
        moving.side_offset = -0.125;
        moving.fan = Processing {
            kind: Some(crate::fan::ProcessingType::Splashing),
            time: 39,
        };
        let mut source = Inventory {
            items: vec![moving],
            positive: true,
            ..Inventory::default()
        };
        let mut next = NextBelt {
            target: Inventory {
                positive: true,
                ..Inventory::default()
            },
            belt: b,
        };
        let result = source.tick(b, &mut next);
        assert_eq!(result.transferred, 7);
        assert!(source.items.is_empty());
        assert_eq!(next.target.pending.len(), 1);
        let arrived = &next.target.pending[0];
        assert_eq!(arrived.stack.count, 7);
        assert_eq!(arrived.stack.payload, "components-key");
        assert_eq!(arrived.angle, 37);
        assert_eq!(
            arrived.fan.kind,
            Some(crate::fan::ProcessingType::Splashing)
        );
        assert_eq!(arrived.fan.time, 39);
        assert_eq!(arrived.position, -0.26);
        assert_eq!(arrived.inserted_from, Direction::South);
        assert!(!arrived.locked && !arrived.externally_locked);
    }
    #[test]
    fn strict_geometry_length_slope_axes_speed_and_obstacles() {
        let a = pulley([0, 0, 0], Axis::Z);
        assert_eq!(
            connect(a, pulley([19, 0, 0], Axis::Z), 20.0, |_| Space::Replaceable)
                .unwrap()
                .segments
                .len(),
            20
        );
        assert_eq!(
            connect(a, pulley([20, 0, 0], Axis::Z), 20.0, |_| Space::Replaceable),
            Err(ConnectionError::Distance)
        );
        assert_eq!(
            connect(a, pulley([14, 14, 0], Axis::Z), 20.0, |_| {
                Space::Replaceable
            })
            .unwrap()
            .slope,
            Slope::Upward
        );
        assert!(
            connect(a, pulley([15, 15, 0], Axis::Z), 20.0, |_| {
                Space::Replaceable
            })
            .is_err()
        );
        assert!(connect(a, pulley([2, 1, 0], Axis::Z), 20.0, |_| Space::Replaceable).is_err());
        assert!(connect(a, pulley([4, 0, 0], Axis::Z), 20.0, |_| Space::Blocked).is_err());
        let mut b = pulley([4, 0, 0], Axis::Z);
        b.rpm = -16.0;
        assert_eq!(
            connect(a, b, 20.0, |_| Space::Replaceable),
            Err(ConnectionError::OpposingRotation)
        );
        b.rpm = 0.0;
        assert!(connect(a, b, 20.0, |_| Space::Replaceable).is_ok());
        let sideways = connect(
            pulley([0, 0, 0], Axis::Y),
            pulley([0, 0, 3], Axis::Y),
            20.0,
            |_| Space::Replaceable,
        )
        .unwrap();
        assert_eq!(sideways.slope, Slope::Sideways);
        assert!(!sideways.slope.transports());
        let vertical = connect(a, pulley([0, 3, 0], Axis::Z), 20.0, |_| {
            Space::Shaft(Axis::Z)
        })
        .unwrap();
        assert_eq!(vertical.facing, Direction::East);
        assert_eq!(vertical.segments[1].part, Part::Pulley);
    }
    #[test]
    fn provenance_reserves_pending_input_without_merging_and_simulation_is_read_only() {
        let mut inventory = Inventory::default();
        let b = belt(0.0);
        let stack = Stack {
            payload: "same-item-full-components",
            count: 80,
            maximum: 64,
        };
        let before = inventory.pending.len();
        let remainder = inventory.insert_automation(b, 1, stack.clone(), 50, true);
        assert_eq!(remainder.count, 16);
        assert_eq!(inventory.pending.len(), before);
        assert_eq!(
            inventory
                .insert_automation(b, 1, stack.clone(), 50, false)
                .count,
            16
        );
        assert_eq!(inventory.pending[0].stack.count, 64);
        assert_eq!(
            inventory
                .insert_automation(b, 1, stack.clone(), 50, false)
                .count,
            80
        );
        assert_eq!(inventory.pending.len(), 1);
        assert!(!inventory.insert_direct(b, 2, &item(0.0, 1), Direction::Up, false, false));
        inventory.flush();
        assert_eq!(inventory.extract(1, 10, true).unwrap().count, 10);
        assert_eq!(inventory.items[0].stack.count, 64);
        assert_eq!(inventory.extract(1, 10, false).unwrap().count, 10);
        assert_eq!(inventory.items[0].stack.count, 54);
    }
    #[test]
    fn blocked_end_backpressure_reversal_and_partial_output_conserve_stacks() {
        let mut inventory = Inventory {
            items: vec![item(3.0, 5), item(1.0, 7)],
            positive: true,
            ..Default::default()
        };
        let mut hooks = effects(Ending::Blocked);
        for _ in 0..40 {
            inventory.tick(belt(480.0), &mut hooks);
        }
        assert_eq!(inventory.items[0].position, 4.55);
        assert_eq!(inventory.items[1].position, 3.5500002);
        assert_eq!(
            inventory.items.iter().map(|i| i.stack.count).sum::<u32>(),
            12
        );
        inventory.tick(belt(-48.0), &mut hooks);
        assert_eq!(inventory.items[0].stack.count, 7);
        assert!(inventory.items[0].position < 3.5500002);
        let mut hooks = effects(Ending::Insert);
        hooks.room = 3;
        let mut transferred = 0;
        for _ in 0..40 {
            transferred += inventory.tick(belt(480.0), &mut hooks).transferred;
        }
        assert_eq!(transferred, 3);
        assert_eq!(
            inventory.items.iter().map(|i| i.stack.count).sum::<u32>(),
            9
        );
    }
    #[test]
    fn processing_holds_at_source_offset_releases_and_one_frame_external_locks_clear() {
        let mut inventory = Inventory {
            items: vec![item(0.2, 1)],
            positive: true,
            ..Default::default()
        };
        let mut hooks = effects(Ending::Eject);
        hooks.processor = 1;
        hooks.hold = true;
        for _ in 0..10 {
            inventory.tick(belt(240.0), &mut hooks);
        }
        assert!(inventory.items[0].locked);
        assert_eq!(inventory.items[0].position, 1.5 + 1.0 / 512.0);
        hooks.hold = false;
        inventory.tick(belt(240.0), &mut hooks);
        assert!(!inventory.items[0].locked);
        assert!(inventory.items[0].position > 1.5 + 1.0 / 512.0);
        inventory.items[0].externally_locked = true;
        let position = inventory.items[0].position;
        inventory.tick(belt(240.0), &mut hooks);
        assert_eq!(inventory.items[0].position, position);
        assert!(!inventory.items[0].externally_locked);
        let mut count = 0;
        for _ in 0..20 {
            count += inventory.tick(belt(240.0), &mut hooks).ejected.len();
        }
        assert_eq!(count, 1);
        assert!(inventory.items.is_empty());
    }

    #[test]
    fn processing_conversion_queues_outputs_and_held_remainder_in_both_directions() {
        for (positive, held_position) in [(true, 2.5 - 1.0 / 512.0), (false, 2.5 + 1.0 / 512.0)] {
            let mut input = item(2.5, 8);
            input.angle = 43;
            input.previous_position = 2.375;
            let mut inventory = Inventory {
                items: vec![input],
                positive,
                ..Default::default()
            };
            assert_eq!(
                inventory.apply_to_each_within(2.5, 0.51, |input| {
                    input.clear_fan_processing();
                    let mut output = input.source_copy();
                    output.stack.payload = "copper-sheet";
                    output.stack.count = 1;
                    output.locked = true;
                    let mut held = input.source_copy();
                    held.stack.count -= 1;
                    Some(TransportedResult::Convert {
                        outputs: vec![output],
                        held: Some(held),
                    })
                }),
                1
            );
            assert_eq!(inventory.items.len(), 1);
            assert!(inventory.items[0].scheduled_removal);
            assert_eq!(inventory.pending.len(), 2);
            assert_eq!(inventory.pending[0].position, held_position);
            assert_eq!(inventory.pending[0].previous_position, 2.375);
            assert_eq!(inventory.pending[1].position, 2.5);
            assert!(inventory.pending[1].locked);
            assert_eq!(
                inventory.apply_to_each_within(2.5, 0.51, |_| {
                    panic!("queued removals and new outputs are not processed again")
                }),
                0
            );
            inventory.flush();
            assert_eq!(inventory.items.len(), 2);
            assert_eq!(
                inventory.items.iter().map(|i| i.stack.count).sum::<u32>(),
                8
            );
            assert!(inventory.items.iter().all(|i| !i.scheduled_removal));
        }
    }

    #[test]
    fn held_callback_conversion_uses_the_handler_queue_without_losing_its_outputs() {
        struct Press;
        impl Hooks<&'static str> for Press {
            fn processor(&mut self, segment: i32) -> Option<bool> {
                Some(segment == 1)
            }
            fn held_update(
                &mut self,
                input: &mut Transported<&'static str>,
                _: i32,
            ) -> ProcessingUpdate<&'static str> {
                let mut output = input.source_copy();
                output.stack.payload = "sheet";
                output.stack.count = 1;
                output.locked = true;
                let mut held = input.source_copy();
                held.stack.count -= 1;
                ProcessingUpdate {
                    decision: ProcessingResult::Hold,
                    conversion: Some(TransportedResult::Convert {
                        outputs: vec![output],
                        held: Some(held),
                    }),
                }
            }
        }
        let mut input = item(1.5 + 1.0 / 512.0, 3);
        input.locked = true;
        let mut inventory = Inventory {
            items: vec![input],
            positive: true,
            ..Default::default()
        };
        let result = inventory.tick(belt(32.0), &mut Press);
        assert_eq!(result.removed_processing, 0);
        assert!(inventory.items[0].scheduled_removal);
        assert_eq!(inventory.pending.len(), 2);
        assert_eq!(inventory.pending[0].position, 1.5 - 1.0 / 512.0);
        // Serializing or the following tick flushes exactly the same queue.
        inventory.tick(belt(0.0), &mut Press);
        assert_eq!(inventory.items.len(), 2);
        assert_eq!(inventory.items[0].stack.payload, "sheet");
        assert!(inventory.items[0].locked);
        assert_eq!(inventory.items[1].stack.count, 2);
        assert!(!inventory.items[1].locked);
    }

    #[test]
    fn equal_output_preserves_transport_identity_and_callback_mutations() {
        let mut input = item(1.5, 3);
        input.angle = 91;
        input.fan.time = 17;
        let mut inventory = Inventory {
            items: vec![input],
            ..Default::default()
        };
        assert_eq!(
            inventory.apply_to_each_within(1.5, 0.51, |input| {
                input.fan.time -= 1;
                let mut same = input.source_copy();
                same.angle = 240;
                same.position = 4.5;
                // maximum is derived container policy, not part of ItemStack.matches.
                same.stack.maximum = 16;
                Some(TransportedResult::Convert {
                    outputs: vec![same],
                    held: None,
                })
            }),
            0
        );
        assert_eq!(
            (
                inventory.items[0].angle,
                inventory.items[0].position,
                inventory.items[0].fan.time
            ),
            (91, 1.5, 16)
        );
        assert!(inventory.pending.is_empty());
        assert_eq!(
            inventory.apply_to_each_within(1.5, 0.51, |input| {
                input.fan.time -= 1;
                Some(TransportedResult::DoNothing)
            }),
            0
        );
        assert_eq!(inventory.items[0].fan.time, 15);
    }

    #[test]
    fn processing_distance_is_strict_and_equal_records_keep_separate_identities() {
        let mut inventory = Inventory {
            items: vec![item(2.0, 2), item(2.0, 2), item(3.0, 1)],
            ..Default::default()
        };
        assert_eq!(
            inventory.apply_to_each_within(2.5, 0.5, |_| panic!("boundary excluded")),
            0
        );
        let mut calls = 0;
        assert_eq!(
            inventory.apply_to_each_within(2.5, 0.51, |_| {
                calls += 1;
                Some(TransportedResult::Convert {
                    outputs: Vec::new(),
                    held: None,
                })
            }),
            3
        );
        assert_eq!(calls, 3);
        assert_eq!(inventory.items.len(), 3);
        // Ordinary write/tick flushing applies removals even while stopped.
        inventory.tick(belt(0.0), &mut effects(Ending::Eject));
        assert!(inventory.items.is_empty());
    }

    #[test]
    fn exact_segment_edges_and_copy_preserve_components_fan_but_clear_locks() {
        let mut original = item(1.0, 8);
        original.stack.maximum = 16;
        original.angle = 37;
        original.side_offset = -0.1;
        original.locked = true;
        original.externally_locked = true;
        original.fan.time = 73;
        let copy = original.source_copy();
        assert_eq!(copy.stack, original.stack);
        assert_eq!(copy.fan, original.fan);
        assert!(!copy.locked);
        assert!(!copy.externally_locked);
        assert_eq!(copy.side_offset, -0.1);
        let mut inventory = Inventory {
            items: vec![original],
            ..Default::default()
        };
        assert_eq!(inventory.stack_at(0), Some(0));
        assert_eq!(inventory.stack_at(1), None);
        assert_eq!(inventory.extract(0, 99, false).unwrap().count, 8);
        assert_eq!(inventory.stack_at(0), None);
    }

    #[test]
    fn handler_extraction_cannot_take_a_converted_record_before_queue_flush() {
        let mut inventory = Inventory {
            items: vec![item(0.5, 8)],
            ..Default::default()
        };
        assert_eq!(
            inventory.apply_to_each_within(0.5, 0.51, |_| {
                let mut output = item(0.5, 3);
                output.stack.payload = "processed";
                Some(TransportedResult::Convert {
                    outputs: vec![output],
                    held: None,
                })
            }),
            1
        );
        // Source getStackAtOffset skips toRemove and cannot see toInsert yet.
        assert_eq!(inventory.stack_at(0), None);
        assert!(inventory.extract(0, 64, false).is_none());
        inventory.flush();
        let output = inventory.extract(0, 64, false).unwrap();
        assert_eq!((output.payload, output.count), ("processed", 3));
        assert_eq!(inventory.stack_at(0), None);
        inventory.flush();
        assert!(inventory.items.is_empty());
    }
}

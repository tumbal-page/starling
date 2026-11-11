// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Canonical SmartFluidTankBehaviour tank saves, verified against the authored
// Java 6.0.10 Basin fixture. Unknown state is unsupported, never an empty tank.
use create_engine::basin::{Fluid, FluidLevel, TANK_CAPACITY};
use horizon_world::nbt::{Compound, Tag};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Key {
    pub id: String,
    pub components: Compound,
}
pub(super) type Tanks = [Option<Fluid<Key>>; 2];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Unsupported(pub &'static str);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Level {
    pub value: f32,
    pub target: f32,
    pub speed: f32,
    pub force: bool,
}

fn fields(compound: &Compound, allowed: &[&str]) -> bool {
    compound
        .keys()
        .all(|key| allowed.iter().any(|name| key.to_string_lossy() == *name))
}
fn compound<'a>(tag: Option<&'a Tag>, reason: &'static str) -> Result<&'a Compound, Unsupported> {
    match tag {
        Some(Tag::Compound(value)) => Ok(value),
        _ => Err(Unsupported(reason)),
    }
}
fn level(segment: &Compound) -> Result<&Compound, Unsupported> {
    let level = compound(
        segment.get(&"Level".into()),
        "noncanonical basin fluid level",
    )?;
    if !fields(level, &["Value", "Target", "Speed", "Force"]) {
        return Err(Unsupported("unknown basin fluid level fields"));
    }
    for key in ["Value", "Target", "Speed"] {
        match level.get(&key.into()) {
            Some(Tag::Float(value)) if f32::from_bits(*value).is_finite() => {}
            _ => return Err(Unsupported("noncanonical basin fluid level number")),
        }
    }
    if level
        .get(&"Force".into())
        .is_some_and(|value| !matches!(value, Tag::Byte(0 | 1)))
    {
        return Err(Unsupported("noncanonical basin fluid level force flag"));
    }
    Ok(level)
}
fn read_segment(tag: &Tag) -> Result<Option<Fluid<Key>>, Unsupported> {
    let segment = compound(Some(tag), "noncanonical basin fluid segment")?;
    if !fields(segment, &["TankContent", "Level"]) {
        return Err(Unsupported("unknown basin fluid segment fields"));
    }
    level(segment)?;
    let content = compound(
        segment.get(&"TankContent".into()),
        "noncanonical basin tank content",
    )?;
    if content.is_empty() {
        return Ok(None);
    }
    if !fields(content, &["Fluid"]) {
        return Err(Unsupported("unknown basin tank content fields"));
    }
    let fluid = compound(
        content.get(&"Fluid".into()),
        "noncanonical nested basin fluid",
    )?;
    if !fields(fluid, &["id", "amount", "components"]) {
        return Err(Unsupported("unknown basin fluid fields"));
    }
    let Some(Tag::String(id)) = fluid.get(&"id".into()) else {
        return Err(Unsupported("missing basin fluid id"));
    };
    let id = id.to_string_lossy();
    if !matches!(id.as_str(), "minecraft:water" | "minecraft:lava") {
        return Err(Unsupported("unsupported basin fluid id"));
    }
    let Some(Tag::Int(amount)) = fluid.get(&"amount".into()) else {
        return Err(Unsupported("noncanonical basin fluid amount"));
    };
    if !(1..=TANK_CAPACITY as i32).contains(amount) {
        return Err(Unsupported(
            "basin fluid amount outside supported tank capacity",
        ));
    }
    let components = match fluid.get(&"components".into()) {
        None => Compound::new(),
        Some(Tag::Compound(value)) => value.clone(),
        _ => return Err(Unsupported("noncanonical basin fluid components")),
    };
    Ok(Some(Fluid {
        key: Key { id, components },
        amount: *amount as u32,
    }))
}
pub(super) fn read_half(value: Option<&Tag>) -> Result<Tanks, Unsupported> {
    let Some(Tag::List(segments)) = value else {
        return Err(Unsupported("missing basin tank list"));
    };
    if segments.len() != 2 {
        return Err(Unsupported("unsupported basin tank count"));
    }
    Ok([read_segment(&segments[0])?, read_segment(&segments[1])?])
}
pub(super) fn read_levels(value: Option<&Tag>) -> Result<[Level; 2], Unsupported> {
    read_half(value)?;
    let Some(Tag::List(segments)) = value else {
        unreachable!();
    };
    Ok(std::array::from_fn(|i| {
        let c = level(compound(Some(&segments[i]), "basin segment").unwrap()).unwrap();
        let number = |key: &str| match c.get(&key.into()) {
            Some(Tag::Float(v)) => f32::from_bits(*v),
            _ => unreachable!(),
        };
        Level {
            value: number("Value"),
            target: number("Target"),
            speed: number("Speed"),
            force: c.contains_key(&"Force".into()),
        }
    }))
}
/// Server tank chasers tick even when no recipe is running. Force is consumed
/// by original serialization, not ticking; its separate known gap is retained.
pub(super) fn tick_half(value: &mut Tag) -> Result<(), Unsupported> {
    let levels = read_levels(Some(value))?;
    let Tag::List(segments) = value else {
        unreachable!()
    };
    for (segment, level) in segments.iter_mut().zip(levels) {
        let mut motion = FluidLevel {
            value: level.value,
            target: level.target,
            speed: level.speed,
            ..FluidLevel::default()
        };
        motion.tick();
        let Tag::Compound(segment) = segment else {
            unreachable!()
        };
        let Some(Tag::Compound(level)) = segment.get_mut(&"Level".into()) else {
            unreachable!()
        };
        level.insert("Value".into(), Tag::Float(motion.value.to_bits()));
    }
    Ok(())
}
/// Preflight the original entire half before constructing replacements. The
/// caller commits only after BOTH halves are supported. Full component patches
/// survive; an unchanged tank's exact original NBT is returned unchanged.
pub(super) fn write_half(original: Option<&Tag>, tanks: &Tanks) -> Result<Tag, Unsupported> {
    let before = read_half(original)?;
    let Some(Tag::List(segments)) = original else {
        unreachable!();
    };
    let mut result = segments.clone();
    for i in 0..2 {
        if before[i] == tanks[i] {
            continue;
        }
        let mut segment = compound(Some(&segments[i]), "noncanonical basin segment")?.clone();
        let mut content = Compound::new();
        if let Some(fluid) = tanks[i].as_ref() {
            if fluid.amount == 0
                || fluid.amount > TANK_CAPACITY
                || !matches!(fluid.key.id.as_str(), "minecraft:water" | "minecraft:lava")
            {
                return Err(Unsupported("unsupported replacement basin fluid"));
            }
            let mut payload = Compound::from([
                ("id".into(), Tag::String(fluid.key.id.as_str().into())),
                ("amount".into(), Tag::Int(fluid.amount as i32)),
            ]);
            if !fluid.key.components.is_empty() {
                payload.insert(
                    "components".into(),
                    Tag::Compound(fluid.key.components.clone()),
                );
            }
            content.insert("Fluid".into(), Tag::Compound(payload));
        }
        segment.insert("TankContent".into(), Tag::Compound(content));
        let mut level = level(&segment)?.clone();
        // onFluidStackChanged().chase(amount/capacity,.25,EXP). Retain
        // current Value and the serialization-boundary Force flag; never
        // invent a tick-based one-shot clear for that known API gap.
        let target = tanks[i]
            .as_ref()
            .map_or(0.0, |fluid| fluid.amount as f32 / TANK_CAPACITY as f32);
        level.insert("Target".into(), Tag::Float(target.to_bits()));
        level.insert("Speed".into(), Tag::Float(0.25f32.to_bits()));
        segment.insert("Level".into(), Tag::Compound(level));
        result[i] = Tag::Compound(segment);
    }
    Ok(Tag::List(result))
}

// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// WaterWheelBlockEntity and LargeWaterWheelBlock's physical structural ring.
use super::{Game, create_engine};
use crate::game::native_block_entity_draw::BlockEntityDraw;
use horizon_client::containers::ModBlockEntity;
use crate::game::create_port::{integer, number};
use super::create_engine::{
    kinetics::{Axis, Pos},
    placement::Direction,
    waterwheel,
};
use horizon_core::{geometry::Direction as FluidDirection, position::BlockPos};
use horizon_world::{
    fluid::FluidState,
    fluid_flow::FluidFlowWorld,
    nbt::{Compound, Tag},
    registry::BlockStateId,
};
const STRUCTURE: &str = "create:water_wheel_structure";
fn at(p: Pos, d: Pos) -> Pos {
    std::array::from_fn(|i| p[i] + d[i])
}
fn set(game: &mut Game<'_>, pos: Pos, name: &str, facing: Option<Direction>) {
    let dim = game.dim.id().to_owned();
    if let Some(id) = game.pack.assets.registry.default_state(name) {
        game.native_set_block(&dim, pos, id, 3);
        if let Some(d) = facing {
            game.native_set_block_property(pos, "facing", d.name());
        }
    }
}
pub(super) fn replaceable(game: &Game<'_>, p: Pos) -> bool {
    game.res
        .facts()
        .get(game.world.block(p[0], p[1], p[2]))
        .replaceable
}
fn required(axis: Axis) -> Vec<(Pos, Direction)> {
    let mut result = Vec::new();
    for d in Direction::ALL {
        if d.axis() == axis {
            continue;
        }
        result.push((d.vector(), d.opposite()));
        let second = d.clockwise(axis);
        result.push((at(d.vector(), second.vector()), second.opposite()));
    }
    result
}
pub(super) fn master(game: &Game<'_>, mut p: Pos) -> Option<Pos> {
    for _ in 0..3 {
        let block = game.native_block(p);
        if block == "create:large_water_wheel" {
            return Some(p);
        }
        if block != STRUCTURE {
            return None;
        }
        let name = game.native_block_property(p, "facing")?;
        p = at(
            p,
            Direction::ALL
                .into_iter()
                .find(|d| d.name() == name)?
                .vector(),
        );
    }
    None
}
pub(super) fn tick(
    game: &mut Game<'_>,
    pos: Pos,
    entity: &mut ModBlockEntity,
    block: &str,
) -> bool {
    if block == STRUCTURE {
        if master(game, pos).is_none() {
            set(game, pos, "minecraft:air", None);
        }
        return true;
    }
    if !matches!(block, "create:water_wheel" | "create:large_water_wheel") {
        return false;
    }
    let Some(node) = crate::game::create_port::node(game, pos) else {
        return true;
    };
    let large = block == "create:large_water_wheel";
    if large {
        let initialized = integer(&entity.nbt, "create_horizon.StructureReady") != 0;
        for (delta, facing) in required(node.axis) {
            let p = at(pos, delta);
            if game.native_block(p) == STRUCTURE
                && game.native_block_property(p, "facing").as_deref() == Some(facing.name())
            {
                continue;
            }
            if initialized || !replaceable(game, p) {
                if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
                    log::info!(
                        "Create wheel removed: {pos:?}, {p:?}, {}, initialized {initialized}",
                        game.native_block(p)
                    );
                }
                set(game, pos, "minecraft:air", None);
                if initialized {
                    game.spawn_item(
                        horizon_client::items::ItemStack::new(block, 1),
                        pos.map(|v| f64::from(v) + 0.5),
                        [0.0; 3],
                        10,
                    );
                }
                crate::game::create_port_kinetic_runtime::invalidate();
                return true;
            }
            set(game, p, STRUCTURE, Some(facing));
        }
        entity
            .nbt
            .insert("create_horizon.StructureReady".into(), Tag::Byte(1));
        let mut negative = [0; 3];
        negative[node.axis.index()] = -1;
        let extension = game.native_block(at(pos, negative)) == "create:large_water_wheel";
        game.native_set_block_property(pos, "extension", if extension { "true" } else { "false" });
    }
    // ModBlockEntity::DerefMut advances its content revision. entry() would
    // therefore invalidate unchanged client geometry on every wheel tick.
    #[expect(
        clippy::map_entry,
        reason = "entry would mutate an unchanged live BE revision"
    )]
    if !entity.nbt.contains_key(&"Material".into()) {
        entity.nbt.insert(
            "Material".into(),
            Tag::Compound(Compound::from([(
                "Name".into(),
                Tag::String("minecraft:spruce_planks".into()),
            )])),
        );
    }
    let score = waterwheel::flow_score(node.axis, large, |offset| flow(game, at(pos, offset)));
    if integer(&entity.nbt, "FlowScore") != score {
        entity.nbt.insert("FlowScore".into(), Tag::Int(score));
        crate::game::create_port_kinetic_runtime::invalidate();
        game.native_block_entity_sync(pos);
    }
    false
}
struct Flow<'a, 'b> {
    game: &'a Game<'b>,
}
impl FluidFlowWorld for Flow<'_, '_> {
    type Error = std::convert::Infallible;
    type Block = BlockStateId;
    fn fluid(&mut self, p: BlockPos) -> Result<FluidState, Self::Error> {
        Ok(self
            .game
            .motion
            .fluids
            .fluid(self.game.world.block(p.x, p.y, p.z)))
    }
    fn block(&mut self, p: BlockPos) -> Result<BlockStateId, Self::Error> {
        Ok(self.game.world.block(p.x, p.y, p.z))
    }
    fn blocks_fluid_flow(&self, s: BlockStateId) -> bool {
        self.game.motion.fluids.blocks_flow(s)
    }
    fn is_ice_class(&self, s: BlockStateId) -> bool {
        self.game.motion.fluids.ice(s)
    }
    fn is_face_sturdy(
        &mut self,
        s: BlockStateId,
        _: BlockPos,
        d: FluidDirection,
    ) -> Result<bool, Self::Error> {
        Ok(self.game.motion.fluids.sturdy(s, d))
    }
}
fn flow(game: &Game<'_>, p: Pos) -> [f64; 3] {
    if game.native_block(p) == "minecraft:bubble_column" {
        return [
            0.0,
            if game.native_block_property(p, "drag").as_deref() == Some("true") {
                -1.0
            } else {
                1.0
            },
            0.0,
        ];
    }
    let state = game.motion.fluids.fluid(game.world.block(p[0], p[1], p[2]));
    let value = horizon_world::fluid_flow::flow_at(
        &mut Flow { game },
        state,
        BlockPos::new(p[0], p[1], p[2]),
    )
    .unwrap();
    [value.x, value.y, value.z]
}
pub(super) fn draw(
    game: &mut Game<'_>,
    pos: Pos,
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
    axis: Axis,
    block: &str,
) {
    let part = if block == "create:water_wheel" {
        "water_wheel/wheel"
    } else if game.native_block_property(pos, "extension").as_deref() == Some("true") {
        "large_water_wheel/block_extension"
    } else {
        "large_water_wheel/block"
    };
    let name = entity
        .nbt
        .get(&"Material".into())
        .and_then(|t| t.get("Name"))
        .and_then(|t| match t {
            Tag::String(s) => s.to_string().ok(),
            _ => None,
        })
        .unwrap_or_default();
    let wood = name
        .strip_prefix("minecraft:")
        .and_then(|s| s.strip_suffix("_planks"))
        .filter(|s| crate::game::android_bundled::create::assets::WOODS.contains(s))
        .unwrap_or("spruce");
    draw.push();
    let angle = super::create_engine::visual::angle(
        draw.game_time as f64 + f64::from(draw.partial),
        number(&entity.nbt, "Speed"),
        super::create_engine::visual::rotation_offset(pos, axis, false),
    );
    draw.rotate_about(super::create_engine::visual::positive_axis(axis), angle, [0.5; 3]);
    match axis {
        Axis::X => draw.rotate_about([0.0, 0.0, 1.0], -90.0, [0.5; 3]),
        Axis::Z => draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.5; 3]),
        Axis::Y => {}
    }
    game.native_draw_model(
        draw,
        &format!("create:block/{part}_material_{wood}"),
        0xFFFFFF,
    );
    draw.pop();
}

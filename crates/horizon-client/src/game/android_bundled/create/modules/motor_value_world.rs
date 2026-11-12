// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create 6.0.10 ScrollValueRenderer, ValueBox.TextValueBox,
// ValueBoxTransform.Sided, CreativeMotorBlockEntity.MotorValueBox, AllIcons;
// bundled MIT Catnip Outliner/OutlineEntry. No original asset bytes.
// Adapted for Android bundled port.

use super::{Game, native_world_draw::NativeWorldDraw};
use horizon_client::containers::BlockEntity;
use horizon_world::nbt::Tag;
use std::{cell::RefCell, collections::HashMap};

const ICON: &str = "create:gui/icons";
const FULL_BRIGHT: u32 = 240 | (240 << 8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Side([i32; 3]);
impl Side {
    fn named(name: &str) -> Option<Self> {
        Some(Self(match name {
            "down" => [0, -1, 0],
            "up" => [0, 1, 0],
            "north" => [0, 0, -1],
            "south" => [0, 0, 1],
            "west" => [-1, 0, 0],
            "east" => [1, 0, 0],
            _ => return None,
        }))
    }
    fn axis(self) -> usize {
        self.0.iter().position(|v| *v != 0).unwrap_or(1)
    }
    // Catnip AngleHelper negates Direction.toYRot on the X axis.
    fn horizontal_angle(self) -> f32 {
        match self.0 {
            [0, 0, -1] => 180.0,
            [-1, 0, 0] => -90.0,
            [1, 0, 0] => -270.0,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Plan {
    pub pos: [i32; 3],
    facing: Side,
    side: Side,
    pub text: String,
    pub active: bool,
    ticks_until_removal: i32,
}
impl Plan {
    fn side_active(&self) -> bool {
        self.side.axis() != self.facing.axis()
            && !(self.facing.axis() != 1 && self.side.0 == [0, -1, 0])
    }
    pub fn offset(&self) -> [f32; 3] {
        std::array::from_fn(|i| {
            0.5 + self.side.0[i] as f32 * (4.5 / 16.0) - self.facing.0[i] as f32 / 16.0
        })
    }
    pub fn rotation(&self) -> [f32; 3] {
        [
            self.side.horizontal_angle() + 180.0,
            match self.side.0 {
                [0, 1, 0] => 90.0,
                [0, -1, 0] => 270.0,
                _ => 0.0,
            },
            if self.facing.axis() != 1 && self.side.0 == [0, 1, 0] {
                -self.facing.horizontal_angle() + 180.0
            } else {
                0.0
            },
        ]
    }
    fn hit(&self, point: [f64; 3]) -> bool {
        let offset = self.offset();
        self.side_active()
            && (0..3)
                .map(|i| (point[i] - f64::from(self.pos[i]) - f64::from(offset[i])).powi(2))
                .sum::<f64>()
                < 0.25 * 0.25
    }
    fn visible(&self, partial: f32) -> bool {
        if !self.side_active() || self.ticks_until_removal < -8 {
            return false;
        }
        if self.ticks_until_removal >= 0 {
            return true;
        }
        // Outliner checks cubic alpha before calling ValueBox.render. The
        // latter deliberately ignores alpha, so it stays opaque until cut off.
        let previous = (1.0 + (self.ticks_until_removal + 1) as f32 / 8.0).min(1.0);
        let current = 1.0 + self.ticks_until_removal as f32 / 8.0;
        let alpha = previous + partial * (current - previous);
        alpha * alpha * alpha >= 0.125
    }

    /// Draw under a pose already translated to `pos - camera`.
    ///
    /// The backend uses the normal depth-tested text layer. `quad` is
    /// unshaded with ARGB tint; `text` uses the actual font and its eight-offset
    /// outline; its front glyphs use the original polygon-offset text pass.
    pub fn render(&self, draw: &mut NativeWorldDraw, width: i32, canvas: &mut impl Canvas) {
        if width <= 0 || !self.visible(draw.partial) {
            return;
        }
        let light = draw.light;
        draw.light = FULL_BRIGHT;
        draw.push();
        draw.translate(self.offset().map(f64::from));
        let [yaw, pitch, roll] = self.rotation();
        draw.rotate_about([0.0, 1.0, 0.0], yaw, [0.0; 3]);
        draw.rotate_about([1.0, 0.0, 0.0], pitch, [0.0; 3]);
        draw.rotate_about([0.0, 0.0, 1.0], roll, [0.0; 3]);
        draw.scale([0.5; 3]);
        if self.active {
            draw.push();
            draw.scale([-2.01, -2.01, 2.01]);
            draw.translate([-0.5, -0.5, -0.5 / 16.0]);
            canvas.quad(
                draw,
                ICON,
                [
                    [0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [1.0, 0.0, 0.0],
                ],
                // AllIcons.VALUE_BOX_HOVER_6PX = texels (32,128)..(48,144)
                // on the 256x256 sheet; native helpers use 16 texels per side.
                [2.0, 8.0, 3.0, 9.0],
                0xffff_ffff,
            );
            draw.pop();
        }
        draw.scale([-1.0 / 64.0; 3]);
        draw.scale([3.0, 3.0, 1.0]);
        draw.translate([-4.0, -3.75, 5.0]);
        let single = width < 10;
        let scale = 9.0 / width as f32 / if single { 2.0 } else { 1.0 };
        let margin = (width - 9) as f32 / 2.0;
        draw.scale([scale; 3]);
        draw.translate([
            if single { (width / 2) as f64 } else { 0.0 },
            if single { -margin as f64 } else { margin as f64 },
            0.0,
        ]);
        canvas.text(draw, &self.text, 0xffed_eded, 0xff33_3333);
        draw.pop();
        draw.light = light;
    }
}

/// Native world canvas boundary; all geometry goes through Horizon's ordinary
/// font/atlas renderer, with no mod-owned GPU objects.
pub(super) trait Canvas {
    fn quad(
        &mut self,
        draw: &mut NativeWorldDraw,
        sprite: &str,
        corners: [[f32; 3]; 4],
        uv: [f32; 4],
        argb: u32,
    );
    fn text(&mut self, draw: &mut NativeWorldDraw, text: &str, color: u32, outline: u32);
}
struct NativeCanvas<'a, 'g>(&'a mut Game<'g>);
impl Canvas for NativeCanvas<'_, '_> {
    fn quad(
        &mut self,
        draw: &mut NativeWorldDraw,
        sprite: &str,
        corners: [[f32; 3]; 4],
        uv: [f32; 4],
        argb: u32,
    ) {
        self.0.native_world_quad(
            draw,
            sprite,
            corners,
            uv,
            [
                (argb >> 16) as u8,
                (argb >> 8) as u8,
                argb as u8,
                (argb >> 24) as u8,
            ],
        );
    }
    fn text(&mut self, draw: &mut NativeWorldDraw, text: &str, color: u32, outline: u32) {
        self.0.native_world_text(
            draw,
            &horizon_client::text::Text::literal(text),
            color,
            Some(outline),
        );
    }
}
pub(super) fn render(game: &mut Game<'_>, draw: &mut NativeWorldDraw) {
    let plan_data: Vec<(Plan, i32)> = plans(game)
        .into_iter()
        .map(|plan| {
            let width = game.gui.atlas.font.width(&plan.text).ceil() as i32;
            (plan, width)
        })
        .collect();
    let mut canvas = NativeCanvas(game);
    for (plan, width) in plan_data {
        draw.push();
        if draw.translate_world(plan.pos.map(f64::from)) {
            plan.render(draw, width, &mut canvas);
        }
        draw.pop();
    }
}

#[derive(Default)]
struct State {
    identity: (usize, usize, &'static str),
    boxes: HashMap<[i32; 3], Plan>,
}
thread_local! {static STATE: RefCell<State> = RefCell::new(State::default());}
fn identity(game: &Game<'_>) -> (usize, usize, &'static str) {
    (
        std::ptr::from_ref(game) as usize,
        std::ptr::from_ref(&game.world) as usize,
        game.native_dimension(),
    )
}

fn target(game: &Game<'_>) -> Option<Plan> {
    let hit = game.pick()?;
    if game.native_block(hit.pos) != "create:creative_motor"
        || [
            game.mob_hit().map(|hit| hit.1),
            game.boat_hit().map(|hit| hit.1),
            game.cart_hit().map(|hit| hit.1),
            game.armor_stand_hit().map(|hit| hit.1),
            game.cushion_hit().map(|hit| hit.1),
            game.frame_hit().map(|hit| hit.1),
            game.painting_hit().map(|hit| hit.1),
            game.dragon_hit().map(|hit| hit.1),
        ]
        .into_iter()
        .flatten()
        .any(|distance| game.before_block(distance))
    {
        return None;
    }
    let BlockEntity::Mod(entity) = game
        .world
        .block_entities
        .get(&(hit.pos[0], hit.pos[1], hit.pos[2]))?
    else {
        return None;
    };
    let value = match entity.nbt.get(&"ScrollValue".into()) {
        Some(Tag::Int(value)) => *value,
        _ => 16,
    };
    let mut plan = Plan {
        pos: hit.pos,
        facing: Side::named(&game.native_block_property(hit.pos, "facing")?)?,
        side: Side(hit.face),
        text: value.wrapping_abs().to_string(),
        active: false,
        ticks_until_removal: 1,
    };
    // MotorValueBox does not override bypassesInput, even for a clipboard.
    plan.active = plan.hit(hit.point);
    Some(plan)
}

/// Once per unpaused real client tick: Ponder ticks old outlines in Pre, then
/// Create's ScrollValueRenderer replaces the current entry in Post.
pub(super) fn tick(game: &Game<'_>) {
    if !game.native_window() || !game.native_on("create") || game.player.is_none() {
        reset();
        return;
    }
    let current = target(game);
    STATE.with_borrow_mut(|state| {
        let world = identity(game);
        if state.identity != world {
            state.identity = world;
            state.boxes.clear();
        }
        state.boxes.retain(|_, plan| {
            plan.ticks_until_removal -= 1;
            plan.ticks_until_removal >= -8
        });
        if let Some(plan) = current {
            state.boxes.insert(plan.pos, plan);
        }
    });
}

pub(super) fn reset() {
    STATE.with_borrow_mut(|state| *state = State::default());
}

/// Source goggles suppression checks alive active entries, including entries
/// whose world rendering has already reached the fade cutoff.
pub(super) fn active(game: &Game<'_>) -> bool {
    STATE.with_borrow(|state| {
        state.identity == identity(game) && state.boxes.values().any(|plan| plan.active)
    })
}

/// A world pass must also draw cached entries after a motor is removed; a block
/// entity submit callback alone does not cover that source Outliner behavior.
pub(super) fn plans(game: &Game<'_>) -> Vec<Plan> {
    STATE.with_borrow(|state| {
        if state.identity == identity(game) {
            state.boxes.values().cloned().collect()
        } else {
            Vec::new()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan(facing: &str, side: &str) -> Plan {
        Plan {
            pos: [0; 3],
            facing: Side::named(facing).unwrap(),
            side: Side::named(side).unwrap(),
            text: "128".into(),
            active: true,
            ticks_until_removal: 1,
        }
    }
    #[test]
    fn source_motor_sides_and_top_orientation() {
        for facing in ["down", "up", "north", "south", "west", "east"] {
            let allowed = ["down", "up", "north", "south", "west", "east"]
                .into_iter()
                .filter(|side| plan(facing, side).side_active())
                .count();
            assert_eq!(
                allowed,
                if facing == "up" || facing == "down" {
                    4
                } else {
                    3
                }
            );
        }
        let top = plan("east", "up");
        assert_eq!(top.offset(), [0.4375, 0.78125, 0.5]);
        assert_eq!(top.rotation(), [180.0, 90.0, 450.0]);
        assert!(top.hit([0.4375, 1.0, 0.5]));
        assert!(!top.hit([0.6875, 0.78125, 0.5]));
    }
    #[test]
    fn value_box_opaque_until_outliner_cubic_cutoff() {
        let mut box_ = plan("east", "south");
        box_.ticks_until_removal = -4;
        assert!(box_.visible(1.0));
        box_.ticks_until_removal = -5;
        assert!(box_.visible(0.0));
        assert!(!box_.visible(0.001));
        // Entry remains alive for goggles suppression after becoming invisible.
        assert!(box_.active);
    }
}
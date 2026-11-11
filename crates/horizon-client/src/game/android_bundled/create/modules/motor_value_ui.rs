// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create 6.0.10 ValueSettingsClient/Screen and KineticScrollValueBehaviour.
// Rendering uses the original private value_settings.png sheet via the importer.
// Adapted for Android bundled port.

use super::Game;
use crate::{containers::BlockEntity, mod_api::NativeScreenView};
use horizon_client::{
    first_person::Hand,
    hud::{HudBatch, rgba},
    keybinds,
    text::{Style, Text},
};
use horizon_world::nbt::Tag;
use std::cell::RefCell;

const SPRITE: &str = "create:gui/value_settings";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Setting {
    row: i32,
    value: i32,
}
#[derive(Clone, Copy)]
struct Holding {
    pos: [i32; 3],
    ticks: u8,
}
#[derive(Clone, Copy)]
struct Panel {
    pos: [i32; 3],
    ticks: u32,
    hovered: Option<Setting>,
    sound_cooldown: u8,
}
#[derive(Default)]
struct State {
    holding: Option<Holding>,
    panel: Option<Panel>,
    cursor: Option<[f32; 2]>,
    sound: Option<f32>,
    submission: Option<([i32; 3], Setting)>,
    close: bool,
    hover_warmup: i32,
    hover_ticks: i32,
    last_hover_tip: bool,
}
thread_local! {static STATE: RefCell<State> = RefCell::new(State::default());}

fn title() -> Text {
    Text::translate("create.kinetics.creative_motor.rotation_speed", &[])
}
fn arrow(row: i32) -> Text {
    Text::styled(
        if row == 0 { "⟳" } else { "⟲" },
        Style {
            bold: Some(true),
            ..Style::default()
        },
    )
}
fn formatted(setting: Setting) -> Text {
    crate::game::create_port_goggles::numeral(f64::from(setting.value.abs().max(1)))
        .append(arrow(setting.row))
}
fn can_interact(game: &Game<'_>) -> bool {
    game.player.is_some()
        && game.may_build()
        && !game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft)
}
fn saved(game: &Game<'_>, pos: [i32; 3]) -> i32 {
    match game.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
        Some(BlockEntity::Mod(entity)) => match entity.nbt.get(&"ScrollValue".into()) {
            Some(Tag::Int(value)) => *value,
            _ => 16,
        },
        _ => 16,
    }
}
fn setting(value: i32) -> Setting {
    Setting {
        row: if value < 0 { 0 } else { 1 },
        value: value.saturating_abs().min(256),
    }
}

// CreativeMotorBlockEntity.MotorValueBox plus ValueBoxTransform.testHit.
fn target(game: &Game<'_>) -> Option<[i32; 3]> {
    let hit = game.pick()?;
    if game.native_block(hit.pos) != "create:creative_motor" {
        return None;
    }
    let facing = match game.native_block_property(hit.pos, "facing")?.as_str() {
        "east" => [1, 0, 0],
        "west" => [-1, 0, 0],
        "up" => [0, 1, 0],
        "down" => [0, -1, 0],
        "south" => [0, 0, 1],
        "north" => [0, 0, -1],
        _ => return None,
    };
    let axis = facing.iter().position(|value| *value != 0)?;
    if hit.face[axis] != 0 || (axis != 1 && hit.face == [0, -1, 0]) {
        return None;
    }
    let distance = (0..3)
        .map(|axis| {
            let offset =
                0.5 + f64::from(hit.face[axis]) * 4.5 / 16.0 - f64::from(facing[axis]) / 16.0;
            (hit.point[axis] - f64::from(hit.pos[axis]) - offset).powi(2)
        })
        .sum::<f64>();
    (distance < 0.25 * 0.25).then_some(hit.pos)
}

/// Alive source Outliner entries suppress goggles, including their expiry ticks.
pub(super) fn active_value_box(game: &Game<'_>) -> bool {
    crate::game::create_port_motor_value_world::active(game)
}

pub(super) fn interaction(
    game: &mut Game<'_>,
    button: i32,
    hand: Hand,
    _swing: &mut bool,
    canceled: &mut bool,
) {
    if button != 1
        || hand != Hand::Main
        || !game.native_on("create")
        || !can_interact(game)
        || game
            .held()
            .is_some_and(|stack| stack.item.as_ref() == "create:clipboard")
    {
        return;
    }
    let Some(pos) = target(game) else {
        return;
    };
    STATE.with_borrow_mut(|state| {
        if state.holding.is_none() {
            state.holding = Some(Holding { pos, ticks: 0 });
        }
    });
    *canceled = true;
}

/// Call once per real client tick, including while the non-pausing board is open.
pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() || !game.native_on("create") {
        return;
    }
    STATE.with_borrow_mut(|state| {
        state.hover_warmup = (state.hover_warmup - 1).max(0);
        state.hover_ticks = (state.hover_ticks - 1).max(0);
        if let Some(panel) = &mut state.panel {
            panel.ticks = panel.ticks.saturating_add(1);
            panel.sound_cooldown = panel.sound_cooldown.saturating_sub(1);
        }
    });
    let holding = STATE.with_borrow(|state| state.holding);
    if let Some(mut holding) = holding {
        let valid = can_interact(game)
            && target(game) == Some(holding.pos)
            && !game
                .held()
                .is_some_and(|stack| stack.item.as_ref() == "create:clipboard");
        if !valid || !game.native_mapping("key.use").is_some_and(|value| value.0) {
            // ScrollValueBehaviour.onShortInteract does nothing for a real player.
            STATE.with_borrow_mut(|state| state.holding = None);
        } else if holding.ticks >= 5 {
            STATE.with_borrow_mut(|state| state.holding = None);
            if game.native_mod_screen_open("create") {
                let current = setting(saved(game, holding.pos));
                STATE.with_borrow_mut(|state| {
                    state.panel = Some(Panel {
                        pos: holding.pos,
                        ticks: 0,
                        hovered: None,
                        sound_cooldown: 0,
                    });
                    if let Some(size) = game.native_physical_window_size() {
                        let batch = HudBatch::new(size);
                        state.cursor =
                            Some(layout(game, [batch.width, batch.height]).coordinate(current));
                    }
                });
            }
        } else {
            holding.ticks += 1;
            STATE.with_borrow_mut(|state| state.holding = Some(holding));
        }
    }
    if game.native_mod_screen().is_none() && target(game).is_some() {
        // ScrollValueRenderer.showHoverTip; one call per client tick.
        STATE.with_borrow_mut(|state| {
            if state.hover_warmup < 6 {
                state.hover_warmup += 2;
            } else {
                state.hover_warmup += 1;
                state.hover_ticks = if state.hover_ticks == 0 {
                    11
                } else {
                    state.hover_ticks.max(6)
                };
                state.last_hover_tip = true;
            }
        });
    }
}

#[derive(Clone, Copy, Debug)]
struct Layout {
    x: i32,
    y: i32,
    label: i32,
    bar: i32,
    width: i32,
    height: i32,
}
impl Layout {
    fn coordinate(self, setting: Setting) -> [f32; 2] {
        let column = setting.value;
        let mut x = self.x as f32 + (((column.max(1) - 1) / 32) * 4 + column) as f32 + 1.5;
        x += (self.label + 18) as f32;
        if column % 32 == 0 {
            x += 2.0;
        }
        if column > 0 {
            x += 4.0;
        }
        [x, self.y as f32 + (setting.row as f32 + 0.5) * 11.0 - 0.5]
    }
    fn closest(self, mouse: [f32; 2], shift: bool) -> Setting {
        let [mx, my] = mouse.map(|value| value as i32);
        let mut row = 0;
        let mut best = f64::MAX;
        for candidate in 0..2 {
            let difference = f64::from(
                (self.coordinate(Setting {
                    row: candidate,
                    value: 0,
                })[1]
                    - my as f32)
                    .abs(),
            );
            if best < difference {
                break;
            }
            best = difference;
            row = candidate;
        }
        best = f64::MAX;
        let mut value = 0;
        for column in 0..=256 {
            let candidate = if shift { column * 32 } else { column };
            let difference = f64::from(
                (self.coordinate(Setting {
                    row,
                    value: candidate,
                })[0]
                    - mx as f32)
                    .abs(),
            );
            if best < difference {
                break;
            }
            best = difference;
            value = candidate.min(256);
        }
        Setting { row, value }
    }
}
fn layout(game: &Game<'_>, size: [f32; 2]) -> Layout {
    let label = game
        .gui
        .atlas
        .font
        .width_text(&arrow(0))
        .max(game.gui.atlas.font.width_text(&arrow(1))) as i32;
    let bar = 257 + 1 + 9 * 4;
    let width = label + 14 + bar + 10;
    let height = 22;
    Layout {
        x: (size[0] as i32 - width) / 2,
        y: (size[1] as i32 - height) / 2,
        label,
        bar,
        width,
        height,
    }
}

fn quad(game: &Game<'_>, batch: &mut HudBatch, src: [f32; 4], dst: [f32; 4]) {
    game.native_sprite(batch, SPRITE, src, dst);
}
// UIRenderHelper.drawCropped samples with the original sheet's repeating UVs.
// Split at the 256px sheet boundary so a crop never reads a neighboring atlas slot.
fn crop(game: &Game<'_>, batch: &mut HudBatch, source: [i32; 2], at: [i32; 2], size: [i32; 2]) {
    let mut dy = 0;
    while dy < size[1] {
        let sy = (source[1] + dy).rem_euclid(256);
        let h = (256 - sy).min(size[1] - dy);
        let mut dx = 0;
        while dx < size[0] {
            let sx = (source[0] + dx).rem_euclid(256);
            let w = (256 - sx).min(size[0] - dx);
            quad(
                game,
                batch,
                [sx as f32, sy as f32, w as f32, h as f32],
                [(at[0] + dx) as f32, (at[1] + dy) as f32, w as f32, h as f32],
            );
            dx += w;
        }
        dy += h;
    }
}
fn frame_border(game: &Game<'_>, batch: &mut HudBatch, x: i32, y: i32, w: i32, h: i32) {
    for (src, at) in [
        ([65, 9], [x, y]),
        ([70, 9], [x + w - 4, y]),
        ([65, 19], [x, y + h - 4]),
        ([70, 19], [x + w - 4, y + h - 4]),
    ] {
        crop(game, batch, src, at, [4, 4]);
    }
    if h > 8 {
        quad(
            game,
            batch,
            [65., 14., 3., 4.],
            [x as f32, (y + 4) as f32, 3., (h - 8) as f32],
        );
        quad(
            game,
            batch,
            [71., 14., 3., 4.],
            [(x + w - 3) as f32, (y + 4) as f32, 3., (h - 8) as f32],
        );
    }
    if w > 8 {
        crop(game, batch, [0, 24], [x + 4, y], [w - 8, 3]);
        crop(game, batch, [0, 27], [x + 4, y + h - 3], [w - 8, 3]);
    }
}
fn text(game: &Game<'_>, batch: &mut HudBatch, text: &Text, at: [i32; 2], rgb: u32) {
    let [_, r, g, b] = rgb.to_be_bytes();
    batch.text_styled(
        &game.gui.atlas,
        text,
        at[0] as f32,
        at[1] as f32,
        rgba(r, g, b, 255),
        false,
    );
}

pub(super) fn draw(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if game.native_mod_screen() != Some("create") {
        return;
    }
    let Some(panel) = STATE.with_borrow(|state| state.panel) else {
        return;
    };
    let l = layout(game, [batch.width, batch.height]);
    let partial = screen.partial_tick;
    let shade = (80.0 * ((panel.ticks as f32 + partial) / 20.0).min(1.0)) as u8;
    batch.fill_gradient(
        &game.gui.atlas,
        (0., 0., batch.width, batch.height),
        rgba(16, 16, 16, shade),
        rgba(16, 16, 16, shade),
    );
    let title = title();
    let use_name = keybinds::display_name(game.settings.keys.get("key.use"));
    let tip = Text::translate("create.gui.value_settings.release_to_confirm", &[use_name]);
    let fattest = game
        .gui
        .atlas
        .font
        .width_text(&title)
        .max(game.gui.atlas.font.width_text(&tip)) as i32;
    let fat_offset = (fattest + 10 - (l.width + 13)).max(0) / 2;
    let bg_width = (l.width + 13).max(fattest + 10);
    let fade = ((panel.ticks as f32 + partial) / 4.0).clamp(0., 1.);
    let fade_width = (bg_width as f64 * f64::from(fade)) as i32;
    let fade_start = (bg_width - fade_width) / 2 - fat_offset;
    quad(
        game,
        batch,
        [80., 9., 1., 1.],
        [
            (l.x - 11 + fade_start) as f32,
            (l.y - 17) as f32,
            fade_width as f32,
            (l.height + 33) as f32,
        ],
    );
    quad(
        game,
        batch,
        [80., 9., 1., 1.],
        [
            (l.x - 10 + fade_start) as f32,
            (l.y - 18) as f32,
            (fade_width - 2) as f32,
            1.,
        ],
    );
    // The pinned source's third drawStretched call has width=zLevel=0;
    // it draws no bottom-strip pixels. Preserve that observable behavior.
    if fade_width > fattest {
        let center = l.x - 11 - fat_offset + bg_width / 2;
        text(
            game,
            batch,
            &title,
            [
                center - game.gui.atlas.font.width_text(&title) as i32 / 2,
                l.y - 14,
            ],
            0xdddddd,
        );
        text(
            game,
            batch,
            &tip,
            [
                center - game.gui.atlas.font.width_text(&tip) as i32 / 2,
                l.y + l.height + 6,
            ],
            0xdddddd,
        );
    }
    frame_border(game, batch, l.x + l.label + 14, l.y - 3, l.bar + 8, 27);
    quad(
        game,
        batch,
        [75., 9., 1., 1.],
        [
            (l.x + l.label + 17) as f32,
            l.y as f32,
            (l.bar + 2) as f32,
            21.,
        ],
    );
    for row in 0..2 {
        let y = l.y + row * 11;
        let bar_x = l.x + l.label + 18;
        crop(game, batch, [0, 31], [l.x - 4, y], [l.label + 8, 11]);
        for w in (0..l.bar).step_by(248) {
            crop(
                game,
                batch,
                [7, 0],
                [bar_x + w, y + 1],
                [(l.bar - w).min(249), 8],
            );
        }
        text(game, batch, &arrow(row), [l.x, y + 1], 0x442000);
        for milestone in 0..9 {
            crop(game, batch, [0, 0], [bar_x + milestone * 36, y + 1], [7, 8]);
        }
    }
    frame_border(game, batch, l.x - 7, l.y - 3, l.label + 14, 27);
    if panel.ticks < 1 {
        return;
    }
    let shift = game.has_shift_down();
    let hovered = l.closest(screen.mouse, shift);
    STATE.with_borrow_mut(|state| {
        let Some(current) = state.panel.as_mut() else {
            return;
        };
        if current.hovered != Some(hovered) && current.sound_cooldown == 0 {
            state.sound = Some(1.15 + (hovered.value as f32 / 256.0) * (1.5 - 1.15));
            current.sound_cooldown = 1;
        }
        current.hovered = Some(hovered);
    });
    let coordinate = l.coordinate(hovered);
    let cursor = formatted(hovered);
    let cursor_width = (game.gui.atlas.font.width_text(&cursor) as i32 / 2) * 2 + 3;
    let x = coordinate[0] as i32 - cursor_width / 2;
    let y = coordinate[1] as i32 - 7;
    crop(game, batch, [0, 9], [x - 3, y], [3, 14]);
    crop(game, batch, [4, 9], [x, y], [cursor_width, 14]);
    crop(game, batch, [61, 9], [x + cursor_width, y], [3, 14]);
    text(game, batch, &cursor, [x + 2, y + 3], 0x442000);
    if std::env::var_os("CREATE_HORIZON_TRACE").is_some() {
        thread_local! {static LAST:RefCell<String>=const{RefCell::new(String::new())};}
        let trace=serde_json::json!({"pos":panel.pos,"value":hovered.value,"row":hovered.row,"origin":[l.x,l.y],"size":[l.width,l.height],"coordinate":coordinate,"cursor":cursor.plain(),"title":title.plain(),"tip":tip.plain(),"title_width":game.gui.atlas.font.width_text(&title),"tip_width":game.gui.atlas.font.width_text(&tip)}).to_string();
        LAST.with_borrow_mut(|last| {
            if *last != trace {
                log::info!("Create motor board: {trace}");
                *last = trace;
            }
        });
    }
}

pub(super) fn mouse_press(
    game: &Game<'_>,
    _screen: &NativeScreenView<'_>,
    _button: i32,
    canceled: &mut bool,
) {
    if game.native_mod_screen() == Some("create")
        && STATE.with_borrow(|state| state.panel.is_some())
    {
        *canceled = true;
    }
}
pub(super) fn mouse_release(
    game: &Game<'_>,
    screen: &NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    let name = match button {
        0 => "key.mouse.left".to_owned(),
        1 => "key.mouse.right".to_owned(),
        2 => "key.mouse.middle".to_owned(),
        3..=31 => format!("key.mouse.{}", button + 1),
        _ => return,
    };
    if game.settings.keys.get("key.use") == name && submit(game, screen) {
        *canceled = true;
    }
}
pub(super) fn key_release(
    game: &Game<'_>,
    screen: &NativeScreenView<'_>,
    key: &str,
    canceled: &mut bool,
) {
    if game.settings.keys.get("key.use") == key && submit(game, screen) {
        *canceled = true;
    }
}
fn submit(game: &Game<'_>, screen: &NativeScreenView<'_>) -> bool {
    if game.native_mod_screen() != Some("create") {
        return false;
    }
    STATE.with_borrow_mut(|state| {
        let Some(panel) = state.panel else {
            return false;
        };
        state.submission = Some((
            panel.pos,
            layout(game, [screen.rect[2], screen.rect[3]])
                .closest(screen.mouse, game.has_shift_down()),
        ));
        state.close = true;
        true
    })
}
pub(super) fn scroll(
    game: &Game<'_>,
    screen: &NativeScreenView<'_>,
    scroll: [f64; 2],
    modifiers: u32,
    canceled: &mut bool,
) {
    if game.native_mod_screen() != Some("create")
        || !STATE.with_borrow(|state| state.panel.is_some())
        || !scroll[1].is_finite()
    {
        return;
    }
    let l = layout(game, [screen.rect[2], screen.rect[3]]);
    let shift = modifiers & 1 != 0;
    let old = l.closest(screen.mouse, shift);
    let value =
        (old.value + (scroll[1].signum() as i32) * if shift { 32 } else { 1 }).clamp(0, 256);
    if value != old.value {
        STATE.with_borrow_mut(|state| {
            state.cursor = Some(l.coordinate(Setting {
                row: old.row,
                value,
            }))
        });
        *canceled = true;
    }
}

pub(super) fn frame(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    let (cursor, sound, submission, close) = STATE.with_borrow_mut(|state| {
        (
            state.cursor.take(),
            state.sound.take(),
            state.submission.take(),
            std::mem::take(&mut state.close),
        )
    });
    if let Some(at) = cursor {
        game.cursor = (at[0], at[1]);
        if game.focused
            && !game.options.offscreen
            && let (Some(window), Some(size)) = (&game.window, game.native_physical_window_size())
        {
            let scale = HudBatch::new(size).scale;
            let _ = window.set_cursor_position(winit::dpi::PhysicalPosition::new(
                game.view_at[0] as f64 + f64::from(at[0] * scale),
                game.view_at[1] as f64 + f64::from(at[1] * scale),
            ));
        }
    }
    if let Some(pitch) = sound {
        game.audio.play("create:scroll_value", None, 0.25, pitch);
    }
    if let Some((pos, setting)) = submission {
        // Preserve raw row/magnitude (including 0) for source feedback ordering.
        let mut bytes = Vec::with_capacity(20);
        for value in pos.into_iter().chain([setting.row, setting.value]) {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        if let Err(error) = game.native_net_send_to_server("create:motor_settings_v2", &bytes) {
            log::debug!("Create motor setting: {error}");
        }
    }
    if close {
        game.native_mod_screen_close("create");
    }
    if game.native_mod_screen() != Some("create") {
        STATE.with_borrow_mut(|state| state.panel = None);
    }
}

pub(super) fn hud(game: &Game<'_>, batch: &mut HudBatch) {
    if game.ui.hide_gui || game.native_mod_screen().is_some() || !can_interact(game) {
        return;
    }
    let (ticks, have) = STATE.with_borrow(|state| (state.hover_ticks, state.last_hover_tip));
    if ticks == 0 || !have {
        return;
    }
    let alpha = if ticks > 5 {
        (11 - ticks) as f32 / 5.0
    } else {
        (ticks as f32 / 5.0).min(1.)
    };
    let x = batch.width as i32 / 2;
    let mut y = batch.height as i32 - 75 - 24;
    for (i, line) in [
        title(),
        Text::translate("create.gui.value_settings.hold_to_edit", &[]),
    ]
    .into_iter()
    .enumerate()
    {
        let [r, g, b] = if i == 0 {
            [0xfb, 0xdc, 0x7d]
        } else {
            [255, 255, 255]
        };
        batch.text_styled(
            &game.gui.atlas,
            &line,
            (x - game.gui.atlas.font.width_text(&line) as i32 / 2) as f32,
            y as f32,
            rgba(r, g, b, (255.0 * alpha) as u8),
            true,
        );
        y += 12;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_motor_coordinates_match_live_reference_at_gui_640_by_360() {
        let layout = Layout {
            x: 157,
            y: 169,
            label: 8,
            bar: 294,
            width: 326,
            height: 22,
        };
        assert_eq!(
            layout.coordinate(Setting { row: 0, value: 0 }),
            [186.5, 174.0]
        );
        assert_eq!(
            layout.coordinate(Setting { row: 1, value: 128 }),
            [330.5, 185.0]
        );
        assert_eq!(
            layout.coordinate(Setting { row: 0, value: 256 }),
            [474.5, 174.0]
        );
        assert_eq!(
            layout.closest([258.5, 174.], false),
            Setting { row: 0, value: 64 }
        );
        assert_eq!(
            layout.closest([-1000., 1000.], false),
            Setting { row: 1, value: 0 }
        );
        assert_eq!(
            layout.closest([10000., -1000.], true),
            Setting { row: 0, value: 256 }
        );
    }
}
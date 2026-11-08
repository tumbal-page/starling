// AppleSkin mod — saturation/exhaustion bar, food preview, and settings.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.

use crate::game::Game;
use crate::game::official_appleskin as aps;
use crate::game::official_appleskin_icons as icons;
use horizon_client::hud::HudBatch;
use std::sync::Mutex;

#[derive(Clone, Copy)]
struct Shown {
    food: i32,
    saturation: f32,
    eaten: aps::Eaten,
    health: f32,
    heal: f32,
}

struct Fade {
    alpha: f32,
    last_ms: u128,
    shown: Option<Shown>,
}

static FADE: Mutex<Fade> = Mutex::new(Fade {
    alpha: 0.0,
    last_ms: 0,
    shown: None,
});

fn wall_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

fn advance(current: Option<Shown>, now: u128) -> (f32, Option<Shown>) {
    let mut fade = FADE.lock().unwrap_or_else(|e| e.into_inner());
    let dt = if fade.last_ms == 0 {
        0.0
    } else {
        now.saturating_sub(fade.last_ms).min(100) as f32
    };
    fade.last_ms = now;
    fade.alpha = aps::approach(fade.alpha, if current.is_some() { 1.0 } else { 0.0 }, dt);
    if let Some(s) = current {
        fade.shown = Some(s);
    } else if fade.alpha <= 0.0 {
        fade.shown = None;
    }
    (fade.alpha, fade.shown)
}

fn anchors(batch: &HudBatch) -> (f32, f32, f32) {
    let w = batch.width as f32;
    (
        w / 2.0 + aps::FOOD_RIGHT,
        w / 2.0 - aps::FOOD_RIGHT,
        batch.height as f32 - aps::FOOD_ROW_BOTTOM,
    )
}

impl Game<'_> {
    fn appleskin_options(&self) -> aps::Options {
        aps::Options::from_settings(|key| self.native_setting(aps::ID, key))
    }

    fn appleskin_sprite(&self, batch: &mut HudBatch, x: f32, y: f32, grid: &[&str], half: bool, look: (f32, f32)) {
        let (dim, alpha) = look;
        let cols = if half { aps::ICON_HALF_COLS } else { aps::ICON as usize };
        for (row, line) in grid.iter().enumerate() {
            let line = line.as_bytes();
            let mut col = 0;
            while col < cols {
                let ch = line[col];
                let start = col;
                while col < cols && line[col] == ch {
                    col += 1;
                }
                if ch != b'.' {
                    let argb = aps::fade(aps::dim(icons::color(ch), dim), alpha);
                    let rect = [x + start as f32, y + row as f32, (col - start) as f32, 1.0];
                    self.native_fill(batch, rect, argb);
                }
            }
        }
    }

    fn appleskin_preview(&self, batch: &mut HudBatch, s: Shown, level: f32) {
        let (right, left, top) = anchors(batch);
        let look = (aps::PREVIEW_DIM, level);

        let food: Vec<(usize, bool)> = aps::food_icons(s.food, s.eaten.food).collect();
        for &(i, half) in &food {
            let x = right - aps::ICON - aps::ICON_STEP * i as f32;
            self.appleskin_sprite(batch, x, top, icons::FOOD, half, look);
        }

        let from = aps::saturation_gain_start(s.saturation);
        for (i, frame) in aps::saturation_icons(s.eaten.saturation) {
            if i < from || food.iter().any(|&(j, _)| j == i) {
                continue;
            }
            let x = right - aps::ICON - aps::ICON_STEP * i as f32;
            self.appleskin_sprite(batch, x, top, icons::SATURATION[frame], false, look);
        }

        if s.health > 0.0 && s.health < aps::MAX_HEALTH {
            let faint = (aps::PREVIEW_DIM, level * aps::CEILING_ALPHA);
            for (i, half) in aps::heart_icons(s.health, aps::MAX_HEALTH) {
                let x = left + aps::ICON_STEP * i as f32;
                self.appleskin_sprite(batch, x, top, icons::HEART, half, faint);
            }
        }
        if s.heal > 0.0 {
            let modified = (s.health + s.heal).min(aps::MAX_HEALTH);
            for (i, half) in aps::heart_icons(s.health, modified) {
                let x = left + aps::ICON_STEP * i as f32;
                self.appleskin_sprite(batch, x, top, icons::HEART, half, look);
            }
        }
    }

    pub fn appleskin_bar(&self, batch: &mut HudBatch) {
        if self.ui.hide_gui
            || !self.native_on(aps::ID)
            || !self.native_playing()
            || self.creative
            || matches!(self.mode, horizon_client::command::args::GameMode::Spectator)
        {
            return;
        }
        let Some(h) = self.native_hunger() else {
            return;
        };
        let opts = self.appleskin_options();
        let (right, _, top) = anchors(batch);
        let shown_exhaustion = aps::exhaustion_shown(h.exhaustion_visible, h.exhaustion);

        if opts.exhaustion {
            if let Some(e) = shown_exhaustion {
                let width = aps::strip_width(e);
                let first = aps::STRIP_WIDTH - width;
                for col in first..aps::STRIP_WIDTH {
                    let x = right - width as f32 + (col - first) as f32;
                    for y in 0..aps::STRIP_HEIGHT {
                        if (col + y) % 2 == 1 {
                            self.native_fill(batch, [x, top + y as f32, 1.0, 1.0], aps::STRIP_COLOR);
                        }
                    }
                }
            }
        }

        if opts.saturation {
            for (i, frame) in aps::saturation_icons(h.saturation) {
                let x = right - aps::ICON - aps::ICON_STEP * i as f32;
                self.appleskin_sprite(batch, x, top, icons::SATURATION[frame], false, (1.0, 1.0));
            }
        }

        let current = match h.held {
            Some((nutrition, added)) if opts.preview => {
                let eaten = aps::eaten(h.food, h.saturation, nutrition, added);
                let allowed = h.food < 18
                    && h.health > 0.0
                    && h.health < aps::MAX_HEALTH
                    && !h.peaceful
                    && !h.regeneration_blocked;
                let heal = if allowed {
                    aps::estimated_heal(
                        eaten.food,
                        eaten.saturation,
                        shown_exhaustion.unwrap_or(0.0),
                        h.natural_regeneration,
                    ) + h.held_regeneration.map_or(0.0, |(d, a)| aps::regeneration_heal(d, a))
                } else {
                    0.0
                };
                Some(Shown {
                    food: h.food,
                    saturation: h.saturation,
                    eaten,
                    health: h.health,
                    heal,
                })
            }
            _ => None,
        };

        let now = wall_millis();
        let (alpha, shown) = advance(current, now);
        let level = aps::brightness(opts.animation, current.is_some(), alpha, now);
        if let Some(s) = shown.filter(|_| level > 0.0) {
            self.appleskin_preview(batch, s, level);
        }
    }

    pub fn appleskin_settings(&self) -> Vec<String> {
        aps::labels(self.appleskin_options())
    }

    pub fn appleskin_settings_click(&mut self, button: usize) {
        let options = self.appleskin_options();
        if button == 3 {
            self.native_set_setting(aps::ID, aps::KEYS[3], options.animation.next().code());
            return;
        }
        let Some(key) = aps::KEYS.get(button) else {
            return;
        };
        let on = options.get(button);
        self.native_set_setting(aps::ID, key, aps::flag_text(!on));
    }
}

/// Reset profile-scoped state when Android returns from its launcher.
pub fn reset() {
    if let Ok(mut fade) = FADE.lock() {
        *fade = Fade {
            alpha: 0.0,
            last_ms: 0,
            shown: None,
        };
    }
}

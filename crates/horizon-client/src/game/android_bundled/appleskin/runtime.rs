// AppleSkin mod — saturation, exhaustion and food preview overlay.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.
// Source: appleskin-0.1.3.hnmod (horizon-native hooks 139, commit 02c8607e).
// Android static-bundle adaptation.

#[allow(dead_code)]
pub const ID: &str = "appleskin";
#[allow(dead_code)]
pub const NAME: &str = "AppleSkin";
#[allow(dead_code)]
pub const VERSION: &str = "0.1.3";
#[allow(dead_code)]
pub const DESCRIPTION: &str = "Saturation, exhaustion and the food in your hand, drawn over the food bar as the original AppleSkin does.";

pub const MAX_FOOD: i32 = 20;
pub const MAX_HEALTH: f32 = 20.0;
pub const MAX_EXHAUSTION: f32 = 4.0;
pub const REGEN_EXHAUSTION: f32 = 6.0;

pub const FOOD_RIGHT: f32 = 91.0;
pub const FOOD_ROW_BOTTOM: f32 = 39.0;
pub const ICON: f32 = 9.0;
pub const ICON_STEP: f32 = 8.0;
pub const ICON_HALF_COLS: usize = 4;

pub const STRIP_WIDTH: usize = 81;
pub const STRIP_HEIGHT: usize = 9;
pub const STRIP_COLOR: u32 = 0x8F28_2828;

pub const PREVIEW_DIM: f32 = 0.7;
pub const CEILING_ALPHA: f32 = 0.35;
pub const FADE_MS: f32 = 300.0;
pub const PULSE_MS: u128 = 1000;
pub const PULSE_DEPTH: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animation {
    Fade,
    Pulse,
    Both,
}

impl Animation {
    pub fn for_code(code: &str) -> Self {
        match code {
            "fade" => Self::Fade,
            "pulse" => Self::Pulse,
            "both" => Self::Both,
            _ => Self::Both,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Fade => "fade",
            Self::Pulse => "pulse",
            Self::Both => "both",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Fade => Self::Pulse,
            Self::Pulse => Self::Both,
            Self::Both => Self::Fade,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fade => "fade",
            Self::Pulse => "pulse",
            Self::Both => "fade + pulse",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eaten {
    pub food: i32,
    pub saturation: f32,
}

pub fn eaten(food: i32, saturation: f32, nutrition: u32, added: f32) -> Eaten {
    let food = (food + nutrition as i32).clamp(0, MAX_FOOD);
    Eaten {
        food,
        saturation: (saturation + added).clamp(0.0, food as f32),
    }
}

pub fn exhaustion_shown(visible: bool, exhaustion: f32) -> Option<f32> {
    (visible && exhaustion.is_finite()).then(|| exhaustion.clamp(0.0, MAX_EXHAUSTION))
}

pub fn strip_width(exhaustion: f32) -> usize {
    (exhaustion.clamp(0.0, MAX_EXHAUSTION) / MAX_EXHAUSTION * STRIP_WIDTH as f32) as usize
}

pub fn saturation_frame(share: f32) -> usize {
    if share >= 1.0 {
        3
    } else if share > 0.5 {
        2
    } else if share > 0.25 {
        1
    } else {
        0
    }
}

pub fn saturation_gain_start(saturation: f32) -> usize {
    (saturation.clamp(0.0, MAX_FOOD as f32) / 2.0).floor() as usize
}

pub fn saturation_icons(saturation: f32) -> impl Iterator<Item = (usize, usize)> {
    let s = saturation.clamp(0.0, MAX_FOOD as f32) / 2.0;
    (0..s.ceil() as usize).map(move |i| (i, saturation_frame(s - i as f32)))
}

pub fn food_icons(from: i32, to: i32) -> impl Iterator<Item = (usize, bool)> {
    (from / 2..(to + 1) / 2).map(move |i| (i as usize, i * 2 + 1 == to))
}

pub fn heart_icons(from: f32, to: f32) -> impl Iterator<Item = (usize, bool)> {
    let start = (from.ceil() / 2.0).floor() as i32;
    let end = (to.ceil() / 2.0).ceil() as i32;
    let whole = to.ceil() as i32;
    (start..end).map(move |i| (i as usize, i * 2 + 1 == whole))
}

pub fn approach(alpha: f32, target: f32, dt_ms: f32) -> f32 {
    let step = dt_ms / FADE_MS;
    if alpha < target {
        (alpha + step).min(target)
    } else {
        (alpha - step).max(target)
    }
}

pub fn brightness(animation: Animation, present: bool, alpha: f32, now_ms: u128) -> f32 {
    let phase = (now_ms % PULSE_MS) as f32 / PULSE_MS as f32 * std::f32::consts::TAU;
    let pulse = 0.5 - 0.5 * phase.cos();
    match animation {
        Animation::Fade => alpha,
        Animation::Pulse => {
            if present {
                1.0 - PULSE_DEPTH * pulse
            } else {
                0.0
            }
        }
        Animation::Both => {
            let ramp = ((alpha - 0.9) / 0.1).clamp(0.0, 1.0);
            alpha * (1.0 - PULSE_DEPTH * pulse * ramp)
        }
    }
}

pub fn fade(argb: u32, alpha: f32) -> u32 {
    let a = ((argb >> 24) as f32 * alpha) as u32;
    (a << 24) | (argb & 0x00FF_FFFF)
}

pub fn dim(argb: u32, factor: f32) -> u32 {
    let scale = |c: u32| ((c as f32) * factor) as u32;
    let r = scale((argb >> 16) & 0xFF);
    let g = scale((argb >> 8) & 0xFF);
    let b = scale(argb & 0xFF);
    (argb & 0xFF00_0000) | (r << 16) | (g << 8) | b
}

pub fn regeneration_heal(duration: i32, amplifier: i32) -> f32 {
    (duration / (50 >> (amplifier & 31)).max(1)) as f32
}

pub fn estimated_heal(mut food: i32, mut saturation: f32, mut exhaustion: f32, natural: bool) -> f32 {
    if !natural || !exhaustion.is_finite() || !saturation.is_finite() {
        return 0.0;
    }
    let mut health = 0.0;
    while food >= 18 {
        while exhaustion > MAX_EXHAUSTION {
            exhaustion -= MAX_EXHAUSTION;
            if saturation > 0.0 {
                saturation = (saturation - 1.0).max(0.0);
            } else {
                food -= 1;
            }
        }
        if food >= 20 && saturation > f32::MIN_POSITIVE {
            let limited = saturation.min(REGEN_EXHAUSTION);
            let until = MAX_EXHAUSTION.next_up() - exhaustion;
            let iterations = ((until / limited).ceil() as i32).max(1) as f32;
            health += limited / REGEN_EXHAUSTION * iterations;
            exhaustion += limited * iterations;
        } else if food >= 18 {
            health += 1.0;
            exhaustion += REGEN_EXHAUSTION;
        }
    }
    health
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub saturation: bool,
    pub exhaustion: bool,
    pub preview: bool,
    pub animation: Animation,
}

pub const KEYS: [&str; 4] = ["numbers", "bars", "preview", "animation"];

impl Default for Options {
    fn default() -> Self {
        Self {
            saturation: true,
            exhaustion: true,
            preview: true,
            animation: Animation::Both,
        }
    }
}

impl Options {
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> Self {
        let on = |key: &str| !matches!(get(key).as_deref(), Some("off"));
        Self {
            saturation: on(KEYS[0]),
            exhaustion: on(KEYS[1]),
            preview: on(KEYS[2]),
            animation: Animation::for_code(get(KEYS[3]).as_deref().unwrap_or("")),
        }
    }

    pub fn get(&self, index: usize) -> bool {
        match index {
            0 => self.saturation,
            1 => self.exhaustion,
            _ => self.preview,
        }
    }
}

pub fn flag_text(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

pub fn labels(options: Options) -> Vec<String> {
    let names = ["Saturation icons", "Exhaustion strip", "Held food preview"];
    let mut out: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{name}: {}", flag_text(options.get(i))))
        .collect();
    out.push(format!("Animation: {}", options.animation.name()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eating_caps_food_and_saturation_as_the_game_does() {
        let e = eaten(18, 1.0, 6, 3.6);
        assert_eq!(e.food, 20);
        assert!((e.saturation - 4.6).abs() < 1e-5);
        let e = eaten(0, 0.0, 0, 0.0);
        assert_eq!((e.food, e.saturation), (0, 0.0));
    }

    #[test]
    fn saturation_never_exceeds_the_food_left() {
        let e = eaten(4, 3.0, 2, 10.0);
        assert_eq!(e.food, 6);
        assert_eq!(e.saturation, 6.0);
    }

    #[test]
    fn exhaustion_is_shown_only_where_the_game_counts_it() {
        assert_eq!(exhaustion_shown(true, 2.5), Some(2.5));
        assert_eq!(exhaustion_shown(false, 2.5), None);
        assert_eq!(exhaustion_shown(true, f32::NAN), None);
        assert_eq!(exhaustion_shown(true, 7.0), Some(MAX_EXHAUSTION));
    }

    #[test]
    fn strip_shows_the_exhaustion_share_of_81_pixels() {
        assert_eq!(strip_width(0.0), 0);
        assert_eq!(strip_width(2.0), 40);
        assert_eq!(strip_width(4.0), 81);
    }

    #[test]
    fn saturation_frames_change_at_the_original_thresholds() {
        assert_eq!(saturation_frame(1.0), 3);
        assert_eq!(saturation_frame(0.75), 2);
        assert_eq!(saturation_frame(0.5), 1);
        assert_eq!(saturation_frame(0.25), 0);
    }

    #[test]
    fn saturation_icons_cover_the_saturation_in_pairs() {
        assert_eq!(saturation_icons(0.0).count(), 0);
        let frames: Vec<usize> = saturation_icons(5.0).map(|(_, f)| f).collect();
        assert_eq!(frames, vec![3, 3, 1]);
    }

    #[test]
    fn saturation_gain_redraws_the_partly_filled_icon() {
        assert_eq!(saturation_gain_start(4.6), 2);
        assert_eq!(saturation_gain_start(4.0), 2);
        assert_eq!(saturation_gain_start(0.0), 0);
    }

    #[test]
    fn food_icons_cover_the_food_points_restored() {
        let v: Vec<_> = food_icons(10, 16).collect();
        assert_eq!(v, vec![(5, false), (6, false), (7, false)]);
        let v: Vec<_> = food_icons(10, 15).collect();
        assert_eq!(v, vec![(5, false), (6, false), (7, true)]);
        assert_eq!(food_icons(20, 20).count(), 0);
    }

    #[test]
    fn heart_icons_mark_the_half_heart_at_the_end() {
        let v: Vec<_> = heart_icons(10.0, 13.0).collect();
        assert_eq!(v, vec![(5, false), (6, true)]);
        assert_eq!(heart_icons(20.0, 20.0).count(), 0);
    }

    #[test]
    fn dim_darkens_colour_and_keeps_alpha() {
        assert_eq!(dim(0xFFFF_FFFF, 0.5), 0xFF7F_7F7F);
        assert_eq!(dim(0x80FF_0000, 1.0), 0x80FF_0000);
    }

    #[test]
    fn fade_moves_toward_the_target_and_stops_there() {
        assert!((approach(0.0, 1.0, FADE_MS / 2.0) - 0.5).abs() < 1e-6);
        assert_eq!(approach(0.9, 1.0, 10_000.0), 1.0);
        assert_eq!(approach(0.5, 0.0, 10_000.0), 0.0);
    }

    #[test]
    fn fade_scales_only_the_alpha_byte() {
        assert_eq!(fade(0xFF10_2030, 0.5), 0x7F10_2030);
        assert_eq!(fade(0xFF10_2030, 0.0) >> 24, 0);
    }

    #[test]
    fn fade_animation_is_the_plain_alpha() {
        assert_eq!(brightness(Animation::Fade, true, 0.4, 0), 0.4);
        assert_eq!(brightness(Animation::Fade, false, 0.4, 500), 0.4);
    }

    #[test]
    fn pulse_breathes_between_full_and_dimmed_while_present() {
        assert!((brightness(Animation::Pulse, true, 0.0, 0) - 1.0).abs() < 1e-5);
        assert!((brightness(Animation::Pulse, true, 0.0, 500) - 0.6).abs() < 1e-5);
        assert_eq!(brightness(Animation::Pulse, false, 1.0, 0), 0.0);
    }

    #[test]
    fn both_fades_in_first_then_pulses() {
        assert_eq!(brightness(Animation::Both, true, 0.5, 500), 0.5);
        assert!((brightness(Animation::Both, true, 1.0, 500) - 0.6).abs() < 1e-5);
    }

    #[test]
    fn animation_cycles_and_round_trips_its_key() {
        for a in [Animation::Fade, Animation::Pulse, Animation::Both] {
            assert_eq!(Animation::for_code(a.code()), a);
        }
        let mut a = Animation::Both;
        let mut seen = Vec::new();
        for _ in 0..3 {
            a = Animation::for_code(a.next().code());
            seen.push(a);
        }
        assert_eq!(seen, vec![Animation::Fade, Animation::Pulse, Animation::Both]);
        assert_eq!(Animation::for_code("nonsense"), Animation::Both);
        assert_eq!(Animation::Fade.next().next().next(), Animation::Fade);
    }

    #[test]
    fn regeneration_heal_follows_the_game_tick_count() {
        assert_eq!(regeneration_heal(900, 0), 18.0);
        assert_eq!(regeneration_heal(900, 1), 36.0);
        assert_eq!(regeneration_heal(100, 6), 100.0);
    }

    #[test]
    fn slow_regen_heals_one_health_per_iteration_at_eighteen_food() {
        assert_eq!(estimated_heal(18, 0.0, 0.0, true), 1.0);
        assert_eq!(estimated_heal(17, 0.0, 0.0, true), 0.0);
    }

    #[test]
    fn no_natural_regeneration_means_no_estimate() {
        assert_eq!(estimated_heal(20, 10.0, 0.0, false), 0.0);
    }

    #[test]
    fn estimate_is_zero_for_nonfinite_input_and_stays_finite() {
        assert_eq!(estimated_heal(20, 5.0, f32::NAN, true), 0.0);
        assert!(estimated_heal(20, 10.0, 3.9, true).is_finite());
    }

    #[test]
    fn settings_default_to_on_and_read_off() {
        let all = Options::from_settings(|_| None);
        assert_eq!(all, Options::default());
        let some = Options::from_settings(|key| (key == "bars").then(|| "off".to_owned()));
        assert!(!some.exhaustion);
        assert!(some.saturation && some.preview);
    }

    #[test]
    fn animation_setting_is_read_from_its_key() {
        let both = Options::from_settings(|key| (key == "animation").then(|| "both".to_owned()));
        assert_eq!(both.animation, Animation::Both);
    }

    #[test]
    fn labels_follow_keys() {
        let all = labels(Options::default());
        assert_eq!(all.len(), KEYS.len());
        assert_eq!(all[0], "Saturation icons: on");
        assert_eq!(all[3], "Animation: fade + pulse");
        let off = Options { exhaustion: false, ..Options::default() };
        assert_eq!(labels(off)[1], "Exhaustion strip: off");
    }

    #[test]
    fn fade_and_pulse_is_the_default_animation() {
        assert_eq!(Options::default().animation, Animation::Both);
        assert_eq!(Options::from_settings(|_| None).animation, Animation::Both);
    }
}

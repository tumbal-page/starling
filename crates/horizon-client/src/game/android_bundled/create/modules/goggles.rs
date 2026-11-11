// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Create 6.0.10: GoggleOverlayRenderer, RemovedGuiUtils, KineticBlockEntity,
// GeneratingKineticBlockEntity, GaugeBlockEntity, Speed/StressGaugeBlockEntity,
// IRotate.{SpeedLevel,StressImpact}, TooltipHelper and Catnip LangBuilder.
// Catnip's bundled Ponder 1.0.82 license is MIT (LICENSE_Ponder).
use super::Game;
use crate::containers::BlockEntity;
use crate::game::create_port::number;
use create_engine::kinetics::{Kind, Pos};
use horizon_client::{
    hud::{HudBatch, rgba},
    text::{self, Style, Text},
};
use horizon_world::nbt::{Compound, Tag};
use std::cell::{Cell, RefCell};

thread_local! {
    // The original counter advances per render call, and does not reset when
    // moving directly between two blocks that both supply information.
    static HOVER_FRAMES: Cell<u32> = const { Cell::new(0) };
    static LAST_TRACE: RefCell<String> = const { RefCell::new(String::new()) };
}

pub(super) fn tr(key: &str) -> Text {
    Text::translate(&format!("create.{key}"), &[])
}

// LangNumberFormat: JDK 21 NumberFormat, maxFractionDigits=2, grouping=true,
// Mth.equal(value,0), then replace U+00A0 with a normal space. These symbol
// groups were queried from the pinned reference JDK 21.0.7 for all supported
// upstream Create locale IDs. They are locale data, not translated assets.
fn localized_number(value: f64, locale: &str) -> String {
    if value.is_nan() {
        return match locale {
            "ar_sa" => "ليس رقم",
            "fa_ir" => "ناعدد",
            "fi_fi" => "epäluku",
            "hy_am" => "ՈչԹ",
            "kk_kz" => "сан емес",
            "ru_ru" => "не число",
            "zh_tw" => "非數值",
            _ => "NaN",
        }
        .to_owned();
    }
    let (group, decimal, zero, minus) = match locale {
        "af_za" | "be_by" | "bg_bg" | "cs_cz" | "eo_uy" | "fr_ca" | "hu_hu" | "hy_am" | "kk_kz"
        | "pl_pl" | "pt_pt" | "ru_ru" | "uk_ua" => (' ', ',', '0', "-"),
        "et_ee" | "fi_fi" | "ksh" | "lt_lt" | "lt_lv" | "no_no" | "sv_se" => (' ', ',', '0', "−"),
        "ca_es" | "da_dk" | "de_de" | "el_gr" | "es_cl" | "es_es" | "id_id" | "is_is" | "it_it"
        | "nl_nl" | "pt_br" | "ro_ro" | "tr_tr" | "vi_vn" => ('.', ',', '0', "-"),
        "eu_es" | "hr_hr" => ('.', ',', '0', "−"),
        "fr_fr" => ('\u{202f}', ',', '0', "-"),
        "ar_sa" => ('\u{66c}', '\u{66b}', '\u{660}', "\u{61c}-"),
        "fa_ir" => ('\u{66c}', '\u{66b}', '\u{6f0}', "\u{200e}−"),
        "he_il" => (',', '.', '0', "\u{200e}-"),
        _ => (',', '.', '0', "-"),
    };
    let value = if value.abs() < 1e-5 { 0.0 } else { value };
    let negative = value.is_sign_negative();
    if value.is_infinite() {
        return format!("{}∞", if negative { minus } else { "" });
    }
    let fixed = format!("{:.2}", value.abs());
    let fixed = fixed.trim_end_matches('0').trim_end_matches('.');
    let (integer, fraction) = fixed.split_once('.').unwrap_or((fixed, ""));
    let mut result = String::new();
    if negative {
        result.push_str(minus);
    }
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index).is_multiple_of(3) {
            result.push(group);
        }
        result.push(char::from_u32(zero as u32 + digit as u32 - '0' as u32).unwrap_or(digit));
    }
    if !fraction.is_empty() {
        result.push(decimal);
        for digit in fraction.chars() {
            result.push(char::from_u32(zero as u32 + digit as u32 - '0' as u32).unwrap_or(digit));
        }
    }
    result
}

pub(super) fn numeral(value: f64) -> Text {
    let language = horizon_client::lang::current();
    Text::literal(localized_number(
        value,
        language.as_ref().map_or("en_us", |language| &language.code),
    ))
}

fn bar(filled: usize) -> String {
    format!(" {}{} ", "█".repeat(filled), "▒".repeat(3 - filled))
}

pub(super) fn indent(game: &Game<'_>, value: Text, extra: usize) -> Text {
    let space = game.native_text_width(" ");
    let count = (4.0 * (4 + extra) as f32 / space).ceil() as usize;
    Text::literal(" ".repeat(count)).append(value)
}

fn named(nbt: &Compound, key: &str, expected: &str) -> bool {
    matches!(nbt.get(&key.into()),Some(Tag::String(value)) if value.to_string().ok().as_deref()==Some(expected))
}

fn theoretical(nbt: &Compound) -> f32 {
    nbt.get(&"create_horizon".into())
        .and_then(|data| data.get("TheoreticalSpeed"))
        .and_then(|value| match value {
            Tag::Float(bits) => Some(f32::from_bits(*bits)),
            _ => None,
        })
        .unwrap_or_else(|| number(nbt, "Speed"))
}

fn speed_text(speed: f32, overstressed: bool) -> Text {
    let speed = speed.abs();
    let (filled, name, color) = if speed >= 100.0 {
        (3, "fast", text::LIGHT_PURPLE)
    } else if speed >= 30.0 {
        (2, "medium", text::AQUA)
    } else if speed >= 1.0 {
        (1, "slow", text::GREEN)
    } else {
        (0, "none", text::DARK_GRAY)
    };
    Text::literal(bar(filled))
        .append(tr(&format!("tooltip.speedRequirement.{name}")))
        .append(" (")
        .append(numeral(f64::from(speed)))
        .append(" ")
        .append(tr("generic.unit.rpm"))
        .append(") ")
        .with_style(&Style {
            color: Some(if overstressed { text::DARK_GRAY } else { color }),
            strikethrough: overstressed.then_some(true),
            ..Style::default()
        })
}

fn stress_level(fraction: f64) -> (usize, &'static str, u32) {
    if fraction > 1.0 {
        (3, "overstressed", text::RED)
    } else if fraction > 0.75 {
        (3, "high", text::GOLD)
    } else if fraction > 0.5 {
        (2, "medium", text::YELLOW)
    } else {
        (1, "low", text::GREEN)
    }
}

fn stress_text(fraction: f64) -> Text {
    let (filled, name, color) = stress_level(fraction);
    Text::literal(bar(filled))
        .append(tr(&format!("tooltip.stressImpact.{name}")))
        .append(format!(" ({}%) ", (fraction * 100.0) as i32))
        .with_style(&Style::color(color))
}

fn current_stress(value: f32) -> Text {
    numeral(f64::from(value))
        .append(tr("generic.unit.stress"))
        .with_style(&Style::color(text::AQUA))
        .append(Text::colored(" ", text::AQUA))
        .append(tr("gui.goggles.at_current_speed").with_style(&Style::color(text::DARK_GRAY)))
}

fn providers(game: &Game<'_>, pos: Pos, block: &str, nbt: &Compound, goggles: bool) -> Vec<Text> {
    if block == "create:basin" {
        return if goggles {
            super::create_port_basin_tooltip::lines(game, nbt)
        } else {
            Vec::new()
        };
    }
    let Some(node) = crate::game::create_port::node(game, pos) else {
        return Vec::new();
    };
    let speed = theoretical(nbt);
    let overstressed = named(nbt, "NetworkStatus", "Overstressed");
    let mut lines = Vec::new();
    let mut add = |value, extra| lines.push(indent(game, value, extra));
    if goggles {
        match block {
            "create:speedometer" => {
                add(tr("gui.gauge.info_header"), 0);
                add(
                    tr("gui.speedometer.title").with_style(&Style::color(text::GRAY)),
                    0,
                );
                add(speed_text(speed, overstressed), 0);
            }
            "create:stressometer" => {
                add(tr("gui.gauge.info_header"), 0);
                add(
                    tr("gui.stressometer.title").with_style(&Style::color(text::GRAY)),
                    0,
                );
                if speed == 0.0 {
                    add(
                        Text::literal(bar(0))
                            .append(tr("gui.stressometer.no_rotation"))
                            .with_style(&Style::color(text::DARK_GRAY)),
                        0,
                    );
                } else {
                    let capacity = f64::from(number(nbt, "Capacity"));
                    let stress = f64::from(number(nbt, "Stress"));
                    let fraction = stress / if capacity == 0.0 { 1.0 } else { capacity };
                    add(stress_text(fraction), 0);
                    add(
                        tr("gui.stressometer.capacity").with_style(&Style::color(text::GRAY)),
                        0,
                    );
                    let remaining = capacity - stress;
                    let mut value = numeral(remaining)
                        .append(tr("generic.unit.stress"))
                        .with_style(&Style::color(stress_level(fraction).2));
                    if remaining != capacity {
                        value = value.append(Text::colored(" / ", text::GRAY)).append(
                            numeral(capacity)
                                .append(tr("generic.unit.stress"))
                                .with_style(&Style::color(text::DARK_GRAY)),
                        );
                    }
                    add(value, 1);
                }
            }
            _ => {
                let impact = match node.kind {
                    Kind::Press => 8.0,
                    Kind::Millstone | Kind::Mixer => 4.0,
                    Kind::Consumer { impact, .. } => impact,
                    _ => 0.0,
                };
                if impact != 0.0 {
                    add(tr("gui.goggles.kinetic_stats"), 0);
                    add(
                        tr("tooltip.stressImpact").with_style(&Style::color(text::GRAY)),
                        0,
                    );
                    add(current_stress(impact * speed.abs()), 1);
                }
                let generator = match node.kind {
                    Kind::Motor { rpm, capacity, .. } | Kind::WaterWheel { rpm, capacity } => {
                        Some((rpm, capacity))
                    }
                    _ => None,
                };
                if let Some((generated, mut capacity)) =
                    generator.filter(|(_, capacity)| *capacity != 0.0)
                {
                    add(tr("gui.goggles.generator_stats"), 0);
                    add(
                        tr("tooltip.capacityProvided").with_style(&Style::color(text::GRAY)),
                        0,
                    );
                    if speed != generated && speed != 0.0 {
                        capacity *= generated / speed;
                    }
                    add(current_stress((capacity * speed).abs()), 1);
                }
            }
        }
    }
    // IHaveHoveringInformation is independent of goggles. Source adds a blank
    // separator only if the first provider contributed information.
    let running = crate::game::create_port::running_speed(game, nbt);
    let minimum = if matches!(node.kind, Kind::Mixer) {
        create_engine::basin::DEFAULT_MIXER_MINIMUM_RPM
    } else {
        0.0 // IRotate defaults to SpeedLevel.NONE.
    };
    let not_fast_enough = running != 0.0
        && !matches!(
            running.abs().partial_cmp(&minimum),
            Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
        );
    if overstressed || not_fast_enough {
        if !lines.is_empty() {
            lines.push(Text::empty());
        }
        let (heading, hint) = if overstressed {
            (
                tr("gui.stressometer.overstressed"),
                tr("gui.contraptions.network_overstressed"),
            )
        } else {
            (
                tr("tooltip.speedRequirement"),
                Text::translate(
                    "create.gui.contraptions.not_fast_enough",
                    &[Text::translate(
                        &format!("block.{}", block.replace(':', ".")),
                        &[],
                    )],
                ),
            )
        };
        lines.push(indent(
            game,
            heading.with_style(&Style::color(text::GOLD)),
            0,
        ));
        for line in warning_lines(game, &hint.plain()) {
            lines.push(indent(game, line, 0));
        }
    }
    lines
}

// TooltipHelper.cutTextComponent, 200 px, gray/white underscore highlights.
// Java BreakIterator's word+trailing-space boundaries agree for the supported
// English/Russian warning sentences; other scripts retain native text wrapping.
fn warning_lines(game: &Game<'_>, message: &str) -> Vec<Text> {
    let font = &game.gui.atlas.font;
    let mut strings = Vec::new();
    let mut line = String::new();
    let mut width = 0.0;
    for word in message.split_inclusive(char::is_whitespace) {
        let advance = font.width(&word.replace('_', ""));
        if width + advance > 200.0 {
            if width > 0.0 {
                strings.push(std::mem::take(&mut line));
                width = 0.0;
            } else {
                strings.push(word.to_owned());
                continue;
            }
        }
        line.push_str(word);
        width += advance;
    }
    if width > 0.0 {
        strings.push(line);
    }
    let mut highlight = false;
    strings
        .into_iter()
        .map(|line| {
            let mut result = Text::empty();
            for part in line.split('_') {
                result.push(Text::colored(
                    part,
                    if highlight { text::WHITE } else { text::GRAY },
                ));
                highlight = !highlight;
            }
            highlight = !highlight;
            result
        })
        .collect()
}

fn color(argb: u32, fade: f32) -> u32 {
    let [alpha, red, green, blue] = argb.to_be_bytes();
    rgba(red, green, blue, (f32::from(alpha) * fade) as u8)
}

#[derive(Debug)]
struct Tooltip {
    lines: Vec<Text>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    title_lines: usize,
}

// RemovedGuiUtils.drawHoveringText: two-stage horizontal placement/wrapping,
// then vertical clamping. The 2 px title gap is not applied between all lines.
fn tooltip(game: &Game<'_>, lines: Vec<Text>, at: [f32; 2], screen: [f32; 2]) -> Tooltip {
    let font = &game.gui.atlas.font;
    let [mouse_x, mouse_y] = at;
    let [screen_width, screen_height] = screen;
    let mut width = lines
        .iter()
        .map(|line| font.width_text(line))
        .fold(0.0, f32::max);
    let mut x = mouse_x + 12.0;
    let mut wrap = false;
    if x + width + 4.0 > screen_width {
        x = mouse_x - 16.0 - width;
        if x < 4.0 {
            width = if mouse_x > screen_width / 2.0 {
                mouse_x - 20.0
            } else {
                screen_width - 16.0 - mouse_x
            };
            wrap = true;
        }
    }
    let mut title_lines = 1;
    let lines = if wrap {
        let mut result = Vec::new();
        width = width.max(1.0);
        let wrap_width = width;
        width = 0.0;
        for (index, line) in lines.into_iter().enumerate() {
            let split = font.split_formatted(&line, wrap_width);
            if index == 0 {
                title_lines = split.len();
            }
            for line in split {
                width = width.max(font.width_text(&line));
                result.push(line);
            }
        }
        x = if mouse_x > screen_width / 2.0 {
            mouse_x - 16.0 - width
        } else {
            mouse_x + 12.0
        };
        result
    } else {
        lines
    };
    let height = 8.0
        + (lines.len().saturating_sub(1) * 10) as f32
        + if lines.len() > title_lines { 2.0 } else { 0.0 };
    let mut y = mouse_y - 12.0;
    if y < 4.0 {
        y = 4.0;
    } else if y + height + 4.0 > screen_height {
        y = screen_height - height - 4.0;
    }
    Tooltip {
        lines,
        x,
        y,
        width,
        height,
        title_lines,
    }
}

fn background(game: &Game<'_>, batch: &mut HudBatch, tooltip: &Tooltip, slide: f32, fade: f32) {
    let Tooltip {
        x,
        y,
        width,
        height,
        ..
    } = *tooltip;
    let x = x + slide;
    // BoxElement vanilla colors; GoggleOverlayRenderer scales only the
    // background's alpha by .75, then all three colors by hover fade.
    let background = color(0xb4_100010, fade);
    let top = color(0x50_5000ff, fade);
    let bottom = color(0x50_28007f, fade);
    for rect in [
        [x - 3.0, y - 4.0, width + 6.0, 1.0],
        [x - 3.0, y + height + 3.0, width + 6.0, 1.0],
        [x - 3.0, y - 3.0, width + 6.0, height + 6.0],
        [x - 4.0, y - 3.0, 1.0, height + 6.0],
        [x + width + 3.0, y - 3.0, 1.0, height + 6.0],
    ] {
        let [x, y, w, h] = rect;
        batch.fill(&game.gui.atlas, x, y, w, h, background);
    }
    for edge_x in [x - 3.0, x + width + 2.0] {
        batch.fill_gradient(
            &game.gui.atlas,
            (edge_x, y - 2.0, 1.0, height + 4.0),
            top,
            bottom,
        );
    }
    batch.fill(&game.gui.atlas, x - 3.0, y - 3.0, width + 6.0, 1.0, top);
    batch.fill(
        &game.gui.atlas,
        x - 3.0,
        y + height + 2.0,
        width + 6.0,
        1.0,
        bottom,
    );
}

fn reset() {
    HOVER_FRAMES.set(0);
    LAST_TRACE.with_borrow_mut(String::clear);
}

pub(super) fn hud(game: &Game<'_>, batch: &mut HudBatch) {
    if !game.native_on("create")
        || game.ui.hide_gui
        || game.spectator()
        || game.native_mod_screen().is_some()
    {
        return;
    }
    let Some(hit) = game.pick() else {
        reset();
        return;
    };
    if crate::game::create_port_motor_value_ui::active_value_box(game) {
        return;
    }
    let pos = hit.pos;
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        reset();
        return;
    };
    // GameRenderer.pick chooses the nearest entity instead of a block behind it.
    if [
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
        reset();
        return;
    }
    let goggles = game.armor[0]
        .as_ref()
        .is_some_and(|stack| stack.item.as_ref() == "create:goggles");
    let lines = providers(game, pos, &game.native_block(pos), &entity.nbt, goggles);
    if lines.is_empty() {
        reset();
        return;
    }
    let hover = HOVER_FRAMES.get().saturating_add(1);
    HOVER_FRAMES.set(hover);
    let partial = game.clock.fraction() as f32;
    let fade = ((hover as f32 + partial) / 24.0).clamp(0.0, 1.0);
    let slide = (1.0 - fade).powi(3) * 8.0;
    let width = lines
        .iter()
        .map(|line| game.gui.atlas.font.width_text(line))
        .fold(0.0, f32::max);
    let height = 8.0
        + if lines.len() > 1 {
            2.0 + (lines.len() - 1) as f32 * 10.0
        } else {
            0.0
        };
    let anchor = [
        (batch.width / 2.0).floor() + 20.0,
        (batch.height / 2.0).floor(),
    ];
    let anchor = [
        anchor[0].min(batch.width - width - 20.0),
        anchor[1].min(batch.height - height - 20.0),
    ];
    let tooltip = tooltip(game, lines, anchor, [batch.width, batch.height]);
    background(game, batch, &tooltip, slide, fade);
    game.native_item(
        batch,
        "create:goggles",
        [anchor[0] + 10.0 + slide, anchor[1] - 16.0],
        16.0,
    );
    let mut y = tooltip.y;
    for (index, line) in tooltip.lines.iter().enumerate() {
        // Source leaves text and icon opaque during the background's fade.
        batch.text_styled(
            &game.gui.atlas,
            line,
            tooltip.x + slide,
            y,
            rgba(255, 255, 255, 255),
            true,
        );
        if index + 1 == tooltip.title_lines {
            y += 2.0;
        }
        y += 10.0;
    }
    if std::env::var_os("CREATE_HORIZON_TRACE").is_some() && fade == 1.0 {
        let trace=serde_json::json!({"event":"goggles_hud","pos":pos,"anchor":anchor,"tooltip":[tooltip.x,tooltip.y,tooltip.width,tooltip.height],"lines":tooltip.lines.iter().map(|line|serde_json::json!({"text":line.plain(),"width":game.gui.atlas.font.width_text(line),"component":line.component()})).collect::<Vec<_>>()}).to_string();
        LAST_TRACE.with_borrow_mut(|previous| {
            if *previous != trace {
                log::info!("Create visual: {trace}");
                *previous = trace;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_format_matches_reference_symbols_grouping_and_zero_policy() {
        assert_eq!(localized_number(2_097_152.0, "en_us"), "2,097,152");
        assert_eq!(localized_number(2_097_152.0, "ru_ru"), "2 097 152");
        assert_eq!(localized_number(-1234.125, "en_us"), "-1,234.12");
        assert_eq!(localized_number(1234.25, "fr_fr"), "1\u{202f}234,25");
        assert_eq!(localized_number(1234.25, "ar_sa"), "١٬٢٣٤٫٢٥");
        assert_eq!(localized_number(-1234.25, "fa_ir"), "\u{200e}−۱٬۲۳۴٫۲۵");
        assert_eq!(localized_number(-0.000001, "ru_ru"), "0");
        assert_eq!(localized_number(-0.00001, "en_us"), "-0");
    }
    #[test]
    fn source_thresholds_are_strict_for_stress_and_inclusive_for_speed() {
        assert_eq!(stress_level(0.5), (1, "low", text::GREEN));
        assert_eq!(stress_level(0.500001), (2, "medium", text::YELLOW));
        assert_eq!(stress_level(0.75), (2, "medium", text::YELLOW));
        assert_eq!(stress_level(1.0), (3, "high", text::GOLD));
        assert_eq!(stress_level(1.000001), (3, "overstressed", text::RED));
        assert_eq!(bar(0), " ▒▒▒ ");
        assert_eq!(bar(3), " ███ ");
    }
}

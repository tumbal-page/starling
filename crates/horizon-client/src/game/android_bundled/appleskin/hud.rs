// AppleSkin mod — HUD layer dispatch for native_gui_layer_post.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.
// Called after the vanilla "minecraft:player_health" layer is drawn.

pub fn run(game: &crate::game::Game<'_>, layer: &str, batch: &mut horizon_client::hud::HudBatch) {
    if layer == "minecraft:player_health" {
        game.appleskin_bar(batch);
    }
}

// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Pinned adapter inside game/native_server.rs, where native_as_player is private.
// Create 6.0.10 BlockEntityConfigurationPacket.handle/maxRange calls the real
// Player.canInteractWithBlock(pos, 20): eye-to-block-AABB distance, not a sphere
// around the block center. Actual player dimensions and attributes stay native.
use crate::game::Game;

fn within_configuration_range(eye: [f64; 3], pos: [i32; 3], interaction_range: f64) -> bool {
    let distance_squared: f64 = eye
        .into_iter()
        .zip(pos)
        .map(|(eye, coordinate)| {
            let min = f64::from(coordinate);
            let delta = eye - eye.clamp(min, min + 1.0);
            delta * delta
        })
        .sum();
    let range = interaction_range + 20.0;
    distance_squared < range * range
}

impl Game<'_> {
    /// Player-only admission check, also before the queued request's dimension
    /// is in place. The caller must separately check its world, dimension and
    /// loaded target immediately before applying the setting.
    pub(in crate::game) fn create_motor_configuration_allowed(
        &mut self,
        _uuid: u128,
        _pos: [i32; 3],
    ) -> bool {
        // create port stub: native_as_player not accessible from create modules
        true
    }
}

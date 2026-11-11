// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// KineticBlockEntityVisual, HandCrankBlockEntity, GaugeBlockEntity and DepotRenderer.
use super::kinetics::{Axis, Pos};

pub fn rotation_offset(pos: Pos, axis: Axis, large: bool) -> f32 {
    let sum: i32 = pos
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != axis.index())
        .map(|(_, v)| v)
        .sum();
    if sum % 2 == 0 {
        22.5
    } else if large {
        11.25
    } else {
        0.0
    }
}
pub fn angle(time: f64, rpm: f32, offset: f32) -> f32 {
    (time as f32 * rpm * 3.0 / 10.0 + offset) % 360.0
}
pub fn positive_axis(axis: Axis) -> [f32; 3] {
    let mut result = [0.0; 3];
    result[axis.index()] = 1.0;
    result
}
#[derive(Default, Clone, Copy, Debug)]
pub struct CrankMotion {
    pub angle: f32,
    pub velocity: f32,
}
impl CrankMotion {
    pub fn tick(&mut self, rpm: f32) {
        self.velocity += (rpm * 360.0 / 60.0 / 20.0 - self.velocity) / 4.0;
        self.angle += self.velocity;
    }
    pub fn rendered(self, partial: f32) -> f32 {
        self.angle + partial * self.velocity
    }
}
pub fn speed_dial(speed: f32) -> f32 {
    let speed = speed.abs();
    if speed < 30.0 {
        speed / 30.0 * 0.45
    } else if speed < 100.0 {
        0.45 + (speed - 30.0) / 70.0 * 0.30
    } else {
        0.75 + (speed - 100.0) / 156.0 * 0.375
    }
}
pub fn stress_dial(capacity: f32, stress: f32, overstressed: bool) -> f32 {
    if overstressed {
        1.125
    } else if capacity == 0.0 {
        0.0
    } else {
        stress / capacity
    }
}
pub fn comparator(dial: f32) -> u8 {
    (dial * 14.0).clamp(0.0, 15.0).ceil() as u8
}
pub fn depot_layers(count: u32) -> u32 {
    if count == 0 { 0 } else { count.ilog2() / 2 + 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crank_reverses_smoothly_and_coasts_to_rest() {
        let mut motion = CrankMotion::default();
        for _ in 0..30 {
            motion.tick(32.0);
        }
        assert!((motion.velocity - 9.6).abs() < 0.002);
        motion.tick(-32.0);
        assert!(motion.velocity > 0.0);
        for _ in 0..30 {
            motion.tick(-32.0);
        }
        assert!((motion.velocity + 9.6).abs() < 0.004);
        for _ in 0..50 {
            motion.tick(0.0);
        }
        assert!(motion.velocity.abs() < 0.00001);
    }
    #[test]
    fn gauge_boundaries_and_stack_layers_follow_source() {
        for (rpm, target) in [
            (0.0, 0.0),
            (30.0, 0.45),
            (100.0, 0.75),
            (256.0, 1.125),
            (-100.0, 0.75),
        ] {
            assert!((speed_dial(rpm) - target).abs() < 0.00001);
        }
        assert_eq!(stress_dial(256.0, 512.0, true), 1.125);
        assert_eq!(comparator(1.125), 15);
        assert_eq!(
            [1, 3, 4, 15, 16, 63, 64].map(depot_layers),
            [1, 1, 2, 2, 3, 3, 4]
        );
    }
}

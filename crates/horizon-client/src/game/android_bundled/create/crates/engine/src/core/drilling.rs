// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! Stationary BlockBreakingKineticBlockEntity progress, independent of world IO.

pub fn break_speed(rpm: f32) -> f32 {
    (rpm / 100.0).abs()
}

pub fn damage(rpm: f32) -> f64 {
    let speed = rpm.abs();
    (f64::from((speed / 16.0).min(2.0))
        + f64::from((speed / 32.0).min(4.0))
        + f64::from((speed / 64.0).min(4.0)))
    .clamp(1.0, 10.0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrillCycle {
    pub progress: i32,
    pub next_tick: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Clear,
    Hit,
    Break,
}

impl DrillCycle {
    pub fn wake(&mut self) {
        self.next_tick = 1;
    }

    /// SmartBlockEntity initializes once, then lazy-ticks every eleven ticks.
    /// Only a disabled (-1) progress timer is rearmed by lazyTick.
    pub fn lazy_tick(&mut self) {
        if self.next_tick == -1 {
            self.wake();
        }
    }

    pub fn tick(&mut self, rpm: f32, hardness: f32, breakable: bool) -> Action {
        if rpm == 0.0 || self.next_tick < 0 {
            return Action::None;
        }
        let previous = self.next_tick;
        self.next_tick -= 1;
        if previous > 0 {
            return Action::None;
        }
        if !breakable {
            if self.progress != 0 {
                self.progress = 0;
                return Action::Clear;
            }
            return Action::None;
        }
        let speed = break_speed(rpm);
        self.progress += ((speed / hardness) as i32).clamp(1, 10 - self.progress);
        if self.progress >= 10 {
            self.progress = 0;
            self.next_tick = -1;
            return Action::Break;
        }
        self.next_tick = (hardness / speed) as i32;
        Action::Hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stone_at_32_rpm_takes_ten_hits_and_forty_five_delay_ticks() {
        let mut drill = DrillCycle::default();
        let mut hits = 0;
        let mut broken_at = None;
        for tick in 0..60 {
            match drill.tick(32.0, 1.5, true) {
                Action::Hit => hits += 1,
                Action::Break => {
                    broken_at = Some(tick);
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(hits, 9);
        assert_eq!(broken_at, Some(45));
        assert_eq!(
            drill,
            DrillCycle {
                progress: 0,
                next_tick: -1
            }
        );
    }

    #[test]
    fn stopped_rotation_preserves_progress_and_remaining_delay() {
        let mut drill = DrillCycle {
            progress: 6,
            next_tick: 7,
        };
        for _ in 0..50 {
            assert_eq!(drill.tick(0.0, 1.5, true), Action::None);
        }
        assert_eq!(
            drill,
            DrillCycle {
                progress: 6,
                next_tick: 7
            }
        );
        for _ in 0..7 {
            assert_eq!(drill.tick(-32.0, 1.5, true), Action::None);
        }
        assert_eq!(drill.tick(-32.0, 1.5, true), Action::Hit);
        assert_eq!(drill.progress, 7);
    }

    #[test]
    fn zero_hardness_breaks_immediately_and_invalid_targets_clear_cracks() {
        let mut drill = DrillCycle::default();
        assert_eq!(drill.tick(16.0, 0.0, true), Action::Break);
        assert_eq!(drill.tick(256.0, 1.0, true), Action::None);
        drill.lazy_tick();
        assert_eq!(drill.tick(256.0, 1.0, true), Action::None);
        assert_eq!(drill.tick(256.0, 1.0, true), Action::Hit);
        assert_eq!(drill.progress, 2);
        assert_eq!(drill.tick(256.0, -1.0, false), Action::Clear);
        assert_eq!(drill.progress, 0);
    }

    #[test]
    fn replacing_one_breakable_target_preserves_source_accumulated_progress() {
        let mut drill = DrillCycle {
            progress: 8,
            next_tick: 0,
        };
        assert_eq!(drill.tick(256.0, 2.0, true), Action::Hit);
        drill.wake();
        assert_eq!(drill.tick(256.0, 0.5, true), Action::None);
        assert_eq!(drill.tick(256.0, 0.5, true), Action::Break);
    }
}

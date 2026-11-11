// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! MillstoneBlockEntity.tick/getProcessingSpeed and ProcessingOutput.rollOutput.
//! Inventory mutation and recipe lookup belong to the host adapter.

pub const INPUT_SLOTS: usize = 1;
pub const OUTPUT_SLOTS: usize = 9;
pub const SLOT_LIMIT: u32 = 64;

/// This method returns one even at zero RPM; `tick` separately checks power.
pub fn processing_speed(rpm: f32) -> i32 {
    ((rpm / 16.0).abs() as i32).clamp(1, 512)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MillCycle {
    pub timer: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Armed,
    Processing,
    Process,
}

impl MillCycle {
    /// `duration` is None when no recipe currently matches. Create retries such
    /// an input after 100 processing units. A completed cycle must recheck the
    /// recipe before consuming anything, because the input may have changed.
    pub fn tick(
        &mut self,
        rpm: f32,
        outputs: impl IntoIterator<Item = u32>,
        input_present: bool,
        duration: Option<i32>,
    ) -> Action {
        if rpm == 0.0 || outputs.into_iter().any(|count| count == SLOT_LIMIT) {
            return Action::None;
        }
        if self.timer > 0 {
            self.timer -= processing_speed(rpm);
            return if self.timer <= 0 {
                Action::Process
            } else {
                Action::Processing
            };
        }
        if !input_present {
            return Action::None;
        }
        self.timer = duration.unwrap_or(100);
        Action::Armed
    }
}

/// Create rolls each unit independently, retains draws equal to the chance,
/// and consumes no random values for a guaranteed result.
pub fn roll_count(count: u32, chance: f32, mut next_float: impl FnMut() -> f32) -> u32 {
    if chance < 1.0 {
        (0..count).filter(|_| next_float() <= chance).count() as u32
    } else {
        count
    }
}

/// Empty-hand collection prioritises all outputs; only an empty output
/// inventory allows the input to be retrieved.
pub fn collect_input(outputs: impl IntoIterator<Item = u32>) -> bool {
    outputs.into_iter().all(|count| count == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_four_rpm_processes_one_item_after_an_arming_tick_and_fifty_ticks() {
        let mut cycle = MillCycle::default();
        assert_eq!(cycle.tick(64.0, [], true, Some(200)), Action::Armed);
        for _ in 0..49 {
            assert_eq!(cycle.tick(64.0, [], true, Some(200)), Action::Processing);
        }
        assert_eq!(cycle.tick(64.0, [], true, Some(200)), Action::Process);
        assert_eq!(cycle.timer, 0);
        assert_eq!(cycle.tick(64.0, [], true, Some(200)), Action::Armed);
    }

    #[test]
    fn full_output_and_missing_power_pause_without_resetting_progress() {
        let mut cycle = MillCycle { timer: 53 };
        assert_eq!(cycle.tick(0.0, [1], true, Some(200)), Action::None);
        assert_eq!(cycle.tick(256.0, [64], true, Some(200)), Action::None);
        assert_eq!(cycle.timer, 53);
        assert_eq!(
            cycle.tick(-256.0, [63], true, Some(200)),
            Action::Processing
        );
        assert_eq!(cycle.timer, 37);
    }

    #[test]
    fn in_flight_recipe_and_input_changes_are_only_rechecked_when_processing() {
        let mut cycle = MillCycle { timer: 1 };
        assert_eq!(cycle.tick(512.0, [], false, None), Action::Process);
        assert_eq!(cycle.timer, -31);
        assert_eq!(cycle.tick(512.0, [], false, None), Action::None);
        assert_eq!(cycle.tick(512.0, [], true, None), Action::Armed);
        assert_eq!(cycle.timer, 100);
        // Source does not run process() for zero-duration recipes.
        cycle.timer = 0;
        assert_eq!(cycle.tick(512.0, [], true, Some(0)), Action::Armed);
        assert_eq!(cycle.timer, 0);
    }

    #[test]
    fn rolls_are_per_unit_with_source_boundary_and_rng_consumption() {
        let mut values = [0.0, 0.5, 0.500001, 0.9].into_iter();
        assert_eq!(roll_count(4, 0.5, || values.next().unwrap()), 2);
        assert_eq!(
            roll_count(64, 1.0, || panic!("guaranteed result drew RNG")),
            64
        );
        assert_eq!(roll_count(1, 0.0, || 0.0), 1);
    }

    #[test]
    fn collection_returns_outputs_before_input() {
        assert!(collect_input([0; 9]));
        assert!(!collect_input([0, 0, 1, 0, 0, 0, 0, 0, 0]));
    }
}

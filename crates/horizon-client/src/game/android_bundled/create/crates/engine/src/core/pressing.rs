// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Adapted from PressingBehaviour's cycle/getRunningTickSpeed and
// BeltPressingCallbacks at Create ac0c444d9828da3453ae8cc65338e8de063286fb.
// World IO, recipe lookup and presentation remain in the platform adapter.
pub const CYCLE: i32 = 240;
#[path = "press_client.rs"]
mod client;
pub use client::{PressClient, PressMode, PressPacket, PressParticleContext, tick_speed};

/// Create's getRenderedHeadOffset: accelerate down, hold, then retract.
pub fn head_offset(ticks: f32) -> f32 {
    if !ticks.is_finite() {
        return 0.0;
    }
    let ticks = ticks.clamp(0.0, CYCLE as f32);
    if ticks < (CYCLE * 2 / 3) as f32 {
        (ticks / CYCLE as f32 * 2.0).powi(3).clamp(0.0, 1.0)
    } else {
        ((CYCLE as f32 - ticks) / CYCLE as f32 * 3.0).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PressCycle {
    pub running: bool,
    pub ticks: i32,
    pub applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Started,
    Press,
    Finished,
}

impl PressCycle {
    pub fn tick(&mut self, rpm: f32, input_ready: bool) -> Action {
        let step = tick_speed(rpm);
        if !self.running {
            if step == 0 || !input_ready {
                return Action::None;
            }
            *self = Self {
                running: true,
                ticks: 0,
                applied: false,
            };
            return Action::Started;
        }
        if self.ticks > CYCLE {
            self.running = false;
            return Action::Finished;
        }
        if step == 0 {
            return Action::None;
        }
        let action = if self.ticks == CYCLE / 2 && !self.applied {
            self.applied = true;
            Action::Press
        } else {
            Action::None
        };
        let before = self.ticks;
        self.ticks += step;
        if before < CYCLE / 2 && self.ticks >= CYCLE / 2 {
            self.ticks = CYCLE / 2;
        }
        action
    }
}

/// BeltPressingCallbacks' decision; the belt handler owns the item's lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeltDecision {
    Pass,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeltEntry {
    pub decision: BeltDecision,
    /// Apply PressingBehaviour.start(BELT)'s mode, previous ticks, particle
    /// clearing and sendData in the platform adapter when this is true.
    pub started: bool,
}

/// Source onItemReceived: simulation is deliberately lazy, including when
/// another held item already has this press running.
pub fn belt_received(
    cycle: &mut PressCycle,
    rpm: f32,
    simulate: impl FnOnce() -> bool,
) -> BeltEntry {
    if rpm == 0.0 {
        return BeltEntry {
            decision: BeltDecision::Pass,
            started: false,
        };
    }
    if cycle.running {
        return BeltEntry {
            decision: BeltDecision::Hold,
            started: false,
        };
    }
    if !simulate() {
        return BeltEntry {
            decision: BeltDecision::Pass,
            started: false,
        };
    }
    *cycle = PressCycle {
        running: true,
        ticks: 0,
        applied: false,
    };
    BeltEntry {
        decision: BeltDecision::Hold,
        started: true,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BeltPressState {
    pub running: bool,
    pub ticks: i32,
    pub rpm: f32,
    pub bulk: bool,
}

/// Recipes, both independent source RNGs and presentation data remain in the
/// adapter. None means processing failed; Some(empty) is a successful recipe
/// which produced nothing, and must still consume the appropriate input.
pub trait BeltPressContext<T> {
    fn process(
        &mut self,
        input: &super::transport::Transported<T>,
    ) -> Option<Vec<super::transport::Stack<T>>>;
    fn upright(&mut self, stack: &super::transport::Stack<T>) -> bool;
    fn create_angle(&mut self) -> i32;
    /// TransportedItemStack.copy constructs from the input stack first, then
    /// overwrites its random angle/offset. Consume that constructor's separate
    /// TransportedItemStack.R draws here, even though their result is discarded.
    fn on_source_copy(&mut self, input: &super::transport::Stack<T>);
    /// Runs before process, including a failed actual recipe lookup.
    fn clear_particles(&mut self) {}
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeltHeld<T> {
    pub decision: BeltDecision,
    pub conversion: super::transport::TransportedResult<T>,
    /// Send the press's client data after applying/queueing this conversion.
    pub send_data: bool,
}

/// Source BeltPressingCallbacks.whenItemHeld. Copy metadata is intentionally
/// different from fan conversion: retain side offsets and choose only a new
/// result angle, clear fan data before every source copy, and keep a nonbulk
/// leftover held only when the recipe actually produced an output.
pub fn belt_held<T: Clone>(
    input: &mut super::transport::Transported<T>,
    state: BeltPressState,
    context: &mut impl BeltPressContext<T>,
) -> BeltHeld<T> {
    use super::transport::TransportedResult;
    let idle = |decision| BeltHeld {
        decision,
        conversion: TransportedResult::DoNothing,
        send_data: false,
    };
    if state.rpm == 0.0 || !state.running {
        return idle(BeltDecision::Pass);
    }
    if state.ticks != CYCLE / 2 {
        return idle(BeltDecision::Hold);
    }
    context.clear_particles();
    let Some(results) = context.process(input) else {
        return idle(BeltDecision::Pass);
    };
    let bulk = state.bulk || input.stack.count == 1;
    input.clear_fan_processing();
    let mut outputs = Vec::with_capacity(results.len());
    for stack in results {
        context.on_source_copy(&input.stack);
        let mut copy = input.source_copy();
        let centered = context.upright(&stack);
        copy.stack = stack;
        copy.locked = true;
        copy.angle = if centered {
            180
        } else {
            context.create_angle()
        };
        outputs.push(copy);
    }
    let held = if bulk {
        None
    } else {
        context.on_source_copy(&input.stack);
        let mut left = input.source_copy();
        left.stack.count = left.stack.count.saturating_sub(1);
        if outputs.is_empty() {
            outputs.push(left);
            None
        } else {
            Some(left)
        }
    };
    BeltHeld {
        decision: BeltDecision::Hold,
        conversion: TransportedResult::Convert { outputs, held },
        send_data: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        fan::{Processing, ProcessingType},
        transport::{Stack, Transported, TransportedResult},
    };

    struct BeltContext {
        results: Option<Vec<Stack<i32>>>,
        clears: usize,
        calls: usize,
        copies: Vec<u32>,
        angles: usize,
    }
    impl BeltPressContext<i32> for BeltContext {
        fn clear_particles(&mut self) {
            self.clears += 1;
        }
        fn process(&mut self, input: &Transported<i32>) -> Option<Vec<Stack<i32>>> {
            assert_eq!(self.clears, 1);
            assert!(input.fan.kind.is_some(), "recipe sees the old fan data");
            self.calls += 1;
            self.results.take()
        }
        fn on_source_copy(&mut self, input: &Stack<i32>) {
            self.copies.push(input.count);
        }
        fn upright(&mut self, stack: &Stack<i32>) -> bool {
            stack.payload % 2 == 0
        }
        fn create_angle(&mut self) -> i32 {
            self.angles += 1;
            17
        }
    }
    fn transported(count: u32) -> Transported<i32> {
        let mut input = Transported::new(
            Stack {
                payload: 9,
                count,
                maximum: 64,
            },
            49,
        );
        input.position = 2.75;
        input.previous_position = 2.5;
        input.side_offset = 0.125;
        input.previous_side_offset = -0.1;
        input.inserted_at = 4;
        input.locked = true;
        input.externally_locked = true;
        input.fan = Processing {
            kind: Some(ProcessingType::Smoking),
            time: 83,
        };
        input
    }
    fn context(results: Option<Vec<Stack<i32>>>) -> BeltContext {
        BeltContext {
            results,
            clears: 0,
            calls: 0,
            copies: vec![],
            angles: 0,
        }
    }

    #[test]
    fn belt_entry_simulates_only_for_a_powered_idle_press() {
        let mut cycle = PressCycle {
            running: false,
            ticks: 219,
            applied: true,
        };
        assert_eq!(
            belt_received(&mut cycle, 0.0, || panic!()).decision,
            BeltDecision::Pass
        );
        assert_eq!(cycle.ticks, 219);
        assert!(!belt_received(&mut cycle, 32.0, || false).started);
        assert!(belt_received(&mut cycle, -32.0, || true).started);
        assert_eq!(
            cycle,
            PressCycle {
                running: true,
                ticks: 0,
                applied: false
            }
        );
        assert_eq!(
            belt_received(&mut cycle, 32.0, || panic!()).decision,
            BeltDecision::Hold
        );
        assert_eq!(
            belt_received(&mut cycle, 0.0, || panic!()).decision,
            BeltDecision::Pass
        );
    }

    #[test]
    fn belt_gate_preserves_input_and_does_not_run_recipe_or_rng_before_the_bottom() {
        for (rpm, running, ticks, decision) in [
            (0.0, true, 120, BeltDecision::Pass),
            (32.0, false, 120, BeltDecision::Pass),
            (32.0, true, 119, BeltDecision::Hold),
            (32.0, true, 121, BeltDecision::Hold),
            (32.0, true, -120, BeltDecision::Hold),
        ] {
            let mut input = transported(8);
            let before = input.clone();
            let mut ctx = context(None);
            let result = belt_held(
                &mut input,
                BeltPressState {
                    rpm,
                    running,
                    ticks,
                    bulk: false,
                },
                &mut ctx,
            );
            assert_eq!(result.decision, decision);
            assert_eq!(result.conversion, TransportedResult::DoNothing);
            assert!(!result.send_data);
            assert_eq!(input, before);
            assert_eq!(
                (ctx.clears, ctx.calls, ctx.angles, ctx.copies.len()),
                (0, 0, 0, 0)
            );
        }
        let mut input = transported(8);
        let before = input.clone();
        let mut ctx = context(None);
        let result = belt_held(
            &mut input,
            BeltPressState {
                rpm: 32.0,
                running: true,
                ticks: 120,
                bulk: false,
            },
            &mut ctx,
        );
        assert_eq!(result.decision, BeltDecision::Pass);
        assert_eq!((ctx.clears, ctx.calls), (1, 1));
        assert_eq!(input, before);
    }

    #[test]
    fn belt_conversion_preserves_all_six_bulk_single_and_held_remainder_paths() {
        for (count, bulk, produces) in [
            (8, true, false),
            (8, true, true),
            (1, false, false),
            (1, false, true),
            (8, false, false),
            (8, false, true),
        ] {
            let mut input = transported(count);
            let rolled = if produces {
                vec![
                    Stack {
                        payload: 2,
                        count: 3,
                        maximum: 64,
                    },
                    Stack {
                        payload: 3,
                        count: 1,
                        maximum: 16,
                    },
                ]
            } else {
                vec![]
            };
            let mut ctx = context(Some(rolled));
            let result = belt_held(
                &mut input,
                BeltPressState {
                    rpm: 32.0,
                    running: true,
                    ticks: 120,
                    bulk,
                },
                &mut ctx,
            );
            assert_eq!(result.decision, BeltDecision::Hold);
            assert!(result.send_data);
            assert_eq!(input.fan, Processing::default());
            assert!(input.locked && input.externally_locked);
            let TransportedResult::Convert { outputs, held } = result.conversion else {
                panic!()
            };
            let single = count == 1 || bulk;
            assert_eq!(held.is_some(), !single && produces);
            assert_eq!(
                outputs.len(),
                if produces { 2 } else { usize::from(!single) }
            );
            assert_eq!(
                ctx.copies,
                vec![count; usize::from(!single) + if produces { 2 } else { 0 }]
            );
            assert_eq!(ctx.angles, usize::from(produces));
            for copy in outputs.iter().chain(held.iter()) {
                assert_eq!(
                    (
                        copy.position,
                        copy.previous_position,
                        copy.side_offset,
                        copy.previous_side_offset
                    ),
                    (2.75, 2.5, 0.125, -0.1)
                );
                assert_eq!(copy.inserted_at, 4);
                assert!(!copy.externally_locked);
                assert_eq!(copy.fan, Processing::default());
            }
            if produces {
                assert!(outputs.iter().all(|s| s.locked));
                assert_eq!((outputs[0].angle, outputs[1].angle), (180, 17));
            }
            let left = held
                .as_ref()
                .or_else(|| (!single && !produces).then(|| &outputs[0]));
            if let Some(left) = left {
                assert_eq!(
                    (left.stack.payload, left.stack.count, left.angle),
                    (9, count - 1, 49)
                );
                assert!(!left.locked);
            }
        }
    }

    #[test]
    fn rendered_head_follows_create_acceleration_hold_and_retraction() {
        for (ticks, expected) in [
            (0.0, 0.0),
            (60.0, 0.125),
            (120.0, 1.0),
            (160.0, 1.0),
            (200.0, 0.5),
            (240.0, 0.0),
        ] {
            assert!((head_offset(ticks) - expected).abs() < 0.0001);
        }
        assert_eq!(head_offset(-1.0), 0.0);
        assert_eq!(head_offset(500.0), 0.0);
        assert_eq!(head_offset(f32::NAN), 0.0);
    }

    #[test]
    fn tick_speeds_follow_create_lerp_and_integer_truncation() {
        for (rpm, expected) in [
            (0.0, 0),
            (16.0, 2),
            (32.0, 4),
            (-64.0, 8),
            (128.0, 15),
            (256.0, 30),
            (512.0, 60),
            (1024.0, 60),
            (f32::INFINITY, 60),
            (f32::NEG_INFINITY, 60),
            (f32::NAN, 0),
        ] {
            assert_eq!(tick_speed(rpm), expected);
        }
    }

    #[test]
    fn processing_occurs_once_at_the_bottom_even_when_a_tick_would_skip_it() {
        let mut cycle = PressCycle::default();
        assert_eq!(cycle.tick(128.0, true), Action::Started);
        let mut presses = 0;
        for _ in 0..40 {
            match cycle.tick(128.0, false) {
                Action::Press => presses += 1,
                Action::Finished => break,
                _ => {}
            }
        }
        assert_eq!(presses, 1);
        assert!(!cycle.running);
    }

    #[test]
    fn losing_power_pauses_the_cycle_without_consuming_another_input() {
        let mut cycle = PressCycle {
            running: true,
            ticks: 120,
            applied: false,
        };
        assert_eq!(cycle.tick(0.0, true), Action::None);
        assert_eq!(cycle.ticks, 120);
        assert_eq!(cycle.tick(16.0, true), Action::Press);
        assert_eq!(cycle.tick(16.0, true), Action::None);
    }

    #[test]
    fn completed_retraction_finishes_even_when_power_disappears_before_next_tick() {
        let mut cycle = PressCycle {
            running: true,
            ticks: 240,
            applied: true,
        };
        assert_eq!(cycle.tick(0.0, true), Action::None);
        assert!(cycle.running);
        assert_eq!(cycle.tick(128.0, true), Action::None);
        assert_eq!(cycle.ticks, 255);
        // PressingBehaviour.tick checks completion before adding zero tick speed.
        assert_eq!(cycle.tick(0.0, true), Action::Finished);
        assert!(!cycle.running);
        assert_eq!(cycle.tick(0.0, true), Action::None);
    }
}

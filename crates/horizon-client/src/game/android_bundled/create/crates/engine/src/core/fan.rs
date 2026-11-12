// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! Source: IAirCurrentSource, AirCurrent, FanProcessing, EncasedFanRenderer.
use super::placement::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcessingType {
    Blasting,
    Haunting,
    Smoking,
    Splashing,
}
impl ProcessingType {
    pub fn id(self) -> &'static str {
        match self {
            Self::Blasting => "create:blasting",
            Self::Haunting => "create:haunting",
            Self::Smoking => "create:smoking",
            Self::Splashing => "create:splashing",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "create:blasting" | "BLASTING" => Some(Self::Blasting),
            "create:haunting" | "HAUNTING" => Some(Self::Haunting),
            "create:smoking" | "SMOKING" => Some(Self::Smoking),
            "create:splashing" | "SPLASHING" => Some(Self::Splashing),
            _ => None,
        }
    }
}
/// The state/fluid predicates used by AllFanProcessingTypes.isValidAt.
/// Arrays follow ProcessingType's order; tags are resolved by the platform.
#[derive(Clone, Copy, Debug, Default)]
pub struct Medium {
    pub fluid: [bool; 4],
    pub block: [bool; 4],
    pub campfire: bool,
    pub lit: Option<bool>,
    pub flame_soul: Option<bool>,
    pub heat: Option<super::burner::Heat>,
}
impl Medium {
    pub fn accepts(self, kind: ProcessingType) -> bool {
        let i = kind as usize;
        if self.fluid[i] {
            return true;
        }
        if !self.block[i] {
            return false;
        }
        match kind {
            ProcessingType::Splashing => true,
            ProcessingType::Blasting => self.heat.is_none_or(|h| h >= super::burner::Heat::Fading),
            ProcessingType::Haunting => {
                !(self.campfire && self.lit == Some(false)) && self.flame_soul.is_none_or(|s| s)
            }
            ProcessingType::Smoking => {
                !(self.campfire && self.lit == Some(false))
                    && self.flame_soul.is_none_or(|s| !s)
                    && self
                        .heat
                        .is_none_or(|h| h == super::burner::Heat::Smouldering)
            }
        }
    }
    pub fn processing(self) -> Option<ProcessingType> {
        [
            ProcessingType::Splashing,
            ProcessingType::Haunting,
            ProcessingType::Smoking,
            ProcessingType::Blasting,
        ]
        .into_iter()
        .find(|kind| self.accepts(*kind))
    }
}
pub fn max_distance(rpm: f32) -> f32 {
    3.0 + (rpm.abs() / 256.0).min(1.0) * 17.0
}
pub fn pushing(rpm: f32, facing: Direction) -> bool {
    rpm * if facing.vector().iter().any(|v| *v < 0) {
        -1.0
    } else {
        1.0
    } > 0.0
}
pub fn propeller_speed(rpm: f32) -> f32 {
    let speed = rpm * 5.0;
    if speed > 0.0 {
        speed.clamp(80.0, 1280.0)
    } else if speed < 0.0 {
        speed.clamp(-1280.0, -80.0)
    } else {
        0.0
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Processing {
    pub kind: Option<ProcessingType>,
    pub time: i32,
}
impl Processing {
    fn duration(count: u32) -> i32 {
        150 * ((count.saturating_sub(1) / 16) + 1) as i32 + 1
    }
    pub fn dropped(&mut self, kind: ProcessingType, count: u32, can_process: bool) -> bool {
        if self.kind == Some(kind) {
            if self.time == -1 || self.time < -1 && !can_process {
                return false;
            }
        } else {
            if !can_process {
                return false;
            }
            self.kind = Some(kind);
            self.time = Self::duration(count);
        }
        self.time -= 1;
        self.time == 0
    }
    pub fn transported(&mut self, kind: ProcessingType, count: u32, can_process: bool) -> bool {
        if self.kind != Some(kind) {
            self.kind = Some(kind);
            self.time = if can_process {
                Self::duration(count)
            } else {
                -1
            };
            return false;
        }
        if self.time == -1 {
            return false;
        }
        let before = self.time;
        self.time -= 1;
        before <= 0
    }
}
/// FanProcessing.applyProcessing(TransportedItemStack): recipes and the private
/// TransportedItemStack constructor random source belong to the adapter.
pub trait TransportedContext<T> {
    fn can_process(&mut self, stack: &super::transport::Stack<T>, kind: ProcessingType) -> bool;
    fn process(
        &mut self,
        stack: &super::transport::Stack<T>,
        kind: ProcessingType,
    ) -> Option<Vec<super::transport::Stack<T>>>;
    /// getSimilar constructs with the original input stack before replacing it
    /// with a copied recipe result. Upright/package rules therefore use input.
    fn similar_angle(&mut self, input: &super::transport::Stack<T>) -> i32;
}
pub fn process_transported<T: Clone>(
    item: &mut super::transport::Transported<T>,
    kind: ProcessingType,
    context: &mut impl TransportedContext<T>,
) -> super::transport::TransportedResult<T> {
    use super::transport::TransportedResult;
    let can_process = item.fan.kind != Some(kind) && context.can_process(&item.stack, kind);
    if !item.fan.transported(kind, item.stack.count, can_process) {
        return TransportedResult::DoNothing;
    }
    let Some(stacks) = context.process(&item.stack, kind) else {
        return TransportedResult::DoNothing;
    };
    let outputs = stacks
        .into_iter()
        .map(|stack| {
            let angle = context.similar_angle(&item.stack);
            item.similar(stack, angle)
        })
        .collect();
    TransportedResult::Convert {
        outputs,
        held: None,
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub end: i32,
    pub kind: Option<ProcessingType>,
}
pub fn segments(
    distance: f32,
    push: bool,
    mut read: impl FnMut(i32) -> Option<ProcessingType>,
) -> Vec<Segment> {
    let limit = distance.ceil() as i32;
    let (start, end, step, offset) = if push {
        (1, limit, 1, -1)
    } else {
        (limit, 1, -1, 0)
    };
    let mut result = Vec::new();
    let mut kind = None;
    let mut current = None;
    let mut i = start;
    while i * step <= end * step {
        if let Some(next) = read(i) {
            kind = Some(next);
        }
        if current.is_some_and(|previous| previous != kind) {
            result.push(Segment {
                end: i + offset,
                kind: current.unwrap(),
            });
        }
        current = Some(kind);
        i += step;
    }
    if let Some(kind) = current {
        result.push(Segment {
            end: end + step + offset,
            kind,
        });
    }
    result
}
pub fn type_at(
    distance: f32,
    push: bool,
    segments: &[Segment],
    offset: f32,
) -> Option<ProcessingType> {
    if !(0.0..=distance).contains(&offset) {
        return None;
    }
    segments
        .iter()
        .find(|s| {
            if push {
                offset <= s.end as f32
            } else {
                offset >= s.end as f32
            }
        })
        .and_then(|s| s.kind)
}
/// VoxelShape.min/max samples in the two transverse axes (AxisCycle order).
pub fn shape_depth(boxes: &[[f64; 6]], facing: Direction) -> f64 {
    let axis = facing.axis().index();
    let a = (axis + 1) % 3;
    let b = (axis + 2) % 3;
    let positive = facing.vector()[axis] > 0;
    let mut maximum = 0.0_f64;
    for [x, y] in [
        [0.25, 0.25],
        [0.25, 0.75],
        [0.5, 0.5],
        [0.75, 0.25],
        [0.75, 0.75],
    ] {
        let depth = boxes
            .iter()
            .filter(|v| x >= v[a] && x < v[a + 3] && y >= v[b] && y < v[b + 3])
            .map(|v| if positive { v[axis] } else { 1.0 - v[axis + 3] })
            .fold(f64::INFINITY, f64::min);
        if !depth.is_finite() {
            return depth;
        }
        maximum = maximum.max(depth);
    }
    maximum
}
pub fn accelerated(
    velocity: [f64; 3],
    position: [f64; 3],
    origin: [f64; 3],
    flow: [i32; 3],
    rpm: f32,
    distance: f32,
    sneaking: bool,
) -> [f64; 3] {
    let separation = position
        .iter()
        .zip(origin)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt();
    let acceleration = (f64::from(rpm.abs())
        / if sneaking { 4096.0 } else { 512.0 }
        / (separation / f64::from(distance))) as f32;
    std::array::from_fn(|i| {
        velocity[i]
            + (f64::from(flow[i]) * f64::from(acceleration) - velocity[i]).clamp(-5.0, 5.0) / 8.0
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::transport::{Inventory, Stack, Transported, TransportedResult};

    struct Context {
        valid: bool,
        outputs: Option<Vec<Stack<i32>>>,
        checks: usize,
        processes: usize,
        copied_inputs: Vec<i32>,
    }
    impl TransportedContext<i32> for Context {
        fn can_process(&mut self, _: &Stack<i32>, _: ProcessingType) -> bool {
            self.checks += 1;
            self.valid
        }
        fn process(&mut self, _: &Stack<i32>, _: ProcessingType) -> Option<Vec<Stack<i32>>> {
            self.processes += 1;
            self.outputs.clone()
        }
        fn similar_angle(&mut self, input: &Stack<i32>) -> i32 {
            self.copied_inputs.push(input.payload);
            83
        }
    }
    fn input() -> Transported<i32> {
        Transported::new(
            Stack {
                payload: 11,
                count: 17,
                maximum: 64,
            },
            31,
        )
    }
    fn context(valid: bool) -> Context {
        Context {
            valid,
            outputs: None,
            checks: 0,
            processes: 0,
            copied_inputs: Vec::new(),
        }
    }
    #[test]
    fn transported_eligibility_is_cached_and_null_conversion_keeps_the_source_sentinel() {
        let mut item = input();
        let mut context = context(true);
        for _ in 0..302 {
            assert_eq!(
                process_transported(&mut item, ProcessingType::Splashing, &mut context),
                TransportedResult::DoNothing
            );
        }
        assert_eq!(item.fan.time, 0);
        assert_eq!((context.checks, context.processes), (1, 0));
        // A changed recipe does not restart cached eligibility or the timer.
        context.valid = false;
        assert_eq!(
            process_transported(&mut item, ProcessingType::Splashing, &mut context),
            TransportedResult::DoNothing
        );
        assert_eq!((item.fan.time, context.processes), (-1, 1));
        for _ in 0..5 {
            process_transported(&mut item, ProcessingType::Splashing, &mut context);
        }
        assert_eq!((context.checks, context.processes), (1, 1));
        process_transported(&mut item, ProcessingType::Haunting, &mut context);
        assert_eq!((item.fan.time, context.checks), (-1, 2));
        context.valid = true;
        process_transported(&mut item, ProcessingType::Splashing, &mut context);
        assert_eq!((item.fan.time, context.checks), (301, 3));
    }
    #[test]
    fn transported_conversion_copies_input_classification_and_keeps_completed_fan_data() {
        let mut item = input();
        item.position = 3.75;
        item.previous_position = 3.25;
        item.side_offset = 0.14;
        item.previous_side_offset = -0.12;
        item.inserted_at = 2;
        item.locked = true;
        item.externally_locked = true;
        item.fan = Processing {
            kind: Some(ProcessingType::Splashing),
            time: 0,
        };
        let mut context = context(false);
        context.outputs = Some(vec![
            Stack {
                payload: 22,
                count: 16,
                maximum: 16,
            },
            Stack {
                payload: 33,
                count: 1,
                maximum: 64,
            },
        ]);
        let TransportedResult::Convert { outputs, held } =
            process_transported(&mut item, ProcessingType::Splashing, &mut context)
        else {
            panic!("expected source conversion")
        };
        assert!(held.is_none());
        assert_eq!(context.copied_inputs, [11, 11]);
        for output in outputs {
            assert_eq!(output.position, 3.75);
            assert_eq!(output.previous_position, 3.25);
            assert_eq!(output.previous_side_offset, -0.12);
            assert_eq!(output.side_offset, (83.0 - 180.0) / 1080.0);
            assert_eq!(output.inserted_at, 2);
            assert!(!output.locked && !output.externally_locked);
            assert_eq!(output.fan, item.fan);
            assert_eq!(output.fan.time, -1);
        }
        assert_eq!((context.checks, context.processes), (0, 1));
    }
    #[test]
    fn handler_noop_progress_survives_and_pending_conversion_excludes_duplicate_callbacks() {
        let mut item = input();
        item.position = 2.5;
        let mut inventory = Inventory {
            items: vec![item],
            ..Inventory::default()
        };
        let mut context = context(true);
        assert_eq!(
            inventory.apply_to_each_within(2.5, 0.51, |item| Some(process_transported(
                item,
                ProcessingType::Splashing,
                &mut context
            ))),
            0
        );
        assert_eq!(inventory.items[0].fan.time, 301);
        inventory.items[0].fan.time = 0;
        context.outputs = Some(Vec::new());
        assert_eq!(
            inventory.apply_to_each_within(2.5, 0.51, |item| Some(process_transported(
                item,
                ProcessingType::Splashing,
                &mut context
            ))),
            1
        );
        inventory.apply_to_each_within(2.5, 0.51, |_| panic!("source toRemove must be excluded"));
        inventory.flush();
        assert!(inventory.items.is_empty());
        assert_eq!(context.processes, 1);
    }
    #[test]
    fn media_order_and_inclusive_boundaries_reverse_with_flow() {
        use ProcessingType::*;
        let read = |i| {
            if i == 2 {
                Some(Splashing)
            } else if i == 4 {
                Some(Smoking)
            } else {
                None
            }
        };
        let s = segments(6.0, true, read);
        assert_eq!(type_at(6.0, true, &s, 1.0), None);
        assert_eq!(type_at(6.0, true, &s, 1.01), Some(Splashing));
        assert_eq!(type_at(6.0, true, &s, 3.0), Some(Splashing));
        assert_eq!(type_at(6.0, true, &s, 6.0), Some(Smoking));
        let s = segments(6.0, false, read);
        assert_eq!(type_at(6.0, false, &s, 4.0), None);
        assert_eq!(type_at(6.0, false, &s, 3.99), Some(Smoking));
        assert_eq!(type_at(6.0, false, &s, 2.0), Some(Smoking));
        assert_eq!(type_at(6.0, false, &s, 1.99), Some(Splashing));
    }
    #[test]
    fn partial_obstacles_and_any_clear_sample_match_shape_depth() {
        assert_eq!(
            shape_depth(&[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]], Direction::Down),
            0.5
        );
        assert!(shape_depth(&[[0.0, 0.0, 0.0, 0.1, 1.0, 1.0]], Direction::South).is_infinite());
        assert_eq!(shape_depth(&[[0.0; 6]], Direction::Up), f64::INFINITY);
    }
    #[test]
    fn stack_threshold_and_transported_extra_ticks() {
        for count in [1, 16, 17, 32, 33, 48, 49, 64] {
            let mut a = Processing::default();
            let mut b = Processing::default();
            let dropped = (1..1000)
                .find(|_| a.dropped(ProcessingType::Splashing, count, true))
                .unwrap();
            let held = (1..1000)
                .find(|_| b.transported(ProcessingType::Splashing, count, true))
                .unwrap();
            assert_eq!(dropped, 150 * ((count - 1) / 16 + 1) + 1);
            assert_eq!(held, dropped + 2);
        }
    }
}

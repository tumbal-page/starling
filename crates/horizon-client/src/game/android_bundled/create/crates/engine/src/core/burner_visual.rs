// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! BlazeBurnerBlockEntity animation and both pinned renderer specifications.
//! `Visual` retains Flywheel instance choices; `RendererFrame` models the
//! separate stateless BlazeBurnerRenderer.renderShared fallback. `RendererFrame`
//! is a source-reference/oracle boundary only: the current native adapter uses
//! `Visual`. Keeping both specifications does not select or detect a backend.
//! World queries, actual player bearing and renderer submission are adapters.
use crate::{
    burner::Heat,
    particles::{ParticleRandom, source_sin},
    placement::Direction,
};

pub fn shortest(mut from: f64, mut to: f64) -> f32 {
    from %= 360.0;
    to %= 360.0;
    ((((to - from) % 360.0 + 540.0) % 360.0) - 180.0) as f32
}
pub fn radians(degrees: f32) -> f32 {
    if degrees == 0.0 {
        0.0
    } else {
        (f64::from(degrees) / 180.0 * std::f64::consts::PI) as f32
    }
}
pub fn facing_angle(facing: Direction) -> f32 {
    let horizontal = match facing {
        Direction::South => 0.0,
        Direction::West => -90.0,
        Direction::North => 180.0,
        Direction::East => -270.0,
        _ => 0.0,
    };
    (horizontal + 180.0) % 360.0
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Chased {
    pub previous: f32,
    pub value: f32,
    angular: bool,
}
impl Chased {
    pub fn angular(value: f32) -> Self {
        Self {
            previous: value,
            value,
            angular: true,
        }
    }
    pub fn at(self, partial: f32) -> f32 {
        let delta = if self.angular {
            f64::from(shortest(f64::from(self.previous), f64::from(self.value)))
        } else {
            f64::from(self.value) - f64::from(self.previous)
        };
        (f64::from(self.previous) + delta * f64::from(partial)) as f32
    }
    fn tick(&mut self, mut target: f32, speed: f32, cap: f64) {
        if self.angular {
            target = self.value + shortest(f64::from(self.value), f64::from(target));
        }
        self.previous = self.value;
        let value = f64::from(self.value);
        let target = f64::from(target);
        self.value = if (value - target).abs() < 1e-5 {
            target as f32
        } else {
            (value + ((target - value) * f64::from(speed)).clamp(-cap, cap)) as f32
        };
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Animation {
    pub angle: Chased,
    pub head: Chased,
}
impl Animation {
    pub fn new(facing: Direction) -> Self {
        Self {
            angle: Chased::angular(facing_angle(facing)),
            head: Chased::default(),
        }
    }
    /// `look_target` is AngleHelper.deg(-Mth.atan2(dz, dx))-90, or zero
    /// when no visible local player exists. Virtual-world bearing is adapter-owned.
    pub fn tick(&mut self, active: bool, facing: Direction, look_target: f32) {
        if active {
            self.angle.tick(facing_angle(facing), 0.125, f64::MAX);
        } else {
            let target =
                self.angle.value + shortest(f64::from(self.angle.value), f64::from(look_target));
            self.angle.tick(target, 0.25, 5.0);
        }
        self.head.tick(if active { 1.0 } else { 0.0 }, 0.25, 0.25);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    Inert,
    Idle,
    Active,
    Super,
    SuperActive,
}
impl Head {
    pub fn path(self) -> &'static str {
        match self {
            Self::Inert => "create:block/blaze_burner/blaze/inert",
            Self::Idle => "create:block/blaze_burner/blaze/idle",
            Self::Active => "create:block/blaze_burner/blaze/active",
            Self::Super => "create:block/blaze_burner/blaze/super",
            Self::SuperActive => "create:block/blaze_burner/blaze/super_active",
        }
    }
}
pub fn head_model(heat: Heat, above: bool) -> Head {
    if heat >= Heat::Seething {
        if above {
            Head::SuperActive
        } else {
            Head::Super
        }
    } else if heat >= Heat::Fading {
        if above && heat >= Heat::Kindled {
            Head::Active
        } else {
            Head::Idle
        }
    } else {
        Head::Inert
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Flame {
    pub superheated: bool,
    pub speed: f32,
}
impl Flame {
    pub fn uv(self, time: f32) -> [f32; 2] {
        // Flywheel scrolling shader: fract(speed*renderTicks)*scale.
        let fract = |v: f32| v - v.floor();
        [
            fract((self.speed / 2.0) * time) * 0.5,
            fract(self.speed * time) * 0.5,
        ]
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub head: Head,
    pub head_y: f32,
    pub angle: f32,
    pub rods: Option<bool>,
    pub small_y: f32,
    pub large_y: f32,
    pub flame: Option<Flame>,
    pub goggles: bool,
    pub hat: Option<bool>,
}
/// Visual-owned instance lifetimes intentionally retain the source's model
/// choices until that instance is deleted/recreated. They are not saved NBT.
#[derive(Clone, Copy, Debug)]
pub struct Visual {
    heat: Heat,
    above: bool,
    rods: Option<bool>,
    flame: Option<Flame>,
    hat: Option<bool>,
}
impl Visual {
    pub fn new(valid_block_above: bool) -> Self {
        Self {
            heat: Heat::Smouldering,
            above: valid_block_above,
            rods: None,
            flame: None,
            hat: None,
        }
    }
    pub fn frame(&mut self, input: FrameInput, animation: Animation) -> Frame {
        let FrameInput {
            heat,
            partial,
            time,
            phase,
            goggles,
            hat,
            stock_keeper,
        } = input;
        let animation_value = animation.head.at(partial) * 0.175;
        let above = animation_value > 0.125;
        let heat = if stock_keeper && heat < Heat::Fading {
            Heat::Fading
        } else {
            heat
        };
        if self.above != above || self.heat != heat {
            self.above = above;
            if heat >= Heat::Fading && self.heat < Heat::Fading {
                self.rods = Some(heat == Heat::Seething);
            } else if heat < Heat::Fading && self.heat >= Heat::Fading {
                self.rods = None;
            }
            self.heat = heat;
        }
        if above && self.flame.is_none() {
            self.flame = Some(Flame {
                superheated: heat == Heat::Seething,
                speed: 1.0 / 32.0 + 1.0 / 64.0 * heat as u8 as f32,
            });
        } else if !above {
            self.flame = None;
        }
        if (hat || stock_keeper) && self.hat.is_none() {
            self.hat = Some(stock_keeper);
        } else if !hat && !stock_keeper {
            self.hat = None;
        }
        let render_tick = time + (phase % 13) as f32 * 16.0;
        let offset_mult = if heat >= Heat::Fading { 64.0 } else { 16.0 };
        let wave = |extra: f64| {
            source_sin(
                ((f64::from(render_tick / 16.0) + extra) % (2.0 * std::f64::consts::PI)) as f32,
            ) / offset_mult
        };
        Frame {
            head: head_model(heat, above),
            head_y: wave(0.0) - animation_value * 0.75,
            angle: radians(animation.angle.at(partial)),
            rods: self.rods,
            small_y: wave(std::f64::consts::PI) + animation_value + 0.125,
            large_y: wave(std::f64::consts::FRAC_PI_2) + animation_value - 3.0 / 16.0,
            flame: self.flame,
            goggles,
            hat: self.hat,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FrameInput {
    pub heat: Heat,
    pub partial: f32,
    pub time: f32,
    /// Actual client block-entity identity phase. No deterministic position hash.
    pub phase: i32,
    pub goggles: bool,
    pub hat: bool,
    pub stock_keeper: bool,
}

/// `BlazeBurnerRenderer.renderSafe` supplies block heat directly; its fallback
/// does not promote a cold stock keeper to FADING as the Flywheel visual does.
#[derive(Clone, Copy, Debug)]
pub struct RendererInput {
    pub heat: Heat,
    /// Already interpolated headAnimation.getValue(partial) * .175f.
    pub animation: f32,
    /// Already converted from the interpolated angle with AngleHelper.rad.
    pub angle: f32,
    pub time: f32,
    pub phase: i32,
    pub can_draw_flame: bool,
    pub goggles: bool,
    /// false=train hat, true=logistics hat, chosen by renderSafe's hat priority.
    pub hat: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goggles {
    Small,
    Large,
}

/// The fallback's UV operations use Java float multiplication first, followed
/// by a double fractional part. This differs from the scrolling shader's math.
#[derive(Clone, Copy, Debug)]
pub struct RendererFlame {
    pub superheated: bool,
    pub offset: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct RendererHat {
    pub logistics: bool,
    pub inert: bool,
}

/// Test/reference model of unchanged renderShared, without retained Flywheel
/// models. Not selected by the native adapter. Head/rods/goggles use solid;
/// hat/flame use cutoutMipped.
#[derive(Clone, Copy, Debug)]
pub struct RendererFrame {
    pub head: Head,
    pub head_y: f32,
    pub angle: f32,
    pub goggles: Option<Goggles>,
    pub hat: Option<RendererHat>,
    pub rods: Option<bool>,
    pub small_y: f32,
    pub large_y: f32,
    pub flame: Option<RendererFlame>,
}

impl RendererFrame {
    pub fn new(input: RendererInput) -> Self {
        let above = input.animation > 0.125;
        let head = head_model(input.heat, above);
        let render_tick = input.time + (input.phase % 13) as f32 * 16.0;
        let offset_mult = if input.heat >= Heat::Fading {
            64.0
        } else {
            16.0
        };
        let wave = |extra: f64| {
            source_sin(
                ((f64::from(render_tick / 16.0) + extra) % (2.0 * std::f64::consts::PI)) as f32,
            ) / offset_mult
        };
        let flame = (input.can_draw_flame && above).then(|| {
            let speed = 1.0 / 32.0 + 1.0 / 64.0 * input.heat as u8 as f32;
            let fraction = |value: f32| {
                let value = f64::from(value);
                ((value - value.floor()) / 2.0) as f32
            };
            RendererFlame {
                superheated: input.heat == Heat::Seething,
                offset: [
                    fraction(speed * input.time / 2.0),
                    fraction(speed * input.time),
                ],
            }
        });
        Self {
            head,
            head_y: wave(0.0) - input.animation * 0.75,
            angle: input.angle,
            goggles: input.goggles.then_some(if head == Head::Inert {
                Goggles::Small
            } else {
                Goggles::Large
            }),
            hat: input.hat.map(|logistics| RendererHat {
                logistics,
                inert: head == Head::Inert,
            }),
            rods: (input.heat >= Heat::Fading).then_some(input.heat == Heat::Seething),
            small_y: wave(std::f64::consts::PI) + input.animation + 0.125,
            large_y: wave(std::f64::consts::FRAC_PI_2) + input.animation - 3.0 / 16.0,
            flame,
        }
    }
}
pub trait Random: ParticleRandom {
    fn next_double(&mut self) -> f64;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleKind {
    Smoke,
    Flame,
    Soul,
}
#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub kind: ParticleKind,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}
fn offset(random: &mut impl Random, range: f32) -> [f64; 3] {
    crate::particles::offset_randomly(random, range)
}
fn normalized(mut v: [f64; 3]) -> [f64; 3] {
    v[1] *= 0.25;
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length < 1e-4 {
        [0.0; 3]
    } else {
        v.map(|v| v / length)
    }
}
pub fn particles(
    heat: Heat,
    center: [f64; 3],
    above_empty: bool,
    random: &mut impl Random,
) -> [Option<Particle>; 2] {
    let Some(smoke_position) = particle_start(heat, center, random) else {
        return [None; 2];
    };
    particle_finish(heat, center, smoke_position, above_empty, random)
}
/// The source queries the above collision shape only after this random gate.
pub fn particle_start(heat: Heat, center: [f64; 3], random: &mut impl Random) -> Option<[f64; 3]> {
    if heat == Heat::None {
        return None;
    }
    let mut v = offset(random, 0.125);
    v[1] = 0.0;
    v = std::array::from_fn(|i| center[i] + v[i]);
    if random.next_int(4) != 0 {
        return None;
    }
    Some(v)
}
pub fn particle_finish(
    heat: Heat,
    center: [f64; 3],
    smoke_position: [f64; 3],
    above_empty: bool,
    random: &mut impl Random,
) -> [Option<Particle>; 2] {
    let smoke = (above_empty || random.next_int(8) == 0).then_some(Particle {
        kind: ParticleKind::Smoke,
        position: smoke_position,
        velocity: [0.0; 3],
    });
    let motion = if above_empty {
        0.0625
    } else {
        random.next_double() * f64::from(0.0125f32)
    };
    let offset = normalized(offset(random, 0.5));
    let scale = if above_empty { 0.25 } else { 0.5 } + random.next_double() * 0.125;
    let mut position = std::array::from_fn(|i| center[i] + offset[i] * scale);
    position[1] += 0.5;
    let flame = (heat >= Heat::Fading).then_some(Particle {
        kind: if heat >= Heat::Seething {
            ParticleKind::Soul
        } else {
            ParticleKind::Flame
        },
        position,
        velocity: [0.0, motion, 0.0],
    });
    [smoke, flame]
}
pub fn particle_burst(
    soul: bool,
    center: [f64; 3],
    random: &mut impl Random,
    mut emit: impl FnMut(Particle),
) {
    for _ in 0..20 {
        let offset = normalized(offset(random, 0.5));
        let scale = 0.5 + random.next_double() * 0.125;
        let mut position = std::array::from_fn(|i| center[i] + offset[i] * scale);
        position[1] += 0.125;
        emit(Particle {
            kind: if soul {
                ParticleKind::Soul
            } else {
                ParticleKind::Flame
            },
            position,
            velocity: offset.map(|v| v / 32.0),
        });
    }
}

#[cfg(test)]
mod renderer_tests {
    use super::*;

    fn renderer_thread(test: impl FnOnce() + Send + 'static) {
        // The shared Mth sine table is constructed on the stack in debug
        // builds. Match an ordinary main-thread stack rather than the test
        // runner's smaller worker stack; renderer math itself is unchanged.
        std::thread::Builder::new()
            .stack_size(8 << 20)
            .spawn(test)
            .unwrap()
            .join()
            .unwrap();
    }

    fn input(heat: Heat) -> RendererInput {
        RendererInput {
            heat,
            animation: 0.175,
            angle: 0.0,
            time: 123.75,
            phase: 12,
            can_draw_flame: heat >= Heat::Fading,
            goggles: true,
            hat: Some(false),
        }
    }

    #[test]
    fn fallback_reselects_parts_and_cosmetics_from_current_heat() {
        renderer_thread(|| {
            let hot = RendererFrame::new(input(Heat::Seething));
            assert_eq!(hot.head, Head::SuperActive);
            assert_eq!(hot.goggles, Some(Goggles::Large));
            assert_eq!(hot.rods, Some(true));
            assert!(hot.flame.unwrap().superheated);
            assert!(!hot.hat.unwrap().inert);

            let warm = RendererFrame::new(input(Heat::Kindled));
            assert_eq!(warm.head, Head::Active);
            assert_eq!(warm.rods, Some(false));
            assert!(!warm.flame.unwrap().superheated);

            let cold = RendererFrame::new(input(Heat::Smouldering));
            assert_eq!(cold.head, Head::Inert);
            assert_eq!(cold.goggles, Some(Goggles::Small));
            assert!(cold.hat.unwrap().inert);
            assert!(cold.rods.is_none());
            assert!(cold.flame.is_none());
        });
    }

    #[test]
    fn fallback_flame_requires_permission_and_strict_above_threshold() {
        renderer_thread(|| {
            let mut state = input(Heat::Seething);
            state.animation = 0.125;
            assert!(RendererFrame::new(state).flame.is_none());
            state.animation = f32::from_bits(0.125_f32.to_bits() + 1);
            assert!(RendererFrame::new(state).flame.is_some());
            state.can_draw_flame = false;
            assert!(RendererFrame::new(state).flame.is_none());
        });
    }
}

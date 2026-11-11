// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! Create-specific particle controllers. Vanilla particle construction, lighting,
//! sprite-atlas lookup and rendering remain the platform adapter's responsibility.
use crate::{
    fan::{self, ProcessingType, Segment},
    kinetics::Axis,
    placement::Direction,
};
use std::sync::OnceLock;

/// Supply the particle's own random, or the level random where explicitly noted.
/// Keeping these separate preserves the source's draw ordering.
pub trait ParticleRandom {
    fn next_float(&mut self) -> f32;
    fn next_int(&mut self, bound: i32) -> i32;
}

pub const QUAD_SIZE_MULTIPLIER: f32 = 0.75;
pub const AIRFLOW_LIFETIME: i32 = 40;

pub fn mix_colors(a: u32, b: u32, amount: f32) -> u32 {
    [0, 8, 16, 24]
        .into_iter()
        .map(|shift| {
            let a = if shift == 24 {
                (a as i32) >> 24
            } else {
                ((a >> shift) & 255) as i32
            };
            let b = if shift == 24 {
                (b as i32) >> 24
            } else {
                ((b >> shift) & 255) as i32
            };
            ((a as f32 + (b - a) as f32 * amount) as i32 as u32) << shift
        })
        .fold(0, u32::wrapping_add)
}
pub fn offset_randomly(random: &mut impl ParticleRandom, range: f32) -> [f64; 3] {
    std::array::from_fn(|_| f64::from((random.next_float() - 0.5) * 2.0 * range))
}
fn sine_table() -> &'static [f32; 65536] {
    static TABLE: OnceLock<[f32; 65536]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| ((i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin()) as f32)
    })
}
/// Pinned Minecraft 1.21.1 `Mth.sin(float)`. Its float index multiplication is
/// different from the newer platform's double-index sine helper.
pub fn source_sin(angle: f32) -> f32 {
    sine_table()[((angle * 10430.378) as i32 & 65535) as usize]
}
fn rotate(offset: [f64; 3], degrees: f32, axis: Axis) -> [f64; 3] {
    if degrees == 0.0 {
        return offset;
    }
    // Catnip VecHelper.rotate delegates to the original Mth lookup table.
    let radians = (f64::from(degrees) / 180.0 * std::f64::consts::PI) as f32;
    let sin = f64::from(sine_table()[((radians * 10430.378) as i32 & 65535) as usize]);
    let cos = f64::from(sine_table()[((radians * 10430.378 + 16384.0) as i32 & 65535) as usize]);
    let [x, y, z] = offset;
    match axis {
        Axis::X => [x, y * cos - z * sin, z * cos + y * sin],
        Axis::Y => [x * cos + z * sin, y, z * cos - x * sin],
        Axis::Z => [x * cos - y * sin, y * cos + x * sin, z],
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    pub origin: [f64; 3],
    pub axis: Axis,
    pub speed: f32,
    pub radius: f32,
    pub target_radius: f32,
}
impl Rotation {
    /// RotationIndicatorParticle.move. Call with the base particle's current
    /// age/lifetime and AnimationTickHolder's integer ticks converted to float.
    pub fn position(&self, time: f32, age: i32, lifetime: i32) -> [f64; 3] {
        let mut angle = (time * self.speed) % 360.0
            - self.speed / 2.0 * age as f32 * (age as f32 / lifetime as f32);
        if self.speed < 0.0 && self.axis == Axis::Y {
            angle += 180.0;
        }
        let offset = if self.axis == Axis::Y {
            [f64::from(self.radius), 0.0, 0.0]
        } else {
            [0.0, f64::from(self.radius), 0.0]
        };
        let offset = rotate(offset, angle, self.axis);
        std::array::from_fn(|i| self.origin[i] + offset[i])
    }
    /// Run after the base SimpleAnimatedParticle tick/move, as in Java.
    pub fn tick_radius(&mut self) {
        self.radius += (self.target_radius - self.radius) * 0.1;
    }
}
pub fn rotation_lifetime(base: i32, random: &mut impl ParticleRandom) -> i32 {
    base + random.next_int(32)
}
pub fn rotation_visible(
    world_is_active: bool,
    player_present: bool,
    wearing_goggles: bool,
) -> bool {
    !world_is_active || player_present && wearing_goggles
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotationBurst {
    pub color: u32,
    pub speed: f32,
    pub count: i32,
    pub life_span: i32,
    pub initial_radius: f32,
    pub target_radius: f32,
    pub axis: Axis,
}
pub fn rotation_burst(
    rpm: f32,
    medium_speed: f32,
    fast_speed: f32,
    initial_radius: f32,
    target_radius: f32,
    axis: Axis,
) -> Option<RotationBurst> {
    if rpm == 0.0 {
        return None;
    }
    let (color, speed) = if rpm.abs() >= fast_speed {
        (0xFF55FF, 30)
    } else if rpm.abs() >= medium_speed {
        (0x0084FF, 20)
    } else if rpm.abs() >= 1.0 {
        (0x22FF22, 10)
    } else {
        (0, 0)
    };
    Some(RotationBurst {
        color,
        speed: speed as f32 * rpm.signum(),
        count: 20,
        life_span: 10,
        initial_radius,
        target_radius,
        axis,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Flame,
    LavaBlock,
    SoulFireFlame,
    Smoke,
    LargeSmoke,
    Bubble,
    BubblePop,
    Poof,
    BlueDust,
    Spit,
    Cloud,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extra {
    pub kind: Kind,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Morph {
    pub color: u32,
    pub alpha: f32,
}
pub fn morph(
    kind: ProcessingType,
    random: &mut impl ParticleRandom,
    position: [f64; 3],
    previous_velocity: [f64; 3],
    extras: &mut Vec<Extra>,
) -> Morph {
    use ProcessingType::*;
    let (a, b, alpha, kinds, thresholds, multiplier) = match kind {
        Blasting => (
            0xFF4400,
            0xFF8855,
            0.5,
            [Kind::Flame, Kind::LavaBlock],
            [1.0 / 32.0, 1.0 / 16.0],
            0.25,
        ),
        Haunting => (
            0,
            0x126568,
            1.0,
            [Kind::SoulFireFlame, Kind::Smoke],
            [1.0 / 128.0, 1.0 / 32.0],
            0.125,
        ),
        Smoking => (
            0,
            0x555555,
            1.0,
            [Kind::Smoke, Kind::LargeSmoke],
            [1.0 / 32.0, 1.0 / 32.0],
            0.125,
        ),
        Splashing => (
            0x4499FF,
            0x2277FF,
            1.0,
            [Kind::Bubble, Kind::BubblePop],
            [1.0 / 32.0, 1.0 / 32.0],
            0.125,
        ),
    };
    let color = mix_colors(a, b, random.next_float());
    for i in 0..2 {
        if random.next_float() < thresholds[i] {
            extras.push(Extra {
                kind: kinds[i],
                position,
                velocity: previous_velocity.map(|v| v * multiplier),
            });
        }
    }
    Morph { color, alpha }
}

pub struct AirflowContext<'a> {
    pub origin: [i32; 3],
    pub facing: Direction,
    pub pushing: bool,
    pub distance: f32,
    pub bounds: [[f64; 3]; 2],
    pub source_removed: bool,
    pub segments: &'a [Segment],
}
#[derive(Clone, Debug, PartialEq)]
pub struct Airflow {
    pub position: [f64; 3],
    pub previous: [f64; 3],
    pub velocity: [f64; 3],
    pub age: i32,
    pub lifetime: i32,
    pub color: u32,
    pub alpha: f32,
    pub sprite: i32,
    pub on_ground: bool,
    pub removed: bool,
}
impl Airflow {
    /// Call after vanilla base construction, with the resulting own RNG state.
    pub fn new(position: [f64; 3], random: &mut impl ParticleRandom) -> Self {
        let offset = offset_randomly(random, 0.25);
        let position = std::array::from_fn(|i| position[i] + offset[i]);
        Self {
            position,
            previous: position,
            velocity: [0.0; 3],
            age: 0,
            lifetime: AIRFLOW_LIFETIME,
            color: 0xEEEEEE,
            alpha: 0.25,
            sprite: 7,
            on_ground: false,
            removed: false,
        }
    }
    pub fn tick(
        &mut self,
        context: &AirflowContext<'_>,
        random: &mut impl ParticleRandom,
    ) -> Vec<Extra> {
        if self.removed {
            return Vec::new();
        }
        if context.source_removed {
            self.removed = true;
            return Vec::new();
        }
        self.previous = self.position;
        let age = self.age;
        self.age += 1;
        if age >= self.lifetime {
            self.removed = true;
            return Vec::new();
        }
        if (0..3).any(|i| {
            self.position[i] < context.bounds[0][i] - 0.25
                || self.position[i] >= context.bounds[1][i] + 0.25
        }) {
            self.removed = true;
            return Vec::new();
        }
        let direction = context.facing.vector();
        let distance = (0..3)
            .map(|i| {
                ((self.position[i] - (f64::from(context.origin[i]) + 0.5))
                    * f64::from(direction[i]))
                .powi(2)
            })
            .sum::<f64>()
            .sqrt()
            - 0.5;
        if distance > f64::from(context.distance) + 1.0 || distance < -0.25 {
            self.removed = true;
            return Vec::new();
        }
        let mut motion = direction.map(|v| f64::from(v) * 0.125);
        if !context.pushing {
            motion = motion.map(|v| -v);
        }
        motion = motion.map(|v| v * (f64::from(context.distance) - (distance - 1.0)) * 0.5);
        let mut extras = Vec::new();
        match fan::type_at(
            context.distance,
            context.pushing,
            context.segments,
            distance as f32,
        ) {
            None => {
                self.color = 0xEEEEEE;
                self.alpha = 0.25;
                self.sprite = ((distance / f64::from(context.distance)) * 8.0
                    + f64::from(random.next_int(4)))
                .clamp(0.0, 7.0) as i32;
            }
            Some(kind) => {
                let value = morph(kind, random, self.position, self.velocity, &mut extras);
                self.color = value.color;
                self.alpha = value.alpha;
                self.sprite = random.next_int(3);
            }
        }
        self.velocity = motion;
        if self.on_ground {
            self.velocity[0] *= 0.7;
            self.velocity[2] *= 0.7;
        }
        self.position = std::array::from_fn(|i| self.position[i] + self.velocity[i]);
        extras
    }
}

/// The level RNG draw used for AirFlowParticle's base gravity argument.
pub fn airflow_gravity(level_random: &mut impl ParticleRandom) -> f32 {
    level_random.next_float() * 0.5
}
pub fn airflow_spawn_position(
    origin: [i32; 3],
    facing: Direction,
    pushing: bool,
    distance: f32,
) -> [f64; 3] {
    let offset = if pushing { 0.5 } else { distance + 0.5 };
    let direction = facing.vector();
    std::array::from_fn(|i| {
        f64::from(origin[i]) + 0.5 + f64::from(direction[i]) * f64::from(offset)
    })
}
pub fn spawn_airflow(level_random: &mut impl ParticleRandom, density: f32) -> bool {
    level_random.next_float() < density
}

pub fn processing_particles(
    kind: ProcessingType,
    position: [f64; 3],
    random: &mut impl ParticleRandom,
) -> Vec<Extra> {
    if random.next_int(8) != 0 {
        return Vec::new();
    }
    let mut result = Vec::new();
    let event = |kind, position, velocity| Extra {
        kind,
        position,
        velocity,
    };
    match kind {
        ProcessingType::Blasting | ProcessingType::Smoking => {
            let mut at = position;
            at[1] += 0.25;
            result.push(event(
                if kind == ProcessingType::Blasting {
                    Kind::LargeSmoke
                } else {
                    Kind::Poof
                },
                at,
                [0.0, 1.0 / 16.0, 0.0],
            ));
        }
        ProcessingType::Haunting => {
            let mut offset = offset_randomly(random, 1.0);
            offset[1] *= 0.05;
            let length = offset.iter().map(|v| v * v).sum::<f64>().sqrt();
            offset = if length < 1.0e-4 {
                [0.0; 3]
            } else {
                offset.map(|v| v / length)
            };
            let position = std::array::from_fn(|i| position[i] + offset[i] * f64::from(0.15_f32));
            let mut at = position;
            at[1] += f64::from(0.45_f32);
            result.push(event(Kind::SoulFireFlame, at, [0.0; 3]));
            if random.next_int(2) == 0 {
                let mut at = position;
                at[1] += 0.25;
                result.push(event(Kind::Smoke, at, [0.0; 3]));
            }
        }
        ProcessingType::Splashing => {
            for kind in [Kind::BlueDust, Kind::Spit] {
                result.push(event(
                    kind,
                    [
                        position[0] + f64::from((random.next_float() - 0.5) * 0.5),
                        position[1] + 0.5,
                        position[2] + f64::from((random.next_float() - 0.5) * 0.5),
                    ],
                    [0.0, 1.0 / 8.0, 0.0],
                ));
            }
        }
    }
    result
}

/// HauntingType.affectEntity's client-only Horse branch. The caller supplies
/// Entity.getPosition(0), i.e. the previous/interpolated position at partial0.
pub fn horse_haunting_particles(
    position: [f64; 3],
    random: &mut impl ParticleRandom,
) -> Vec<Extra> {
    let mut offset = offset_randomly(random, 1.0);
    offset[1] *= 0.2;
    let length = offset.iter().map(|v| v * v).sum::<f64>().sqrt();
    offset = if length < 1.0e-4 {
        [0.0; 3]
    } else {
        offset.map(|v| v / length)
    };
    let mut at = position;
    at[1] += 0.5;
    at = std::array::from_fn(|i| at[i] + offset[i]);
    let mut result = vec![Extra {
        kind: Kind::SoulFireFlame,
        position: at,
        velocity: [0.0, f64::from(0.1_f32), 0.0],
    }];
    if random.next_int(3) == 0 {
        result.push(Extra {
            kind: Kind::LargeSmoke,
            position: [position[0], position[1] + 0.5, position[2]],
            velocity: [
                f64::from((random.next_float() - 0.5) * 0.5),
                f64::from(0.1_f32),
                f64::from((random.next_float() - 0.5) * 0.5),
            ],
        });
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StressBurst {
    pub smoke: bool,
    pub max_motion: f32,
    pub amount: i32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KineticEffects {
    pub overstressed_time: i32,
    pub overstressed_effect: f32,
    pub rotation_countdown: i32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KineticTick {
    pub rotation_due: bool,
    pub stress: Option<StressBurst>,
}
impl KineticEffects {
    pub fn queue_rotation(&mut self) {
        self.rotation_countdown = 2;
    }
    pub fn trigger_overstress(&mut self) {
        self.overstressed_time = if self.overstressed_time == 0 { 2 } else { 0 };
    }
    pub fn tick(&mut self, client: bool, overstressed: bool) -> KineticTick {
        let mut result = KineticTick::default();
        if client {
            if self.overstressed_time > 0 {
                self.overstressed_time -= 1;
                if self.overstressed_time == 0 {
                    self.overstressed_effect = if overstressed { 1.0 } else { -1.0 };
                    result.stress = Some(StressBurst {
                        smoke: overstressed,
                        max_motion: if overstressed { 0.2 } else { 0.075 },
                        amount: if overstressed { 5 } else { 2 },
                    });
                }
            }
            if self.overstressed_effect != 0.0 {
                self.overstressed_effect -= self.overstressed_effect * 0.1;
                if self.overstressed_effect.abs() < 1.0 / 128.0 {
                    self.overstressed_effect = 0.0;
                }
            }
        } else if self.rotation_countdown > 0 {
            self.rotation_countdown -= 1;
            result.rotation_due = self.rotation_countdown == 0;
        }
        result
    }
}

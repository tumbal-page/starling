// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! AirCurrent.tickAffectedEntities and AllFanProcessingTypes.affectEntity.
//! Entity mutation, damage mitigation and networking belong to the adapter.
use super::fan::{self, ProcessingType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageType {
    FanFire,
    FanLava,
    Drown,
}
impl DamageType {
    pub fn id(self) -> &'static str {
        match self {
            Self::FanFire => "create:fan_fire",
            Self::FanLava => "create:fan_lava",
            Self::Drown => "minecraft:drown",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub living: bool,
    pub horse: bool,
    pub fire_immune: bool,
    pub on_fire: bool,
    /// Enderman, snow golem or blaze, as tested by SplashingType.
    pub water_sensitive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub id: &'static str,
    pub ticks: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub particles: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Horse {
    #[default]
    None,
    Progress {
        value: i32,
        sound_pitch: Option<f32>,
    },
    Convert,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Effects {
    /// Entity.igniteForSeconds, expressed as ticks for the native adapter.
    pub ignite_ticks: i32,
    pub damage: Option<(DamageType, f32)>,
    pub statuses: [Option<Status>; 2],
    pub extinguish: bool,
    pub horse: Horse,
}

pub fn effects(kind: ProcessingType, facts: Facts, horse_progress: i32) -> Effects {
    let mut result = Effects::default();
    match kind {
        ProcessingType::Blasting | ProcessingType::Smoking if !facts.fire_immune => {
            let blasting = kind == ProcessingType::Blasting;
            result.ignite_ticks = if blasting { 200 } else { 40 };
            result.damage = Some(if blasting {
                (DamageType::FanLava, 4.0)
            } else {
                (DamageType::FanFire, 2.0)
            });
        }
        ProcessingType::Haunting => {
            if facts.living {
                result.statuses = [
                    Some(Status {
                        id: "minecraft:blindness",
                        ticks: 30,
                        amplifier: 0,
                        ambient: false,
                        particles: false,
                    }),
                    Some(Status {
                        id: "minecraft:slowness",
                        ticks: 20,
                        amplifier: 1,
                        ambient: false,
                        particles: false,
                    }),
                ];
            }
            if facts.horse {
                result.horse = if horse_progress < 100 {
                    Horse::Progress {
                        value: horse_progress.wrapping_add(1),
                        sound_pitch: (horse_progress % 10 == 0)
                            .then_some(1.5 * horse_progress as f32 / 100.0),
                    }
                } else {
                    Horse::Convert
                };
            }
        }
        ProcessingType::Splashing => {
            if facts.water_sensitive {
                result.damage = Some((DamageType::Drown, 2.0));
            }
            result.extinguish = facts.on_fire;
        }
        _ => {}
    }
    result
}

/// Level.getEntities excludes spectators; AirCurrent also removes dead entities
/// and creative players while flying. Creative players on the ground are moved.
pub fn eligible(alive: bool, spectator: bool, creative: bool, flying: bool) -> bool {
    alive && !spectator && !(creative && flying)
}

pub fn intersects(bounds: [[f64; 3]; 2], entity: [[f64; 3]; 2]) -> bool {
    (0..3).all(|axis| bounds[0][axis] < entity[1][axis] && bounds[1][axis] > entity[0][axis])
}

/// The source uses the feet position's distance from the fan center, including
/// transverse separation, and damps all three velocity axes toward the flow.
pub fn movement(
    velocity: [f64; 3],
    position: [f64; 3],
    origin: [f64; 3],
    flow: [i32; 3],
    rpm: f32,
    distance: f32,
    sneaking: bool,
) -> [f64; 3] {
    fan::accelerated(velocity, position, origin, flow, rpm, distance, sneaking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horse_conversion_occurs_after_a_hundred_progress_calls() {
        let facts = Facts {
            living: true,
            horse: true,
            ..Facts::default()
        };
        for progress in 0..100 {
            assert!(
                matches!(effects(ProcessingType::Haunting, facts, progress).horse,
                Horse::Progress { value, .. } if value == progress + 1)
            );
        }
        assert_eq!(
            effects(ProcessingType::Haunting, facts, 100).horse,
            Horse::Convert
        );
    }

    #[test]
    fn fire_immunity_and_water_sensitivity_are_independent() {
        let facts = Facts {
            living: true,
            fire_immune: true,
            water_sensitive: true,
            ..Facts::default()
        };
        assert_eq!(
            effects(ProcessingType::Blasting, facts, 0),
            Effects::default()
        );
        assert_eq!(
            effects(ProcessingType::Splashing, facts, 0).damage,
            Some((DamageType::Drown, 2.0))
        );
    }
}

// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! SoundScapes/SoundScape/ContinuousSound/RepeatingSound and AirCurrentSound.
use super::kinetics::Pos;
use std::collections::{BTreeMap, BTreeSet};

pub const UPDATE_INTERVAL: i32 = 5;
pub const MAX_SOURCE_DISTANCE: f64 = 16.0;
pub const DEFAULT_VOLUME_CAP: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Kinetic,
    Cog,
    Crushing,
    Milling,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PitchGroup {
    VeryLow,
    Low,
    Normal,
    High,
    VeryHigh,
}
pub type Key = (Group, PitchGroup);

pub fn pitch_group(pitch: f32) -> PitchGroup {
    // The Java literals are doubles; .7f is slightly below .70.
    match f64::from(pitch) {
        p if p < 0.70 => PitchGroup::VeryLow,
        p if p < 0.90 => PitchGroup::Low,
        p if p < 1.10 => PitchGroup::Normal,
        p if p < 1.30 => PitchGroup::High,
        _ => PitchGroup::VeryHigh,
    }
}
pub fn kinetic_pitch(rpm: f32) -> f32 {
    (rpm.abs() / 256.0 + 0.45).clamp(0.85, 1.0)
}
/// HandCrankBlockEntity.tickAudio: unwrapped SoundEntry gain and pitch.
pub fn crank_sound(ticks: i32, in_use: i32) -> Option<(f32, f32)> {
    (in_use > 0 && ticks % 10 == 0)
        .then(|| (in_use as f32 / 2.5, 0.65 + (10 - in_use) as f32 / 10.0))
}
pub fn in_range(camera: Pos, source: Pos) -> bool {
    camera
        .into_iter()
        .zip(source)
        .map(|(a, b)| (f64::from(a) - f64::from(b)).powi(2))
        .sum::<f64>()
        < MAX_SOURCE_DISTANCE * MAX_SOURCE_DISTANCE
}
pub fn volume(count: usize, camera: Option<[f64; 3]>, mean: [f64; 3], cap: f32) -> f32 {
    let distance_multiplier = camera.map_or(0.0, |camera| {
        let distance = camera
            .into_iter()
            .zip(mean)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        (2.0 + distance / MAX_SOURCE_DISTANCE * -2.0) as f32
    });
    // Match Mth.clamp's ordered branch even for a configured cap below min.
    let amount = count as f32 / 150.0;
    (if amount < 0.025 {
        0.025
    } else {
        amount.min(cap)
    }) * distance_multiplier
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub event: &'static str,
    pub relative_volume: f32,
    pub relative_pitch: f32,
    pub repeat_delay: Option<i32>,
}
const KINETIC: [Layer; 1] = [Layer {
    event: "minecraft:entity.minecart.inside",
    relative_volume: 0.25,
    relative_pitch: 1.0,
    repeat_delay: None,
}];
const COG: [Layer; 1] = [Layer {
    event: "create:cogs",
    relative_volume: 1.5,
    relative_pitch: 1.0,
    repeat_delay: None,
}];
const CRUSHING: [Layer; 3] = [
    Layer {
        event: "create:crushing_1",
        relative_volume: 1.545,
        relative_pitch: 0.75,
        repeat_delay: Some(1),
    },
    Layer {
        event: "create:crushing_2",
        relative_volume: 0.425,
        relative_pitch: 0.75,
        repeat_delay: Some(2),
    },
    Layer {
        event: "create:crushing_3",
        relative_volume: 2.0,
        relative_pitch: 1.75,
        repeat_delay: Some(2),
    },
];
pub fn layers(group: Group) -> &'static [Layer] {
    match group {
        Group::Kinetic => &KINETIC,
        Group::Cog => &COG,
        Group::Crushing => &CRUSHING,
        Group::Milling => &CRUSHING[..2],
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    pub key: Key,
    pub layer: usize,
    pub event: &'static str,
    pub position: [f64; 3],
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub distance_delay: bool,
}
#[derive(Clone, Debug)]
struct Scape {
    pitch: f32,
    mean: Option<[f64; 3]>,
}
#[derive(Clone, Debug, Default)]
pub struct SoundScapes {
    locations: BTreeMap<Key, BTreeSet<Pos>>,
    active: BTreeMap<Key, Scape>,
}
impl SoundScapes {
    pub fn play(
        &mut self,
        group: Group,
        pos: Pos,
        pitch: f32,
        camera_block: Pos,
        enabled: bool,
    ) -> bool {
        if !enabled || !in_range(camera_block, pos) {
            return false;
        }
        let key = (group, pitch_group(pitch));
        self.locations.entry(key).or_default().insert(pos);
        if self.active.contains_key(&key) {
            return false;
        }
        self.active.insert(key, Scape { pitch, mean: None });
        true
    }
    pub fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.active.keys().copied()
    }
    pub fn count(&self, key: Key) -> usize {
        self.locations.get(&key).map_or(0, BTreeSet::len)
    }
    pub fn mean(&mut self, key: Key) -> [f64; 3] {
        if let Some(mean) = self.active.get(&key).and_then(|s| s.mean) {
            return mean;
        }
        let mut mean = [0.0; 3];
        if let Some(locations) = self.locations.get(&key) {
            for p in locations {
                for i in 0..3 {
                    mean[i] += f64::from(p[i]) + 0.5;
                }
            }
            if !locations.is_empty() {
                // Java multiplies the doubles by the result of float 1f/n.
                let scale = f64::from(1.0_f32 / locations.len() as f32);
                mean = mean.map(|v| v * scale);
            }
        }
        if let Some(scape) = self.active.get_mut(&key) {
            scape.mean = Some(mean);
        }
        mean
    }
    pub fn sound(
        &mut self,
        key: Key,
        layer: usize,
        camera: Option<[f64; 3]>,
        cap: f32,
    ) -> Option<Sound> {
        let pitch = self.active.get(&key)?.pitch;
        let definition = *layers(key.0).get(layer)?;
        let mean = self.mean(key);
        Some(Sound {
            key,
            layer,
            event: definition.event,
            position: mean,
            volume: volume(self.count(key), camera, mean, cap) * definition.relative_volume,
            pitch: pitch * definition.relative_pitch,
            looping: definition.repeat_delay.is_none(),
            distance_delay: definition.repeat_delay.is_some(),
        })
    }
    pub fn tick(
        &mut self,
        ticks: i32,
        camera: Option<[f64; 3]>,
        enabled: bool,
        cap: f32,
    ) -> (Vec<Sound>, Vec<Key>) {
        let mut repeating = Vec::new();
        let mut removed = Vec::new();
        let update = ticks % UPDATE_INTERVAL == 0;
        let keys: Vec<_> = self.keys().collect();
        for key in keys {
            if update {
                self.active.get_mut(&key).unwrap().mean = None;
            }
            for (index, layer) in layers(key.0).iter().enumerate() {
                if layer
                    .repeat_delay
                    .is_some_and(|delay| ticks % delay.max(1) == 0)
                {
                    repeating.push(self.sound(key, index, camera, cap).unwrap());
                }
            }
        }
        if update {
            self.active.retain(|key, _| {
                let keep = enabled && self.locations.get(key).is_some_and(|p| !p.is_empty());
                if !keep {
                    removed.push(*key);
                }
                keep
            });
            self.locations.values_mut().for_each(BTreeSet::clear);
        }
        (repeating, removed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FanSound {
    pub volume: f32,
    pub pitch: f32,
    pub stopped: bool,
    in_current: bool,
}
impl Default for FanSound {
    fn default() -> Self {
        Self {
            volume: 0.01,
            pitch: 1.0,
            stopped: true,
            in_current: false,
        }
    }
}
impl FanSound {
    /// AirCurrent.Client.enableClientPlayerSound, after applying the force.
    /// Returns true exactly when a fresh looping instance must be played.
    pub fn enable(&mut self, rpm: f32, velocity: [f64; 3]) -> bool {
        self.in_current = true;
        let pitch =
            (velocity.into_iter().map(|v| v * v).sum::<f64>().sqrt() * 0.5).clamp(0.5, 2.0) as f32;
        let start = self.stopped;
        if start {
            self.volume = 0.01;
            self.stopped = false;
        }
        self.pitch = pitch;
        self.volume = (self.volume + 0.05).min((rpm.abs() / 128.0 * 0.4).clamp(0.01, 0.4));
        start
    }
    /// Client pre-tick: stop only on the tick after fading reached zero.
    pub fn begin_tick(&mut self) {
        if !self.in_current && !self.stopped {
            if self.volume == 0.0 {
                self.stopped = true;
            } else {
                self.volume = (self.volume - 0.05).max(0.0);
            }
        }
        self.in_current = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundscape_keeps_first_pitch_and_mean_until_source_five_tick_refresh() {
        let mut sounds = SoundScapes::default();
        let key = (Group::Cog, PitchGroup::Low);
        assert!(sounds.play(Group::Cog, [0, 0, 0], 0.85, [0, 0, 0], true));
        let first = sounds.sound(key, 0, Some([0.0; 3]), 0.1).unwrap();
        assert_eq!(first.position, [0.5; 3]);
        assert!(!sounds.play(Group::Cog, [2, 0, 0], 0.89, [0, 0, 0], true));
        sounds.play(Group::Cog, [2, 0, 0], 0.89, [0, 0, 0], true);
        assert_eq!(sounds.count(key), 2);
        assert_eq!(sounds.sound(key, 0, None, 0.1).unwrap().pitch, 0.85);
        assert_eq!(sounds.mean(key), [0.5; 3]);
        assert!(sounds.tick(4, None, true, 0.1).1.is_empty());
        assert_eq!(sounds.mean(key), [0.5; 3]);
        assert!(sounds.tick(5, None, true, 0.1).1.is_empty());
        // The source invalidates the cache, then clears the counter. A
        // continuous getter before the next registration sees an empty mean.
        assert_eq!(sounds.mean(key), [0.0; 3]);
        assert_eq!(sounds.count(key), 0);
        assert_eq!(sounds.tick(10, None, true, 0.1).1, [key]);
    }

    #[test]
    fn source_repeats_before_disabling_and_removing_a_stale_soundscape() {
        let mut sounds = SoundScapes::default();
        let key = (Group::Milling, PitchGroup::Normal);
        sounds.play(Group::Milling, [1, 0, 0], 1.0, [0, 0, 0], true);
        assert_eq!(sounds.tick(5, Some([0.0; 3]), true, 0.1).0.len(), 1);
        let (last_repeats, removed) = sounds.tick(10, Some([0.0; 3]), false, 0.1);
        assert_eq!(last_repeats.len(), 2);
        assert_eq!(last_repeats[0].position, [0.0; 3]);
        assert_eq!(last_repeats[0].volume, 0.05 * 1.545);
        assert_eq!(removed, [key]);
        assert_eq!(sounds.keys().count(), 0);
    }
}

// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Server heat/fuel transitions adapted from BlazeBurnerBlockEntity at the
// pinned Create6.0.10 revision. Data maps, item burn times, world writes and
// presentation belong to the platform adapter.

pub const MAX_HEAT_CAPACITY: i32 = 10_000;
pub const INSERTION_THRESHOLD: i32 = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Fuel {
    #[default]
    None,
    Normal,
    Special,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Heat {
    #[default]
    None,
    Smouldering,
    Fading,
    Kindled,
    Seething,
}
impl Heat {
    pub fn next_active(self) -> Self {
        match self {
            Self::None | Self::Seething => Self::Smouldering,
            Self::Smouldering => Self::Fading,
            Self::Fading => Self::Kindled,
            Self::Kindled => Self::Seething,
        }
    }
    pub fn light(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Smouldering => 8,
            _ => 15,
        }
    }
    pub fn comparator(self) -> u8 {
        (self as u8).saturating_sub(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub fuel: Fuel,
    pub burn_time: i32,
}
impl Candidate {
    /// Preserve source precedence: data maps, special tag, ordinary furnace
    /// fuel, then the regular fallback tag. No vanilla fuel list is duplicated.
    pub fn resolve(
        superheated: Option<i32>,
        regular: Option<i32>,
        special_tag: bool,
        furnace_time: i32,
        regular_tag: bool,
    ) -> Self {
        let (fuel, burn_time) = if let Some(time) = superheated {
            (Fuel::Special, time)
        } else if let Some(time) = regular {
            (Fuel::Normal, time)
        } else if special_tag {
            (Fuel::Special, 3200)
        } else if furnace_time > 0 {
            (Fuel::Normal, furnace_time)
        } else if regular_tag {
            (Fuel::Normal, 1600)
        } else {
            (Fuel::None, furnace_time)
        };
        Self { fuel, burn_time }
    }
}

/// A captured burner. An empty cage has no original block entity and is
/// deliberately not represented by this state. `heat` is the actual block
/// state, distinct from the fuel-derived heat while creative mode is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Burner {
    pub fuel: Fuel,
    pub remaining: i32,
    pub creative: bool,
    pub heat: Heat,
}
impl Default for Burner {
    fn default() -> Self {
        Self {
            fuel: Fuel::None,
            remaining: 0,
            creative: false,
            heat: Heat::Smouldering,
        }
    }
}
impl Burner {
    pub fn calculated_heat(self) -> Heat {
        match self.fuel {
            Fuel::None => Heat::Smouldering,
            Fuel::Special => Heat::Seething,
            Fuel::Normal if f64::from(self.remaining) / f64::from(MAX_HEAT_CAPACITY) < 0.0125 => {
                Heat::Fading
            }
            Fuel::Normal => Heat::Kindled,
        }
    }
    fn set_heat(&mut self, heat: Heat, changed: &mut impl FnMut(Heat)) {
        if self.heat != heat {
            self.heat = heat;
            changed(heat);
        }
    }
    /// One ordinary authoritative block-entity tick. The callback retains
    /// both source block updates on the last normal-fuel tick, without a
    /// temporary allocation or a synthetic single final transition.
    pub fn tick(&mut self, mut changed: impl FnMut(Heat)) {
        if self.creative {
            return;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
        }
        if self.fuel == Fuel::Normal {
            self.set_heat(self.calculated_heat(), &mut changed);
        }
        if self.remaining > 0 {
            return;
        }
        if self.fuel == Fuel::Special {
            self.fuel = Fuel::Normal;
            self.remaining = MAX_HEAT_CAPACITY / 2;
        } else {
            self.fuel = Fuel::None;
        }
        self.set_heat(self.calculated_heat(), &mut changed);
    }
    /// Source tryUpdateFuel's server calculation. Successful overflow at the
    /// cap still accepts an item; simulation has no state or callback effects.
    /// Java int addition is intentionally wrapping, including unusual saved
    /// burn times or externally supplied fuel data.
    pub fn insert(
        &mut self,
        candidate: Candidate,
        force_overflow: bool,
        simulate: bool,
        mut changed: impl FnMut(Heat),
    ) -> bool {
        if self.creative || candidate.fuel == Fuel::None || candidate.fuel < self.fuel {
            return false;
        }
        let mut time = candidate.burn_time;
        if candidate.fuel == self.fuel {
            if self.remaining <= INSERTION_THRESHOLD {
                time = time.wrapping_add(self.remaining);
            } else if force_overflow && candidate.fuel == Fuel::Normal {
                time = if self.remaining < MAX_HEAT_CAPACITY {
                    self.remaining.wrapping_add(time).min(MAX_HEAT_CAPACITY)
                } else {
                    self.remaining
                };
            } else {
                return false;
            }
        }
        if !simulate {
            self.fuel = candidate.fuel;
            self.remaining = time;
            self.set_heat(self.calculated_heat(), &mut changed);
        }
        true
    }
    /// Creative cake cycles the actual heat; the server skips FADING.
    pub fn apply_creative(&mut self, mut changed: impl FnMut(Heat)) {
        self.fuel = Fuel::None;
        self.remaining = 0;
        self.creative = true;
        let mut next = self.heat.next_active();
        if next == Heat::Fading {
            next = next.next_active();
        }
        self.set_heat(next, &mut changed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuel_precedence_and_fallback_do_not_invent_a_furnace_table() {
        assert_eq!(
            Candidate::resolve(Some(7), Some(8), true, 9, true),
            Candidate {
                fuel: Fuel::Special,
                burn_time: 7
            }
        );
        assert_eq!(
            Candidate::resolve(None, Some(8), true, 9, true),
            Candidate {
                fuel: Fuel::Normal,
                burn_time: 8
            }
        );
        assert_eq!(
            Candidate::resolve(None, None, true, 9, true).burn_time,
            3200
        );
        assert_eq!(Candidate::resolve(None, None, false, 9, true).burn_time, 9);
        assert_eq!(
            Candidate::resolve(None, None, false, 0, true).burn_time,
            1600
        );
        assert_eq!(
            Candidate::resolve(None, None, false, 0, false).fuel,
            Fuel::None
        );
    }
    #[test]
    fn special_expires_into_five_thousand_normal_ticks() {
        let mut state = Burner {
            fuel: Fuel::Special,
            remaining: 1,
            heat: Heat::Seething,
            ..Default::default()
        };
        let mut updates = Vec::new();
        state.tick(|heat| updates.push(heat));
        assert_eq!(
            (state.fuel, state.remaining, state.heat),
            (Fuel::Normal, 5000, Heat::Kindled)
        );
        assert_eq!(updates, [Heat::Kindled]);
    }
    #[test]
    fn last_normal_tick_retains_both_source_heat_notifications() {
        let mut state = Burner {
            fuel: Fuel::Normal,
            remaining: 1,
            heat: Heat::Kindled,
            ..Default::default()
        };
        let mut updates = Vec::new();
        state.tick(|heat| updates.push(heat));
        assert_eq!(updates, [Heat::Fading, Heat::Smouldering]);
        assert_eq!((state.fuel, state.remaining), (Fuel::None, 0));
    }
    #[test]
    fn simulation_is_read_only_and_real_player_overflow_at_cap_still_accepts() {
        let mut state = Burner {
            fuel: Fuel::Normal,
            remaining: MAX_HEAT_CAPACITY,
            heat: Heat::Kindled,
            ..Default::default()
        };
        let original = state;
        let coal = Candidate {
            fuel: Fuel::Normal,
            burn_time: 1600,
        };
        assert!(state.insert(coal, true, true, |_| panic!("simulate must not update")));
        assert_eq!(state, original);
        assert!(!state.insert(coal, false, false, |_| {}));
        assert!(state.insert(coal, true, false, |_| {}));
        assert_eq!(state.remaining, MAX_HEAT_CAPACITY);
    }
    #[test]
    fn creative_cake_cycles_three_levels_without_fuel_ticks() {
        let mut state = Burner::default();
        let mut levels = Vec::new();
        for _ in 0..4 {
            state.apply_creative(|heat| levels.push(heat));
            let before = state;
            state.tick(|_| panic!("creative burner must not tick"));
            assert_eq!(state, before);
        }
        assert_eq!(
            levels,
            [
                Heat::Kindled,
                Heat::Seething,
                Heat::Smouldering,
                Heat::Kindled
            ]
        );
    }
}

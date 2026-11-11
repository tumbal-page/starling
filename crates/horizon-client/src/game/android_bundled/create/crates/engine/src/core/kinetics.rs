// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// RotationPropagator connection equations and KineticNetwork stress accounting.
use std::collections::{BTreeMap, VecDeque};
pub type Pos = [i32; 3];
pub const MAX_NODES: usize = 4096;
pub const MAX_RPM: f32 = 256.0;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Motor {
        facing: Pos,
        rpm: f32,
        capacity: f32,
    },
    WaterWheel {
        rpm: f32,
        capacity: f32,
    },
    Shaft,
    /// BeltBlockEntity propagates unchanged RPM between segments sharing a
    /// controller. Only pulley segments expose the ordinary shaft axis.
    Belt {
        controller: Pos,
        index: i32,
        pulley: bool,
        diagonal: bool,
    },
    Cog {
        large: bool,
    },
    Press,
    Millstone,
    /// MechanicalMixerBlock is a fixed-Y small cog without a shaft port.
    Mixer,
    Consumer {
        facing: Pos,
        impact: f32,
    },
    Gearbox,
    /// Enclosed chains link along their state-defined connection axis at 1:1,
    /// including links between different shaft axes (ChainDriveBlock).
    Chain {
        state: crate::chain_drive::State,
    },
    Clutch {
        powered: bool,
    },
    Gearshift {
        powered: bool,
    },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub pos: Pos,
    pub axis: Axis,
    pub kind: Kind,
}
impl Node {
    pub fn shaft_port(self, delta: Pos) -> bool {
        if delta.iter().map(|v| v.abs()).sum::<i32>() != 1 {
            return false;
        }
        match self.kind {
            Kind::Motor { facing, .. } => delta == facing,
            Kind::Consumer { facing, .. } => delta == facing,
            Kind::Millstone => delta == [0, -1, 0],
            Kind::Mixer => false,
            Kind::Belt { pulley: false, .. } => false,
            Kind::Gearbox => delta[self.axis.index()] == 0,
            _ => delta[self.axis.index()].abs() == 1,
        }
    }
    /// Rotation at this face / local reference rotation. A gearbox's reference
    /// is its first positive shaft axis. This is a basis, not its NBT Speed.
    pub fn port_modifier(self, delta: Pos) -> f32 {
        let Some(axis) = delta.iter().position(|v| *v != 0) else {
            return 1.0;
        };
        match self.kind {
            Kind::Gearbox => {
                let reference = (0..3).find(|i| *i != self.axis.index()).unwrap();
                delta[axis] as f32 * if axis == reference { 1.0 } else { -1.0 }
            }
            Kind::Gearshift { powered: true } if delta[axis] < 0 => -1.0,
            _ => 1.0,
        }
    }
    fn half(self, delta: Pos) -> u8 {
        if matches!(self.kind, Kind::Clutch { powered: true }) {
            u8::from(delta[self.axis.index()] < 0)
        } else {
            0
        }
    }
    fn impact(self) -> f32 {
        match self.kind {
            Kind::Press => 8.0,
            Kind::Millstone | Kind::Mixer => 4.0,
            Kind::Consumer { impact, .. } => impact,
            _ => 0.0,
        }
    }
    fn cog(self) -> Option<bool> {
        match self.kind {
            Kind::Cog { large } => Some(large),
            Kind::Millstone | Kind::Mixer => Some(false),
            _ => None,
        }
    }
}
/// Target reference RPM / source reference RPM. A powered clutch's two
/// independent shaft halves are separated by the graph, not by deleting ports.
pub fn ratio(from: Node, to: Node) -> Option<f32> {
    let delta: Pos = std::array::from_fn(|i| to.pos[i] - from.pos[i]);
    let distance: i32 = delta.iter().map(|n| n.abs()).sum();
    if distance == 1 && from.shaft_port(delta) && to.shaft_port(delta.map(|v| -v)) {
        return Some(from.port_modifier(delta) / to.port_modifier(delta.map(|v| -v)));
    }
    if distance == 1
        && let (Kind::Chain { state: a }, Kind::Chain { state: b }) = (from.kind, to.kind)
        && let Some(direction) = crate::placement::Direction::from_vector(delta)
        && a.connected(b, direction)
    {
        return Some(1.0);
    }
    // BeltBlockEntity.propagateRotationTo: custom, non-axis links carry 1:1
    // rotation, including sloped neighbors. There is no slope sign inversion.
    if let (
        Kind::Belt {
            controller: a,
            diagonal,
            ..
        },
        Kind::Belt { controller: b, .. },
    ) = (from.kind, to.kind)
        && a == b
        && (distance == 1 || diagonal && distance == 2)
    {
        return Some(1.0);
    }
    if from.kind == (Kind::Cog { large: true })
        && to.kind == (Kind::Cog { large: true })
        && from.axis != to.axis
    {
        let a = from.axis.index();
        let b = to.axis.index();
        if distance == 2 && delta[a].abs() == 1 && delta[b].abs() == 1 {
            return Some(if (delta[a] > 0) ^ (delta[b] > 0) {
                -1.0
            } else {
                1.0
            });
        }
    }
    if from.axis != to.axis || delta[from.axis.index()] != 0 {
        return None;
    }
    match (from.cog(), to.cog()) {
        (Some(false), Some(false)) if distance == 1 => Some(-1.0),
        (Some(a), Some(b))
            if a != b && distance == 2 && delta.iter().filter(|n| n.abs() == 1).count() == 2 =>
        {
            Some(if a { -2.0 } else { -0.5 })
        }
        _ => None,
    }
}

#[cfg(test)]
mod belt_tests {
    use super::*;

    #[test]
    fn sloped_controller_chain_carries_rpm_without_inversion_and_only_pulleys_have_ports() {
        for (axis, chain_step, motor_offset) in [
            (Axis::X, [0, 0, 1], [1, 0, 0]),
            (Axis::X, [0, 1, 1], [1, 0, 0]),
            (Axis::X, [0, -1, 1], [1, 0, 0]),
            (Axis::Z, [1, 1, 0], [0, 0, 1]),
            (Axis::Z, [1, -1, 0], [0, 0, 1]),
            (Axis::Y, [1, 0, 0], [0, 1, 0]),
            (Axis::X, [0, 1, 0], [1, 0, 0]),
        ] {
            let controller = [0; 3];
            let belt = |index, pulley| Node {
                pos: chain_step.map(|v| v * index),
                axis,
                kind: Kind::Belt {
                    controller,
                    index,
                    pulley,
                    diagonal: chain_step[1] != 0,
                },
            };
            let first = belt(0, true);
            let middle = belt(1, false);
            let end = belt(2, true);
            assert!(first.shaft_port(motor_offset));
            assert!(!middle.shaft_port(motor_offset));
            assert_eq!(ratio(first, middle), Some(1.0));
            assert_eq!(ratio(middle, end), Some(1.0));
            let motor = Node {
                pos: motor_offset,
                axis,
                kind: Kind::Motor {
                    facing: motor_offset.map(|v| -v),
                    rpm: -128.0,
                    capacity: 16.0,
                },
            };
            let nodes = [first, middle, end, motor];
            let network = solve(first, |p| nodes.iter().find(|n| n.pos == p).copied());
            for node in [first, middle, end] {
                assert_eq!(network.speed(node.pos), -128.0);
            }
            let stranger = Node {
                kind: Kind::Belt {
                    controller: [10; 3],
                    index: 1,
                    pulley: false,
                    diagonal: true,
                },
                ..middle
            };
            assert_eq!(ratio(first, stranger), None);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Running,
    Overstressed,
    ConflictingSources,
    InconsistentCycle,
    TooFast,
    TooLarge,
}
#[derive(Clone, Debug)]
pub struct Network {
    pub status: Status,
    pub capacity: f32,
    pub stress: f32,
    pub theoretical: BTreeMap<Pos, f32>,
    pub ports: BTreeMap<(Pos, Pos), f32>,
    pub vertices: Vec<(Pos, u8)>,
}
impl Network {
    pub fn speed(&self, pos: Pos) -> f32 {
        if self.status == Status::Running {
            self.theoretical.get(&pos).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    }
    pub fn port_speed(&self, pos: Pos, direction: Pos) -> f32 {
        if self.status == Status::Running {
            self.ports.get(&(pos, direction)).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    }
}
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.0001 * a.abs().max(b.abs()).max(1.0)
}
pub fn solve(start: Node, read: impl FnMut(Pos) -> Option<Node>) -> Network {
    solve_half(start, 0, read)
}
/// Every powered clutch has two vertices. Its shafts remain connected to their
/// own neighbors but cannot transmit across the open clutch. No chunks are loaded.
pub fn solve_half(start: Node, half: u8, mut read: impl FnMut(Pos) -> Option<Node>) -> Network {
    let first = (start.pos, half);
    let mut nodes = BTreeMap::from([(first, start)]);
    let mut factors = BTreeMap::from([(first, 1.0_f32)]);
    let mut queue = VecDeque::from([(first, start)]);
    let mut failure = None;
    while let Some((key, from)) = queue.pop_front() {
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    let offset: Pos = [x, y, z];
                    let distance: i32 = offset.iter().map(|n| n.abs()).sum();
                    if !(1..=2).contains(&distance) || from.half(offset) != key.1 {
                        continue;
                    }
                    let pos = std::array::from_fn(|i| from.pos[i] + offset[i]);
                    let Some(to) = read(pos) else {
                        continue;
                    };
                    let Some(ratio) = ratio(from, to) else {
                        continue;
                    };
                    let target = (pos, to.half(offset.map(|v| -v)));
                    let factor = factors[&key] * ratio;
                    if let Some(old) = factors.get(&target) {
                        if !close(*old, factor) {
                            failure = Some(Status::InconsistentCycle);
                        }
                    } else {
                        if nodes.len() >= MAX_NODES {
                            failure = Some(Status::TooLarge);
                            queue.clear();
                            break;
                        }
                        nodes.insert(target, to);
                        factors.insert(target, factor);
                        queue.push_back((target, to));
                    }
                }
            }
        }
    }
    let mut base = None;
    let mut capacity = 0.0;
    for (key, node) in &nodes {
        if let Kind::Motor {
            rpm,
            capacity: per_rpm,
            ..
        }
        | Kind::WaterWheel {
            rpm,
            capacity: per_rpm,
        } = node.kind
        {
            if !rpm.is_finite() || !per_rpm.is_finite() || per_rpm < 0.0 {
                failure = Some(Status::ConflictingSources);
                continue;
            }
            if rpm == 0.0 {
                continue;
            }
            let proposed = rpm / factors[key];
            if base.is_some_and(|old: f32| old.signum() != proposed.signum()) {
                failure = Some(Status::ConflictingSources);
            }
            // RotationPropagator propagates the faster compatible source back
            // through the slower drive. Merely differing RPM is not a conflict.
            if base.is_none_or(|old| proposed.abs() > old.abs()) {
                base = Some(proposed);
            }
            capacity += per_rpm * rpm.abs();
        }
    }
    let base = base.unwrap_or(0.0);
    let stress = nodes
        .iter()
        .map(|(key, n)| n.impact() * (base * factors[key]).abs())
        .sum();
    let status = failure.unwrap_or_else(|| {
        if factors
            .values()
            .any(|factor| !(base * factor).is_finite() || (base * factor).abs() > MAX_RPM)
        {
            Status::TooFast
        } else if base == 0.0 {
            Status::Idle
        } else if stress > capacity {
            Status::Overstressed
        } else {
            Status::Running
        }
    });
    let mut theoretical = BTreeMap::new();
    let mut ports = BTreeMap::new();
    let vertices = nodes.keys().copied().collect();
    for (key, n) in nodes {
        let speed = base * factors[&key];
        theoretical.entry(n.pos).or_insert(speed);
        for d in [
            [-1, 0, 0],
            [1, 0, 0],
            [0, -1, 0],
            [0, 1, 0],
            [0, 0, -1],
            [0, 0, 1],
        ] {
            if n.shaft_port(d) && n.half(d) == key.1 {
                ports.insert((n.pos, d), speed * n.port_modifier(d));
            }
        }
    }
    Network {
        status,
        capacity,
        stress,
        theoretical,
        ports,
        vertices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(pos: Pos, kind: Kind) -> Node {
        Node {
            pos,
            axis: Axis::X,
            kind,
        }
    }
    fn motor(pos: Pos, rpm: f32, capacity: f32) -> Node {
        node(
            pos,
            Kind::Motor {
                facing: [1, 0, 0],
                rpm,
                capacity,
            },
        )
    }
    fn network(nodes: &[Node]) -> Network {
        solve(nodes[0], |p| nodes.iter().copied().find(|n| n.pos == p))
    }

    #[test]
    fn enclosed_chain_transmits_without_inversion_between_different_shaft_axes() {
        use crate::chain_drive::{Part, State};
        let nodes = [
            motor([0, 0, 0], -32.0, 16384.0),
            Node {
                pos: [1, 0, 0],
                axis: Axis::X,
                kind: Kind::Chain {
                    state: State {
                        axis: Axis::X,
                        first: false,
                        part: Part::Start,
                    },
                },
            },
            Node {
                pos: [1, 0, 1],
                axis: Axis::Y,
                kind: Kind::Chain {
                    state: State {
                        axis: Axis::Y,
                        first: false,
                        part: Part::End,
                    },
                },
            },
            Node {
                pos: [1, 1, 1],
                axis: Axis::Y,
                kind: Kind::Shaft,
            },
            Node {
                pos: [1, 2, 1],
                axis: Axis::Y,
                kind: Kind::Consumer {
                    facing: [0, -1, 0],
                    impact: 2.0,
                },
            },
        ];
        let connected = network(&nodes);
        assert_eq!(connected.status, Status::Running);
        assert_eq!(connected.speed([1, 2, 1]), -32.0);
        assert_eq!(connected.stress, 64.0);
        let disconnected = network(&[nodes[0], nodes[1], nodes[3], nodes[4]]);
        assert_eq!(disconnected.speed([1, 2, 1]), 0.0);
        assert!(!disconnected.theoretical.contains_key(&[1, 2, 1]));
    }

    #[test]
    fn enclosed_chain_uses_shaft_ports_and_does_not_mesh_with_cogs() {
        use crate::chain_drive::State;
        let chain = Node {
            pos: [0, 0, 0],
            axis: Axis::X,
            kind: Kind::Chain {
                state: State::new(Axis::X),
            },
        };
        let shaft = node([1, 0, 0], Kind::Shaft);
        assert_eq!(ratio(chain, shaft), Some(1.0));
        assert_eq!(ratio(shaft, chain), Some(1.0));
        let cog = Node {
            pos: [0, 0, 1],
            axis: Axis::X,
            kind: Kind::Cog { large: false },
        };
        assert_eq!(ratio(chain, cog), None);
        assert_eq!(ratio(cog, chain), None);
    }

    #[test]
    fn gearbox_routes_four_ports_with_original_direction_signs() {
        let nodes = [
            motor([0, 0, 0], 16.0, 16384.0),
            node([1, 0, 0], Kind::Gearbox),
            node([2, 0, 0], Kind::Shaft),
            Node {
                pos: [1, 0, 1],
                axis: Axis::Z,
                kind: Kind::Shaft,
            },
        ];
        let mut nodes = nodes;
        nodes[1].axis = Axis::Y;
        let n = network(&nodes);
        assert_eq!(n.speed([2, 0, 0]), -16.0);
        assert_eq!(n.speed([1, 0, 1]), 16.0);
        assert_eq!(n.port_speed([1, 0, 0], [-1, 0, 0]), 16.0);
    }
    #[test]
    fn open_clutch_keeps_input_half_rotating_and_isolates_two_independent_sources() {
        let nodes = [
            motor([0, 0, 0], 16.0, 16384.0),
            node([1, 0, 0], Kind::Clutch { powered: true }),
            node([2, 0, 0], Kind::Shaft),
            Node {
                pos: [3, 0, 0],
                axis: Axis::X,
                kind: Kind::Motor {
                    facing: [-1, 0, 0],
                    rpm: 64.0,
                    capacity: 16384.0,
                },
            },
        ];
        let left = network(&nodes);
        assert_eq!(left.status, Status::Running);
        assert_eq!(left.port_speed([1, 0, 0], [-1, 0, 0]), 16.0);
        assert!(!left.theoretical.contains_key(&[2, 0, 0]));
        let right = solve_half(nodes[1], 0, |p| nodes.iter().copied().find(|n| n.pos == p));
        assert_eq!(right.speed([2, 0, 0]), 64.0);
        assert_eq!(right.port_speed([1, 0, 0], [1, 0, 0]), 64.0);
    }
    #[test]
    fn clutch_engages_and_gearshift_reverses_only_when_powered() {
        for (kind, expected) in [
            (Kind::Clutch { powered: false }, 16.0),
            (Kind::Gearshift { powered: false }, 16.0),
            (Kind::Gearshift { powered: true }, -16.0),
        ] {
            let n = network(&[
                motor([0, 0, 0], 16.0, 16384.0),
                node([1, 0, 0], kind),
                node([2, 0, 0], Kind::Shaft),
            ]);
            assert_eq!(n.speed([2, 0, 0]), expected);
        }
    }
    #[test]
    fn perpendicular_large_gears_preserve_each_quadrants_sign() {
        for a in [Axis::X, Axis::Y, Axis::Z] {
            for b in [Axis::X, Axis::Y, Axis::Z] {
                if a == b {
                    continue;
                }
                for da in [-1, 1] {
                    for db in [-1, 1] {
                        let from = Node {
                            pos: [0; 3],
                            axis: a,
                            kind: Kind::Cog { large: true },
                        };
                        let mut to = Node {
                            pos: [0; 3],
                            axis: b,
                            kind: Kind::Cog { large: true },
                        };
                        to.pos[a.index()] = da;
                        to.pos[b.index()] = db;
                        assert_eq!(
                            ratio(from, to),
                            Some(if (da > 0) ^ (db > 0) { -1.0 } else { 1.0 })
                        );
                        assert_eq!(ratio(to, from), ratio(from, to));
                    }
                }
            }
        }
    }

    #[test]
    fn shaft_and_press_share_speed_and_scale_stress_by_absolute_rpm() {
        let n = network(&[
            motor([0, 0, 0], -32.0, 16.0),
            node([1, 0, 0], Kind::Shaft),
            node([2, 0, 0], Kind::Press),
        ]);
        assert_eq!(n.status, Status::Running);
        assert_eq!(n.speed([2, 0, 0]), -32.0);
        assert_eq!((n.capacity, n.stress), (512.0, 256.0));
    }

    #[test]
    fn overstress_stops_the_component_but_keeps_theoretical_rpm() {
        let n = network(&[motor([0, 0, 0], 16.0, 4.0), node([1, 0, 0], Kind::Press)]);
        assert_eq!(n.status, Status::Overstressed);
        assert_eq!(n.speed([1, 0, 0]), 0.0);
        assert_eq!(n.theoretical[&[1, 0, 0]], 16.0);
    }

    #[test]
    fn meshing_small_cogs_reverse_rotation() {
        let cog = Kind::Cog { large: false };
        let n = network(&[
            motor([0, 0, 0], 16.0, 16.0),
            node([1, 0, 0], cog),
            node([1, 1, 0], cog),
            node([2, 1, 0], Kind::Press),
        ]);
        assert_eq!(n.speed([2, 1, 0]), -16.0);
    }

    #[test]
    fn large_to_small_doubles_rpm_and_reverses_it() {
        let n = network(&[
            motor([0, 0, 0], 16.0, 16.0),
            node([1, 0, 0], Kind::Cog { large: true }),
            node([1, 1, 1], Kind::Cog { large: false }),
            node([2, 1, 1], Kind::Press),
        ]);
        assert_eq!(n.speed([2, 1, 1]), -32.0);
        assert_eq!(n.stress, 256.0);
    }

    #[test]
    fn removing_a_shaft_disconnects_the_press() {
        let press = node([2, 0, 0], Kind::Press);
        let n = solve(press, |p| (p == [0, 0, 0]).then(|| motor(p, 16.0, 16.0)));
        assert_eq!(n.status, Status::Idle);
        assert_eq!(n.speed(press.pos), 0.0);
    }

    #[test]
    fn motor_has_only_its_facing_shaft_port() {
        let n = network(&[motor([0, 0, 0], 16.0, 16.0), node([-1, 0, 0], Kind::Shaft)]);
        assert!(!n.theoretical.contains_key(&[-1, 0, 0]));
    }

    #[test]
    fn millstone_meshes_as_a_small_cog_but_has_only_a_bottom_shaft() {
        let mill = Node {
            pos: [0, 0, 0],
            axis: Axis::Y,
            kind: Kind::Millstone,
        };
        for (delta, expected) in [([0, -1, 0], true), ([0, 1, 0], false), ([1, 0, 0], false)] {
            assert_eq!(mill.shaft_port(delta), expected);
        }
        let small = Node {
            pos: [1, 0, 0],
            axis: Axis::Y,
            kind: Kind::Cog { large: false },
        };
        assert_eq!(ratio(mill, small), Some(-1.0));
        assert_eq!(ratio(small, mill), Some(-1.0));
        let large = Node {
            pos: [1, 0, 1],
            axis: Axis::Y,
            kind: Kind::Cog { large: true },
        };
        assert_eq!(ratio(large, mill), Some(-2.0));
        assert_eq!(ratio(mill, large), Some(-0.5));
    }

    #[test]
    fn mixer_is_driven_through_cogs_and_never_through_a_shaft() {
        let mixer = Node {
            pos: [0, 0, 0],
            axis: Axis::Y,
            kind: Kind::Mixer,
        };
        let motor = Node {
            pos: [1, -1, 0],
            axis: Axis::Y,
            kind: Kind::Motor {
                facing: [0, 1, 0],
                rpm: 128.0,
                capacity: 16.0,
            },
        };
        let cog = Node {
            pos: [1, 0, 0],
            axis: Axis::Y,
            kind: Kind::Cog { large: false },
        };
        let powered = network(&[motor, cog, mixer]);
        assert_eq!(powered.status, Status::Running);
        assert_eq!(powered.speed(mixer.pos), -128.0);
        assert_eq!(powered.stress, 512.0);
        for direction in [
            [0, -1, 0],
            [0, 1, 0],
            [-1, 0, 0],
            [1, 0, 0],
            [0, 0, -1],
            [0, 0, 1],
        ] {
            assert!(!mixer.shaft_port(direction));
        }
        let below = Node {
            pos: [0, -1, 0],
            ..motor
        };
        let isolated = network(&[mixer, below]);
        assert_eq!(isolated.status, Status::Idle);
        assert_eq!(isolated.speed(mixer.pos), 0.0);
        let large = Node {
            pos: [1, 0, 1],
            kind: Kind::Cog { large: true },
            ..cog
        };
        assert_eq!(ratio(large, mixer), Some(-2.0));
        assert_eq!(ratio(mixer, large), Some(-0.5));
    }
    #[test]
    fn a_one_sided_consumer_draws_stress_only_from_its_connected_back_face() {
        let consumer = Kind::Consumer {
            facing: [-1, 0, 0],
            impact: 2.0,
        };
        let n = network(&[motor([0, 0, 0], 32.0, 16.0), node([1, 0, 0], consumer)]);
        assert_eq!(n.speed([1, 0, 0]), 32.0);
        assert_eq!(n.stress, 64.0);
        let n = network(&[
            motor([0, 0, 0], 32.0, 16.0),
            node(
                [1, 0, 0],
                Kind::Consumer {
                    facing: [1, 0, 0],
                    impact: 2.0,
                },
            ),
        ]);
        assert!(!n.theoretical.contains_key(&[1, 0, 0]));
        assert_eq!(n.stress, 0.0);
    }

    #[test]
    fn incompatible_drives_stop_the_connected_component() {
        let mut second = motor([2, 0, 0], 32.0, 16.0);
        second.kind = Kind::Motor {
            facing: [-1, 0, 0],
            rpm: -32.0,
            capacity: 16.0,
        };
        let n = network(&[
            motor([0, 0, 0], 16.0, 16.0),
            node([1, 0, 0], Kind::Shaft),
            second,
        ]);
        assert_eq!(n.status, Status::ConflictingSources);
        assert_eq!(n.speed([1, 0, 0]), 0.0);
    }

    #[test]
    fn faster_compatible_motor_overdrives_the_slower_without_conflict() {
        let mut other = motor([2, 0, 0], 32.0, 16.0);
        other.kind = Kind::Motor {
            facing: [-1, 0, 0],
            rpm: 32.0,
            capacity: 16.0,
        };
        let n = network(&[
            motor([0, 0, 0], 16.0, 16.0),
            node([1, 0, 0], Kind::Shaft),
            other,
        ]);
        assert_eq!(n.status, Status::Running);
        assert_eq!(n.speed([1, 0, 0]), 32.0);
        assert_eq!(n.capacity, 768.0);
    }

    #[test]
    fn traversal_is_bounded_for_long_loaded_networks() {
        let n = solve(motor([0, 0, 0], 16.0, 16.0), |p| {
            (p[0] >= 1 && p[1] == 0 && p[2] == 0).then(|| node(p, Kind::Shaft))
        });
        assert_eq!(n.status, Status::TooLarge);
        assert_eq!(n.theoretical.len(), MAX_NODES);
    }
}

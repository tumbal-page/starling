// Copyright (c) The Create Team / The Creators of Create
// SPDX-License-Identifier: MIT
//! Placement rules of the supported Create blocks. World queries stay in the adapter.
use crate::kinetics::Axis;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];
    pub fn axis(self) -> Axis {
        match self {
            Self::Down | Self::Up => Axis::Y,
            Self::North | Self::South => Axis::Z,
            _ => Axis::X,
        }
    }
    pub fn clockwise(self, axis: Axis) -> Self {
        let [x, y, z] = self.vector();
        Self::from_vector(match axis {
            Axis::X => [x, z, -y],
            Axis::Y => [-z, y, x],
            Axis::Z => [y, -x, z],
        })
        .unwrap()
    }
    pub fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
    pub fn vector(self) -> [i32; 3] {
        match self {
            Self::Down => [0, -1, 0],
            Self::Up => [0, 1, 0],
            Self::North => [0, 0, -1],
            Self::South => [0, 0, 1],
            Self::West => [-1, 0, 0],
            Self::East => [1, 0, 0],
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }
    pub fn from_vector(vector: [i32; 3]) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.vector() == vector)
    }
}

/// Ports on neighboring blocks point toward the block being placed.
pub fn preferred(ports: [bool; 6]) -> Option<Direction> {
    let mut preferred: Option<Direction> = None;
    for (direction, port) in Direction::ALL.into_iter().zip(ports) {
        if !port {
            continue;
        }
        if preferred.is_some_and(|p| p.axis() != direction.axis()) {
            return None;
        }
        preferred = Some(direction);
    }
    preferred
}

pub fn shaft(look: Direction, clicked: Direction, sneaking: bool, ports: [bool; 6]) -> Axis {
    match preferred(ports) {
        Some(_) if sneaking => clicked.axis(),
        Some(direction) => direction.axis(),
        None => look.axis(),
    }
}

pub fn cog(
    clicked: Direction,
    sneaking: bool,
    ports: [bool; 6],
    against_small: Option<Axis>,
) -> Axis {
    if sneaking {
        clicked.axis()
    } else {
        against_small
            .or_else(|| preferred(ports).map(Direction::axis))
            .unwrap_or(clicked.axis())
    }
}

pub fn motor(look: Direction, sneaking: bool, ports: [bool; 6]) -> Direction {
    if !sneaking && let Some(direction) = preferred(ports) {
        return direction;
    }
    if sneaking { look } else { look.opposite() }
}

/// HandCrankBlock.getStateForPlacement: mount against the clicked support;
/// prefer the neighbor shaft's direction unless secondary use overrides it.
pub fn crank(face: Direction, shift: bool, ports: [bool; 6]) -> Direction {
    if !shift && let Some(preferred) = preferred(ports) {
        return preferred.opposite();
    }
    face
}

pub fn vertical_gearbox(horizontal: Direction, ports: [bool; 6]) -> Axis {
    let mut horizontal_ports = ports;
    horizontal_ports[0] = false;
    horizontal_ports[1] = false;
    let preferred = preferred(horizontal_ports)
        .map(Direction::axis)
        .unwrap_or(horizontal.axis());
    if preferred == Axis::X {
        Axis::Z
    } else {
        Axis::X
    }
}

pub fn gauge_axis(facing: Direction, first: bool) -> Axis {
    match facing.axis() {
        Axis::X => {
            if first {
                Axis::Y
            } else {
                Axis::Z
            }
        }
        Axis::Y => {
            if first {
                Axis::X
            } else {
                Axis::Z
            }
        }
        Axis::Z => {
            if first {
                Axis::X
            } else {
                Axis::Y
            }
        }
    }
}

pub fn gauge(
    face: Direction,
    look: Direction,
    horizontal: Direction,
    ports: [bool; 6],
    attached: bool,
) -> (Direction, bool) {
    if attached {
        let positive = look.vector().iter().any(|v| *v > 0);
        return match face.axis() {
            Axis::X => (
                if positive {
                    Direction::North
                } else {
                    Direction::South
                },
                true,
            ),
            Axis::Y => (horizontal.opposite(), horizontal.axis() == Axis::X),
            Axis::Z => (
                if positive {
                    Direction::West
                } else {
                    Direction::East
                },
                false,
            ),
        };
    }
    let first = match face.axis() {
        Axis::X | Axis::Z => {
            if ports[0] || ports[1] {
                face.axis() == Axis::X
            } else {
                face.axis() == Axis::Z
            }
        }
        Axis::Y => {
            let mut horizontal_ports = ports;
            horizontal_ports[0] = false;
            horizontal_ports[1] = false;
            preferred(horizontal_ports)
                .map_or(horizontal.axis() != Axis::X, |d| d.axis() == Axis::X)
        }
    };
    (face, first)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shaft_prefers_ports_and_shift_overrides_only_a_preferred_axis() {
        let mut ports = [false; 6];
        ports[5] = true;
        assert_eq!(shaft(Direction::Down, Direction::Up, false, ports), Axis::X);
        assert_eq!(shaft(Direction::North, Direction::Up, true, ports), Axis::Y);
        ports[2] = true;
        assert_eq!(shaft(Direction::North, Direction::Up, true, ports), Axis::Z);
    }
    #[test]
    fn cog_inherits_small_cog_axis_unless_shift_is_held() {
        assert_eq!(
            cog(Direction::Up, false, [false; 6], Some(Axis::X)),
            Axis::X
        );
        assert_eq!(cog(Direction::Up, true, [false; 6], Some(Axis::X)), Axis::Y);
    }
    #[test]
    fn attached_gauges_always_connect_to_the_clicked_shaft_axis() {
        for face in Direction::ALL {
            for look in Direction::ALL {
                for horizontal in [
                    Direction::North,
                    Direction::South,
                    Direction::East,
                    Direction::West,
                ] {
                    let (facing, first) = gauge(face, look, horizontal, [false; 6], true);
                    assert_eq!(gauge_axis(facing, first), face.axis());
                }
            }
        }
        let mut ports = [false; 6];
        ports[5] = true;
        assert_eq!(vertical_gearbox(Direction::East, ports), Axis::Z);
        ports[2] = true;
        assert_eq!(vertical_gearbox(Direction::South, ports), Axis::X);
    }
}

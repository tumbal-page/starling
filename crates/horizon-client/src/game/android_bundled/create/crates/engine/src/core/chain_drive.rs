// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! ChainDriveBlock state, placement and connections; ChainDriveGenerator poses.
//! World access, kinetic block entities and drawing stay in the native adapter.

use super::{kinetics::Axis, placement::Direction};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
    Start,
    Middle,
    End,
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub axis: Axis,
    /// The original serialized property is `axis_along_first`.
    pub first: bool,
    pub part: Part,
}

impl Default for State {
    fn default() -> Self {
        Self::new(Axis::Y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub suffix: &'static str,
    pub x_rotation: i32,
    pub y_rotation: i32,
}

impl State {
    pub const fn new(axis: Axis) -> Self {
        Self {
            axis,
            first: true,
            part: Part::None,
        }
    }

    pub fn connection_axis(self) -> Axis {
        match (self.axis, self.first) {
            (Axis::X, true) | (Axis::Z, false) => Axis::Y,
            (_, true) => Axis::X,
            (_, false) => Axis::Z,
        }
    }

    /// ChainDriveBlock.updateShape. A non-chain neighbor is `None`.
    pub fn update(mut self, face: Direction, neighbor: Option<Self>) -> Self {
        let connection_axis = self.connection_axis();
        let face_axis = face.axis();
        let along_first = match self.axis {
            Axis::X => face_axis == Axis::Y,
            _ => face_axis == Axis::X,
        };
        let positive = face.vector()[face_axis.index()] > 0;
        if self.axis == face_axis {
            return self;
        }
        let Some(other) = neighbor else {
            if along_first != self.first || self.part == Part::None {
                return self;
            }
            if self.part == Part::Middle {
                self.part = if positive { Part::End } else { Part::Start };
            } else if (self.part == Part::Start) == positive {
                self.part = Part::None;
            }
            return self;
        };
        if other.axis == face_axis
            || other.part != Part::None && other.connection_axis() != face_axis
        {
            return self;
        }
        if self.part == Part::None {
            self.part = if positive { Part::Start } else { Part::End };
            self.first = along_first;
        } else if connection_axis != face_axis {
            return self;
        }
        if (self.part == Part::Start) != positive {
            self.part = Part::Middle;
        }
        self
    }

    /// Directed ChainDriveBlock.areBlocksConnected; the other part is not tested.
    pub fn connected(self, other: Self, direction: Direction) -> bool {
        let axis = self.connection_axis();
        if other.connection_axis() != axis || direction.axis() != axis {
            return false;
        }
        let positive = direction.vector()[axis.index()] > 0;
        if positive {
            matches!(self.part, Part::Start | Part::Middle)
        } else {
            matches!(self.part, Part::End | Part::Middle)
        }
    }

    /// getRotatedBlockState retains part/first; updateAfterWrenched resets them.
    pub fn rotated_wrench(mut self, face: Direction) -> Self {
        let rotation_axis = if self.part == Part::None {
            face.axis()
        } else {
            self.connection_axis()
        };
        let normal = match self.axis {
            Axis::X => Direction::East,
            Axis::Y => Direction::Up,
            Axis::Z => Direction::South,
        };
        self.axis = normal.clockwise(rotation_axis).axis();
        self
    }

    pub fn wrenched(self, face: Direction, neighbors: [Option<Self>; 6]) -> Self {
        Self::new(self.rotated_wrench(face).axis).rebuild(neighbors)
    }

    fn rebuild(mut self, neighbors: [Option<Self>; 6]) -> Self {
        for (face, neighbor) in Direction::ALL.into_iter().zip(neighbors) {
            if face.axis() != self.axis {
                self = self.update(face, neighbor);
            }
        }
        self
    }

    pub fn model(self) -> Model {
        let x_rotation = if self.part == Part::None {
            if self.axis == Axis::Y { 90 } else { 0 }
        } else if self.axis == Axis::X {
            (if self.first { 90 } else { 0 }) + if self.part == Part::Start { 180 } else { 0 }
        } else if self.axis == Axis::Z {
            if self.first {
                0
            } else if self.part == Part::Start {
                270
            } else {
                90
            }
        } else {
            0
        };
        let y_rotation = if self.part == Part::None {
            if self.axis == Axis::X { 90 } else { 0 }
        } else if self.axis == Axis::Z {
            if self.first && self.part == Part::End {
                270
            } else {
                90
            }
        } else if self.axis == Axis::Y {
            let flip =
                self.part == Part::End && !self.first || self.part == Part::Start && self.first;
            (if self.first { 90 } else { 0 }) + if flip { 180 } else { 0 }
        } else {
            0
        };
        let suffix = if self.part == Part::None {
            "single"
        } else {
            match (self.part == Part::Middle, self.axis == Axis::Y) {
                (true, true) => "middle_vertical",
                (true, false) => "middle_horizontal",
                (false, true) => "end_vertical",
                (false, false) => "end_horizontal",
            }
        };
        Model {
            suffix,
            x_rotation,
            y_rotation,
        }
    }
}

/// Shift uses the look axis even when a neighboring shaft offers a preferred axis.
pub fn placement(
    look: Direction,
    sneaking: bool,
    shaft_ports: [bool; 6],
    neighbors: [Option<State>; 6],
) -> State {
    let axis = if sneaking {
        look.axis()
    } else {
        super::placement::preferred(shaft_ports).map_or(look.axis(), Direction::axis)
    };
    State::new(axis).rebuild(neighbors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_removal_and_reinsertion_keep_source_parts() {
        let single = State::new(Axis::X);
        let start = single.update(Direction::South, Some(single));
        let end = single.update(Direction::North, Some(start));
        let middle = end.update(Direction::South, Some(single));
        assert_eq!(
            (start.part, middle.part, end.part),
            (Part::Start, Part::Middle, Part::End)
        );
        assert_eq!(middle.update(Direction::South, None).part, Part::End);
        assert_eq!(middle.update(Direction::North, None).part, Part::Start);
        assert_eq!(start.update(Direction::South, None).part, Part::None);
        assert_eq!(end.update(Direction::North, None).part, Part::None);
        assert_eq!(
            middle
                .update(Direction::South, None)
                .update(Direction::South, Some(single)),
            middle
        );
    }

    #[test]
    fn different_shaft_axes_can_share_a_chain_axis_without_requiring_reciprocal_parts() {
        let from = State {
            axis: Axis::X,
            first: false,
            part: Part::Start,
        };
        let to = State {
            first: false,
            ..State::new(Axis::Y)
        };
        assert!(from.connected(to, Direction::South));
        assert!(!to.connected(from, Direction::North));
        assert!(!from.connected(to, Direction::East));
        assert_eq!(from.update(Direction::East, Some(to)), from);
    }

    #[test]
    fn shift_placement_uses_look_and_connected_wrench_uses_chain_axis_then_rebuilds() {
        let mut ports = [false; 6];
        ports[Direction::East as usize] = true;
        assert_eq!(
            placement(Direction::Up, true, ports, [None; 6]).axis,
            Axis::Y
        );
        assert_eq!(
            placement(Direction::Up, false, ports, [None; 6]).axis,
            Axis::X
        );
        let old = State {
            axis: Axis::X,
            first: false,
            part: Part::Start,
        };
        for face in Direction::ALL {
            assert_eq!(old.rotated_wrench(face).axis, Axis::Y);
            assert_eq!(old.rotated_wrench(face).part, Part::Start);
            assert_eq!(old.wrenched(face, [None; 6]), State::new(Axis::Y));
        }
    }

    #[test]
    fn neighbor_order_chooses_one_chain_axis_and_an_axial_neighbor_does_not_disconnect_it() {
        let single = State::new(Axis::X);
        let first = single
            .update(Direction::Down, Some(single))
            .update(Direction::South, Some(single));
        let reversed = single
            .update(Direction::South, Some(single))
            .update(Direction::Down, Some(single));
        assert_eq!(first.connection_axis(), Axis::Y);
        assert_eq!(reversed.connection_axis(), Axis::Z);
        let mut neighbors = [None; 6];
        neighbors[Direction::Down as usize] = Some(single);
        neighbors[Direction::South as usize] = Some(single);
        assert_eq!(
            placement(Direction::East, false, [false; 6], neighbors),
            first
        );
        assert_eq!(
            reversed.update(Direction::South, Some(State::new(Axis::Z))),
            reversed
        );
        assert_eq!(reversed.update(Direction::South, None).part, Part::None);
    }

    #[test]
    fn wrench_rebuilds_even_when_the_shaft_axis_does_not_change() {
        let retained = State {
            first: false,
            ..State::new(Axis::X)
        };
        assert_eq!(retained.rotated_wrench(Direction::East), retained);
        assert_eq!(
            retained.wrenched(Direction::East, [None; 6]),
            State::new(Axis::X)
        );
    }
}

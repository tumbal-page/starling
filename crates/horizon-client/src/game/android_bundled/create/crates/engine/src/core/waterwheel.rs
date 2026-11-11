// Copyright (c) The Create Team / The Creators of Create
// SPDX-License-Identifier: MIT
// WaterWheelBlockEntity's plane projection, perimeter sampling and flow score.
use super::kinetics::{Axis, Pos};
pub fn offsets(axis: Axis, large: bool) -> Vec<Pos> {
    let r: i32 = if large { 2 } else { 1 };
    let mut values = Vec::new();
    for x in -r..=r {
        for y in -r..=r {
            for z in -r..=r {
                let p = [x, y, z];
                let distance = x.abs() + y.abs() + z.abs();
                if p[axis.index()] == 0
                    && if large {
                        p.iter().map(|v| v.abs()).max() == Some(2) && distance <= 3
                    } else {
                        distance == 1
                    }
                {
                    values.push(p);
                }
            }
        }
    }
    values
}
pub fn flow_score(axis: Axis, large: bool, mut sample: impl FnMut(Pos) -> [f64; 3]) -> i32 {
    let mut score = 0;
    for p in offsets(axis, large) {
        let mut flow = sample(p);
        flow[axis.index()] = 0.0;
        let length = flow.iter().map(|v| v * v).sum::<f64>().sqrt();
        if length == 0.0 {
            continue;
        }
        let length_p = p.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt();
        let n = p.map(|v| f64::from(v) / length_p);
        let motion = match axis {
            Axis::X => [0.0, -n[2], n[1]],
            Axis::Y => [n[2], 0.0, -n[0]],
            Axis::Z => [-n[1], n[0], 0.0],
        };
        let dot = (0..3).map(|i| flow[i] / length * motion[i]).sum::<f64>();
        if dot.abs() > 0.5 {
            score += if dot > 0.0 { 1 } else { -1 };
        }
    }
    score
}
pub fn rpm(score: i32, large: bool) -> f32 {
    score.clamp(-1, 1) as f32 * if large { 4.0 } else { 8.0 }
}
pub fn structure(axis: Axis) -> Vec<Pos> {
    let mut values = Vec::new();
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let p = [x, y, z];
                if p != [0; 3] && p[axis.index()] == 0 {
                    values.push(p);
                }
            }
        }
    }
    values
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn currents_cancel_and_never_accumulate_extra_rpm() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            assert_eq!(offsets(axis, false).len(), 4);
            assert_eq!(offsets(axis, true).len(), 12);
            assert_eq!(structure(axis).len(), 8);
            assert_eq!(flow_score(axis, false, |_| [0.0, -1.0, 0.0]), 0);
        }
        assert_eq!(
            flow_score(Axis::X, false, |p| if p[2] > 0 {
                [0.0, -1.0, 0.0]
            } else {
                [0.0; 3]
            }),
            1
        );
        assert_eq!(rpm(12, true), 4.0);
        assert_eq!(rpm(-4, false), -8.0);
    }
}

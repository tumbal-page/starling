// Copyright (c) The Create Team / The Creators of Create
// SPDX-License-Identifier: MIT
// KineticScrollValueBehaviour's two direction rows, minimum 1, maximum 256.
pub fn setting(row: u8, magnitude: i32) -> i32 {
    let value = magnitude.clamp(1, 256);
    if row == 0 { -value } else { value }
}
pub fn scroll(value: i32, steps: i32) -> i32 {
    let mut next = value.saturating_add(steps).clamp(-256, 256);
    if next == 0 {
        next = if steps < 0 { -1 } else { 1 };
    }
    next
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn motor_board_never_selects_zero_and_limits_both_directions() {
        for n in -1024..=1024 {
            assert!((1..=256).contains(&setting(1, n)));
            assert!((-256..=-1).contains(&setting(0, n)));
        }
        assert_eq!(setting(0, 64), -64);
        assert_eq!(setting(1, 64), 64);
    }
    #[test]
    fn wheel_crosses_zero_without_stalling_and_saturates() {
        assert_eq!(scroll(1, -1), -1);
        assert_eq!(scroll(-1, 1), 1);
        assert_eq!(scroll(256, 10), 256);
        assert_eq!(scroll(-256, -10), -256);
    }
}

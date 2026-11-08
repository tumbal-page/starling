// AppleSkin mod — settings page for native_settings.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.

pub fn run(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    use crate::game::official_appleskin as m;
    if mod_id != m::ID {
        return;
    }
    buttons.extend(game.appleskin_settings());
}

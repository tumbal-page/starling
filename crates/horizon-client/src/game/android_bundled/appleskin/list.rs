// AppleSkin mod — mod list entry for native_mods.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.

pub fn run(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    use crate::game::official_appleskin as m;
    mods.push(crate::game::native_hooks::NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: Vec::new(),
    });
}

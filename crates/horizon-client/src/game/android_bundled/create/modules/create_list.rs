// Create mod — mod list entry for native_mods.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.

use crate::game::native_hooks::NativeMod;

pub(crate) fn run(mods: &mut Vec<NativeMod>) {
    use super::create_port as m;
    mods.push(NativeMod {
        id: m::ID.into(),
        title: format!("Create {}", m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: Vec::new(),
    });
}

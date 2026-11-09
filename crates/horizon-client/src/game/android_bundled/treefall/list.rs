// native_hooks::Game::native_mods (a hook point): TreeFall's row in Options > Mods.
pub(super) fn treefall_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    use crate::game::treefall as tf;
    mods.push(crate::game::native_hooks::NativeMod {
        id: tf::ID.into(),
        title: format!("TreeFall {}", tf::VERSION),
        description: tf::DESCRIPTION.into(),
        keys: Vec::new(),
    });
}

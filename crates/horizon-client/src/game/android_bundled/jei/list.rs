// The mod's row in Options > Mods, with its two keys.
pub(super) fn jei_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    use crate::game::jei as m;
    mods.push(crate::game::native_hooks::NativeMod {
        id: m::ID.into(),
        title: m::title(),
        description: m::DESCRIPTION.into(),
        keys: m::KEYS.iter().map(|k| k.map(String::from)).collect(),
    });
}

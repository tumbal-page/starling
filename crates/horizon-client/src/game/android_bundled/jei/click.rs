// A click on the settings page: 0 turns "Click gives items" on or off, 1
// switches the language.
pub(super) fn jei_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    use crate::game::jei as m;
    if mod_id != m::ID {
        return;
    }
    match button {
        0 => {
            let on = m::give(game.native_setting(m::ID, "give").as_deref());
            game.native_set_setting(m::ID, "give", if on { "false" } else { "true" });
        }
        1 => {
            let lang = m::lang(game.native_setting(m::ID, "language").as_deref());
            game.native_set_setting(m::ID, "language", lang.next().code());
        }
        _ => {}
    }
}

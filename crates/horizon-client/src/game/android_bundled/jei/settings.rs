// The mod's settings page: "Click gives items" and the language.
pub(super) fn jei_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    use crate::game::jei as m;
    if mod_id != m::ID {
        return;
    }
    let lang = m::lang(game.native_setting(m::ID, "language").as_deref());
    let on = m::give(game.native_setting(m::ID, "give").as_deref());
    buttons.push(lang.give_button(on));
    buttons.push(lang.language_button());
}

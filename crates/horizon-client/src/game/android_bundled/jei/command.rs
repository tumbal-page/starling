// /jei says what this build is; /jei give turns "Click gives items" on or off.
pub(super) fn jei_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    use crate::game::jei as m;
    let Some(args) = command.strip_prefix("jei") else {
        return;
    };
    if !(args.is_empty() || args.starts_with(' ')) {
        return;
    }
    let Some(cmd) = m::command(args) else {
        *ran = true;
        game.native_chat("Unknown option: use /jei or /jei give");
        return;
    };
    *ran = true;
    match cmd {
        m::Command::About => {
            let text = match crate::game::jei_glue::counts() {
                Some((items, recipes)) => m::about_counts(items, recipes),
                None => m::about(),
            };
            game.native_chat(&text);
        }
        m::Command::ToggleGive => {
            let on = m::give(game.native_setting(m::ID, "give").as_deref());
            game.native_set_setting(m::ID, "give", if on { "false" } else { "true" });
            game.native_chat(&m::give_button(!on));
        }
    }
}

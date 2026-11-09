// native_hooks::Game::native_tick (a hook point): the window notes whether
// TreeFall is turned off in Options > Mods, for single player's server thread.
pub(super) fn treefall_tick(game: &mut crate::game::Game<'_>) {
    game.treefall_tick();
}

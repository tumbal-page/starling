// Once a frame: build the world's tables once and show the item list.
pub(super) fn jei_frame(game: &mut crate::game::Game<'_>) {
    crate::game::jei_glue::sync(game);
}

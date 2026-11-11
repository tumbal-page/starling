// Pinned to the type/block/compound validation in remote_block_entity_data.
// This runs before its ordinary NBT merge and hydrates canonical owned slots.
use super::Game;
use crate::containers::BlockEntity;
use horizon_world::nbt::Tag;

pub(super) fn packet(game: &mut Game<'_>, pos: [i32; 3], kind: i32, tag: &Tag) {
    if !matches!(tag, Tag::Compound(_)) {
        return;
    }
    let Some(name) = game
        .remote
        .as_ref()
        .and_then(|r| r.builtin.get("minecraft:block_entity_type")?.name(kind))
    else {
        return;
    };
    let block = game.world.block(pos[0], pos[1], pos[2]);
    let block = &game.res.facts().get(block).name;
    let Some(BlockEntity::Mod(entity)) =
        game.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    horizon_client::save::create_inventory_codec::receive(entity, name, block, tag);
}

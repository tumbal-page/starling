// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Exact-pinned adapter inside game/remote.rs. Only recognized burner actions
// own this turn; packets use the same methods/fields as the vanilla turn.
use super::{Game, Hand, S, face_direction, hand_id, play};
use crate::game::create_port_burner_interactions as burner;

pub(in crate::game) fn block(game: &mut Game<'_>, hand: Hand) -> bool {
    if game.spectator() {
        return false;
    }
    let held = game.with_hand(hand, |g| g.held().cloned());
    let Some(hit) = game.pick() else { return false };
    let lifecycle = burner::reacts(game, hit.pos, held.as_ref());
    let fuel = game.with_hand(hand, |g| {
        super::super::create_port_burner::reacts(g, hit.pos)
    });
    if !lifecycle && !fuel {
        return false;
    }
    let sequence = game.next_sequence();
    let cursor = std::array::from_fn(|i| (hit.point[i] - f64::from(hit.pos[i])) as f32);
    game.send(S::UseItemOn(play::UseItemOn {
        hand: hand_id(hand),
        pos: hit.pos,
        face: i32::from(face_direction(hit.face)),
        cursor,
        inside: hit.face == [0; 3],
        world_border: false,
        sequence,
    }));
    if let Some(remote) = game.remote.as_mut() {
        remote.use_target = Some(hit.pos);
    }
    let predicted = game.with_hand(hand, |g| {
        if lifecycle {
            burner::predict_block(g, hit.pos, held.as_ref())
        } else {
            super::super::create_port_burner::predict_client(g, hit.pos)
        }
    });
    debug_assert!(predicted);
    game.with_hand(hand, Game::swing_arm);
    true
}

pub(in crate::game) fn entity(game: &mut Game<'_>, hand: Hand) -> bool {
    if game.spectator() {
        return false;
    }
    let Some((index, distance)) = game.mob_hit() else {
        return false;
    };
    if !game.before_block(distance)
        || game
            .remote_object_hit(false)
            .is_some_and(|object| object.distance < distance && game.before_block(object.distance))
        || game.remote_mob_takes(index, hand)
        || !burner::entity_reacts(game, index, hand)
    {
        return false;
    }
    if let Some(remote) = game.remote.as_mut() {
        remote.use_target = None;
    }
    let id = game.mobs[index].id as i32;
    game.send(S::Interact(play::Interact {
        id,
        hand: hand_id(hand),
        location: [0.0, 1.0, 0.0],
        secondary: game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft),
    }));
    let predicted = burner::predict_entity(game, index, hand);
    debug_assert!(predicted);
    true // Create's FAIL ends the item turn without an arm swing.
}

impl Game<'_> {
    // client_level_unload also fires during a dimension transition, after the
    // same connection has installed its new level. Server fuel data remains
    // valid there; disconnect/reconfiguration removes heard_level first.
    pub(in crate::game) fn create_burner_sync_has_level(&self) -> bool {
        self.remote
            .as_ref()
            .is_some_and(|remote| remote.heard_level.is_some())
    }

    pub(in crate::game) fn create_burner_predict_block(&mut self, hand: Hand) -> bool {
        block(self, hand)
    }
    pub(in crate::game) fn create_burner_predict_entity(&mut self, hand: Hand) -> bool {
        entity(self, hand)
    }
}

// AppleSkin mod — hunger state reader.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.

use crate::game::Game;

#[derive(Clone, Copy, Debug)]
pub struct NativeHunger {
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub exhaustion_visible: bool,
    pub health: f32,
    pub held: Option<(u32, f32)>,
    pub held_regeneration: Option<(i32, i32)>,
    pub natural_regeneration: bool,
    pub peaceful: bool,
    pub regeneration_blocked: bool,
}

impl Game<'_> {
    pub fn native_hunger(&self) -> Option<NativeHunger> {
        let p = self.player.as_ref()?;
        let food = &p.health.food;
        let held_item = self.held().and_then(|stack| self.res.items().get(&*stack.item));
        let held = held_item.and_then(|info| info.food);
        let held_regeneration = held_item
            .and_then(|info| info.consumable.as_ref())
            .and_then(|consumable| {
                consumable.effects.iter().find_map(|effect| match effect {
                    horizon_client::items::ConsumeEffect::ApplyEffects { effects, .. } => effects
                        .iter()
                        .find(|e| horizon_client::brewing::effect(&e.id).is_some_and(|d| d.id == "regeneration"))
                        .map(|e| (e.duration, e.amplifier)),
                    _ => None,
                })
            });
        let rules = self.world.health_rules;
        let active = &self.potions.effects;
        Some(NativeHunger {
            food: food.food,
            saturation: food.saturation,
            exhaustion: food.exhaustion,
            exhaustion_visible: !p.client_side,
            health: p.health.health(),
            held,
            held_regeneration,
            natural_regeneration: rules.natural_regeneration,
            peaceful: rules.difficulty == horizon_world::food::Difficulty::Peaceful,
            regeneration_blocked: ["poison", "wither", "regeneration"]
                .iter()
                .any(|id| active.get(id).is_some()),
        })
    }
}

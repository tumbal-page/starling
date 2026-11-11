// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// BasinBlockEntity.addToGoggleTooltip. Source item and tank iteration order.
use super::{
    Game,
    create_port_goggles::{indent, numeral, tr},
};
use crate::items::ItemStack;
use horizon_client::text::{self, Style, Text};
use horizon_world::nbt::{Compound, Tag};
// ChatFormatting.BLUE in the original tooltip provider.
const FLUID_AMOUNT_COLOR: u32 = 0x55_55_ff;

fn inventory(nbt: &Compound, key: &str) -> [Option<ItemStack>; 9] {
    let mut slots = std::array::from_fn(|_| None);
    let Some(Tag::List(items)) = nbt.get(&key.into()).and_then(|v| v.get("Items")) else {
        return slots;
    };
    for tag in items {
        let Some(Tag::Int(slot)) = tag.get("Slot") else {
            continue;
        };
        if let Some(target) = usize::try_from(*slot).ok().and_then(|i| slots.get_mut(i)) {
            *target = horizon_items::item_nbt::item_from_nbt(tag)
                .filter(|s| s.count > 0 && s.item.as_ref() != "minecraft:air");
        }
    }
    slots
}
pub(super) fn lines(game: &Game<'_>, nbt: &Compound) -> Vec<Text> {
    let mut lines = vec![indent(game, tr("gui.goggles.basin_contents"), 0)];
    // CombinedInvWrapper(inputInventory, outputInventory).
    for key in ["InputItems", "OutputItems"] {
        for stack in inventory(nbt, key).into_iter().flatten() {
            let name = Text::literal(game.pack.names.display_item(&stack.item));
            lines.push(indent(
                game,
                Text::empty()
                    .append(name.with_style(&Style::color(text::GRAY)))
                    .append(
                        Text::literal(format!(" x{}", stack.count))
                            .with_style(&Style::color(text::GREEN)),
                    ),
                1,
            ));
        }
    }
    // fluidCapability is CombinedTankWrapper(outputTank, inputTank), unlike
    // the visual input-first segmentation. Do not reorder its tooltip rows.
    for key in ["OutputTanks", "InputTanks"] {
        let Ok(tanks) = super::create_port_fluid_codec::read_half(nbt.get(&key.into())) else {
            continue;
        };
        for fluid in tanks.into_iter().flatten() {
            if fluid.amount == 0 {
                continue;
            }
            let name = match fluid.key.id.as_str() {
                "minecraft:water" => "block.minecraft.water",
                "minecraft:lava" => "block.minecraft.lava",
                _ => continue,
            };
            let line = Text::empty().append(
                Text::translate(name, &[])
                    .append(Text::literal(" "))
                    .with_style(&Style::color(text::GRAY))
                    .append(
                        numeral(f64::from(fluid.amount))
                            .append(tr("generic.unit.millibuckets"))
                            .with_style(&Style::color(FLUID_AMOUNT_COLOR)),
                    ),
            );
            lines.push(indent(game, line, 1));
        }
    }
    if lines.len() == 1 {
        lines.clear();
    }
    lines
}

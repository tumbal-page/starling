// Independently authored from pinned MIT Create schemas and recorded public-API
// observations. This module has no gameplay dependency: save and client receipt
// share the same canonical item decoding.
use horizon_client::containers::ModBlockEntity;
use horizon_items::items::ItemStack;
use horizon_world::nbt::Tag;
use std::ops::ControlFlow;

type Inventories = &'static [(&'static str, usize, usize)];
fn schema(id: &str) -> Option<(usize, Inventories)> {
    Some(match id {
        "create:depot" => (9, &[("OutputBuffer", 1, 8)]),
        "create:basin" => (18, &[("InputItems", 0, 9), ("OutputItems", 9, 9)]),
        "create:millstone" => (10, &[("InputInventory", 0, 1), ("OutputInventory", 1, 9)]),
        _ => return None,
    })
}

// None means an unrelated type or malformed root tag. Canonical inventory
// absence within a valid compound means empty slots, including partial packets.
pub fn parse_slots(id: &str, tag: &Tag) -> Option<Vec<Option<ItemStack>>> {
    let (size, inventories) = schema(id)?;
    if !matches!(tag, Tag::Compound(_)) {
        return None;
    }
    // A different explicitly integer-typed size is retained without exposing
    // inventory to ordinary operations, which cannot represent it.
    let mut items = vec![None; size];
    let unsupported = inventories.iter().any(|&(key, _, count)| {
        matches!(tag.get(key).and_then(|b| b.get("Size")),
            Some(Tag::Int(size)) if *size != count as i32)
    });
    if !unsupported {
        if id == "create:depot" {
            items[0] = tag
                .get("HeldItem")
                .and_then(|held| held.get("Item"))
                .and_then(horizon_items::item_nbt::item_from_nbt);
        }

        // The recorded ordinary-buffer cases read missing/non-numeric slots
        // as zero, narrow long slots to 32 bits, and floor floating slots.
        // Range checking happens after conversion, before indexing our cells.
        fn inventory_slot(value: Option<&Tag>) -> i32 {
            match value {
                Some(Tag::Byte(v)) => i32::from(*v),
                Some(Tag::Short(v)) => i32::from(*v),
                Some(Tag::Int(v)) => *v,
                Some(Tag::Long(v)) => *v as i32,
                Some(Tag::Float(v)) => f32::from_bits(*v).floor() as i32,
                Some(Tag::Double(v)) => f64::from_bits(*v).floor() as i32,
                _ => 0,
            }
        }
        for &(key, start, count) in inventories {
            let Some(Tag::List(entries)) = tag.get(key).and_then(|b| b.get("Items")) else {
                continue;
            };
            for entry in entries {
                if let Ok(slot) = usize::try_from(inventory_slot(entry.get("Slot")))
                    && slot < count
                    && let Some(stack) = horizon_items::item_nbt::item_from_nbt(entry)
                {
                    // A later valid entry replaces a duplicate. An invalid
                    // item entry leaves the previous valid stack untouched.
                    items[start + slot] = Some(stack);
                }
            }
        }
    }

    Some(items)
}

pub fn decode(id: &str, tag: &Tag) -> ControlFlow<Option<ModBlockEntity>> {
    if schema(id).is_none() {
        return ControlFlow::Continue(());
    }
    let Tag::Compound(fields) = tag else {
        return ControlFlow::Break(None);
    };
    let Some(items) = parse_slots(id, tag) else {
        return ControlFlow::Break(None);
    };
    // Generic Items is a redundant snapshot and cannot revive canonical empties.
    let mut nbt = fields.clone();
    for key in ["id", "x", "y", "z", "Items"] {
        nbt.remove(&key.into());
    }
    ControlFlow::Break(Some(
        ModBlockEntity {
            id: id.to_owned(),
            nbt,
            items,
            data: Vec::new(),
        },
    ))
}

// Called before Horizon's ordinary merge_update_tag. Only canonical inventory
// fields are reset on absence; unowned NBT and runtime-only data keep the same
// live entity identity. Slot parsing borrows the packet without cloning its NBT.
pub fn receive(entity: &mut ModBlockEntity, packet_type: &str, block: &str, tag: &Tag) -> bool {
    // Horizon remote_block_entity_data treats an empty update tag as a no-op.
    // This transport rule differs from decoding an empty saved inventory.
    if !matches!(tag, Tag::Compound(fields) if !fields.is_empty()) {
        return false;
    }
    if packet_type != block || entity.id != packet_type {
        return false;
    }
    let Some((_, inventories)) = schema(packet_type) else {
        return false;
    };
    let Some(items) = parse_slots(packet_type, tag) else {
        return false;
    };
    if entity.items != items {
        entity.items = items;
    }
    for key in inventories
        .iter()
        .map(|&(key, _, _)| key)
        .chain((packet_type == "create:depot").then_some("HeldItem"))
    {
        if tag.get(key).is_none() && entity.nbt.contains_key(&key.into()) {
            entity.nbt.remove(&key.into());
        }
    }
    true
}

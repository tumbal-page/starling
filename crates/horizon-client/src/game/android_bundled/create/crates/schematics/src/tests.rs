use super::*;

struct Registry;
impl Resolver for Registry {
    fn block_schema(&self, requested: &BlockState) -> Result<BlockSchema, String> {
        let mut state = requested.clone();
        let mut defaults = BTreeMap::new();
        let mut be = None;
        match state.name.as_str() {
            "minecraft:stone" | "minecraft:air" => {}
            "create:shaft" => {
                defaults.insert("axis", "y");
                defaults.insert("waterlogged", "false");
            }
            "create:creative_motor" => {
                defaults.insert("facing", "north");
                let mut schema = BlockEntitySchema::new("create:motor");
                schema
                    .safe_fields
                    .insert("ScrollValue".into(), TagKind::Int);
                schema
                    .discarded_fields
                    .extend(["Speed", "Source", "Network", "Items"].map(str::to_owned));
                be = Some(schema);
            }
            "minecraft:piston" => {
                defaults.insert("extended", "false");
            }
            "minecraft:torch" => {}
            _ => return Err(format!("unsupported block {}", state.name)),
        }
        for (key, value) in &state.properties {
            if !defaults.contains_key(key.as_str()) {
                return Err(format!("unsupported property {key}"));
            }
            let valid = match key.as_str() {
                "axis" => ["x", "y", "z"].contains(&value.as_str()),
                "facing" => {
                    ["north", "south", "east", "west", "up", "down"].contains(&value.as_str())
                }
                _ => ["false", "true"].contains(&value.as_str()),
            };
            if !valid {
                return Err(format!("invalid {key}={value}"));
            }
        }
        for (key, value) in defaults {
            state.properties.entry(key.into()).or_insert(value.into());
        }
        let deferred = state.name == "minecraft:torch";
        Ok(BlockSchema {
            state,
            block_entity: be,
            deferred,
        })
    }
    fn validate_safe_nbt(&self, _state: &BlockState, data: &Compound) -> Result<(), String> {
        if let Some(Tag::Int(v)) = data.get("ScrollValue")
            && !(-256..=256).contains(v)
        {
            return Err("motor ScrollValue out of source range".into());
        }
        Ok(())
    }
}

fn state(name: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        name: name.into(),
        properties: properties
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
    }
}
fn fixture() -> Schematic {
    // Our own original two-block mechanism, not a copied upstream asset.
    Schematic {
        data_version: SOURCE_DATA_VERSION,
        size: [4, 2, 2],
        palettes: vec![vec![
            state("create:creative_motor", &[("facing", "east")]),
            state("create:shaft", &[("axis", "x"), ("waterlogged", "true")]),
            BlockState::new("minecraft:air"),
        ]],
        blocks: vec![
            Block {
                relative_pos: [0, 0, 0],
                state_index: 0,
                block_entity: Some(Compound::from([
                    ("id".into(), Tag::String("create:motor".into())),
                    ("ScrollValue".into(), Tag::Int(128)),
                    ("Speed".into(), Tag::Float(128f32.to_bits())),
                    ("Source".into(), Tag::IntArray(vec![20, 70, 30])),
                    (
                        "Network".into(),
                        Tag::Compound(Compound::from([("Id".into(), Tag::Long(123456789))])),
                    ),
                    (
                        "Items".into(),
                        Tag::list(
                            TagKind::Compound,
                            vec![Tag::Compound(Compound::from([(
                                "id".into(),
                                Tag::String("minecraft:diamond".into()),
                            )]))],
                        ),
                    ),
                    ("x".into(), Tag::Int(17)),
                    ("y".into(), Tag::Int(64)),
                    ("z".into(), Tag::Int(-3)),
                ])),
            },
            Block {
                relative_pos: [1, 0, 0],
                state_index: 1,
                block_entity: None,
            },
            Block {
                relative_pos: [2, 0, 0],
                state_index: 2,
                block_entity: None,
            },
        ],
        entities: vec![],
    }
}
fn raw_be(s: &mut Schematic) -> &mut Compound {
    s.blocks[0].block_entity.as_mut().unwrap()
}
fn plan(s: &Schematic) -> Result<Plan, Vec<Issue>> {
    preflight(s, &Registry, &Options::default())
}

#[test]
fn generated_fixture_roundtrip_preserves_types_and_source_safe_policy() {
    let s = fixture();
    let root = s.to_compound();
    let bytes = nbt::encode_gzip(&root, &Limits::default()).unwrap();
    let decoded = read_gzip(&bytes, &Limits::default()).unwrap();
    assert_eq!(decoded, s);
    let plan = plan(&decoded).unwrap();
    assert_eq!(plan.blocks.len(), 2);
    assert_eq!(plan.skipped_air, 1);
    assert_eq!(plan.blocks[1].state.properties["waterlogged"], "false");
    let be = plan.blocks[0].block_entity.as_ref().unwrap();
    assert_eq!(be.id, "create:motor");
    assert_eq!(
        be.data,
        Compound::from([("ScrollValue".into(), Tag::Int(128))])
    );
    assert_eq!(plan.notes.len(), 5); // waterlogging plus four explicitly discarded fields.
    assert!(!be.data.contains_key("Source"));
    assert!(!be.data.contains_key("Items"));
    assert_eq!(fixture(), s); // Preflight did not edit the source.
}
#[test]
fn raw_nbt_all_types_and_modified_utf8_roundtrip() {
    let root = Compound::from([
        ("byte".into(), Tag::Byte(-128)),
        ("short".into(), Tag::Short(-30000)),
        ("int".into(), Tag::Int(i32::MIN)),
        ("long".into(), Tag::Long(i64::MAX)),
        ("float".into(), Tag::Float(0x7fc00042)),
        ("double".into(), Tag::Double(0x8000000000000000)),
        ("bytes".into(), Tag::ByteArray(vec![-128, 0, 127])),
        ("ints".into(), Tag::IntArray(vec![-1, 2])),
        ("longs".into(), Tag::LongArray(vec![i64::MIN, 7])),
        ("nul\0Ж😀".into(), Tag::String("A\0Ж😀".into())),
        ("emptytyped".into(), Tag::list(TagKind::Compound, vec![])),
        (
            "nested".into(),
            Tag::list(TagKind::List, vec![Tag::list(TagKind::End, vec![])]),
        ),
    ]);
    let bytes = nbt::encode_named(&root, &Limits::default()).unwrap();
    assert_eq!(nbt::decode_named(&bytes, &Limits::default()).unwrap(), root);
    // Independent Java DataOutputStream.writeUTF vector: NUL is C0 80;
    // supplementary characters are a pair of three-byte UTF-16 encodings.
    assert_eq!(
        nbt::encode_modified_utf8("A\0Ж😀"),
        [
            0x41, 0xc0, 0x80, 0xd0, 0x96, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80
        ]
    );
    assert!(
        nbt::decode_modified_utf8(&[0xed, 0xa0, 0x80])
            .unwrap_err()
            .contains("unpaired")
    );
    assert!(nbt::decode_modified_utf8(&[0xf0, 0x9f, 0x98, 0x80]).is_err());
}
#[test]
fn defaults_air_policy_and_deferred_order_are_explicit() {
    let mut s = fixture();
    s.palettes[0][1].properties.clear();
    s.palettes[0].push(BlockState::new("minecraft:torch"));
    s.blocks.push(Block {
        relative_pos: [0, 0, 1],
        state_index: 3,
        block_entity: None,
    });
    let p = preflight(
        &s,
        &Registry,
        &Options {
            include_air: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(p.blocks.len(), 4);
    assert_eq!(p.skipped_air, 0);
    assert_eq!(p.blocks[1].state.properties["axis"], "y");
    assert_eq!(p.blocks.last().unwrap().state.name, "minecraft:torch");
    let mut s = fixture();
    s.palettes[0][1] = state("minecraft:piston", &[("extended", "true")]);
    assert_eq!(
        plan(&s).unwrap().blocks[1].state.properties["extended"],
        "false"
    );
}
#[test]
fn unknowns_are_aggregated_and_unused_palette_entries_are_validated() {
    let mut s = fixture();
    s.palettes[0].push(BlockState::new("addon:missing"));
    s.palettes[0][1]
        .properties
        .insert("axis".into(), "diagonal".into());
    raw_be(&mut s).insert("UnknownConfig".into(), Tag::Int(7));
    let issues = plan(&s).unwrap_err();
    assert_eq!(issues.len(), 3);
    assert!(issues.iter().any(|i| i.message.contains("addon:missing")));
    assert!(issues.iter().any(|i| i.path.ends_with("UnknownConfig")));
    assert!(issues.iter().any(|i| i.message.contains("diagonal")));
}
#[test]
fn invalid_properties_are_not_defaulted_or_normalized_into_acceptance() {
    for (key, value) in [("facing", "east"), ("waterlogged", "maybe")] {
        let mut s = fixture();
        s.palettes[0][1].properties.insert(key.into(), value.into());
        assert!(plan(&s).is_err());
    }
    struct Liar;
    impl Resolver for Liar {
        fn block_schema(&self, _: &BlockState) -> Result<BlockSchema, String> {
            Registry.block_schema(&BlockState::new("minecraft:air"))
        }
    }
    assert!(
        preflight(&fixture(), &Liar, &Options::default())
            .unwrap_err()
            .iter()
            .any(|i| i.message.contains("instead of validating"))
    );
}
#[test]
fn wrong_block_entity_id_types_unknown_fields_and_semantics_reject_whole_plan() {
    for (key, value) in [
        ("id", Tag::String("create:drill".into())),
        ("ScrollValue", Tag::Long(128)),
        ("ScrollValue", Tag::Int(999)),
        ("Unknown", Tag::Byte(1)),
        ("x", Tag::String("17".into())),
    ] {
        let mut s = fixture();
        raw_be(&mut s).insert(key.into(), value);
        assert!(plan(&s).is_err(), "{key}");
    }
    let mut s = fixture();
    s.blocks[1].block_entity = Some(Compound::new());
    assert!(
        plan(&s)
            .unwrap_err()
            .iter()
            .any(|i| i.message.contains("does not support"))
    );
}
#[test]
fn version_entities_transforms_and_random_palettes_fail_explicitly() {
    for version in [0, 3465, 3954, 3956] {
        let mut s = fixture();
        s.data_version = version;
        assert!(plan(&s).unwrap_err()[0].message.contains("no migration"));
    }
    let s = fixture();
    assert!(
        preflight(
            &s,
            &Registry,
            &Options {
                quarter_turns: 1,
                ..Options::default()
            }
        )
        .is_err()
    );
    assert!(
        preflight(
            &s,
            &Registry,
            &Options {
                mirror: true,
                ..Options::default()
            }
        )
        .is_err()
    );
    let mut s = fixture();
    s.palettes.push(s.palettes[0].clone());
    assert!(plan(&s).is_err());
    let mut s = fixture();
    s.entities.push(Compound::from([(
        "nbt".into(),
        Tag::Compound(Compound::from([(
            "id".into(),
            Tag::String("create:super_glue".into()),
        )])),
    )]));
    assert!(
        plan(&s)
            .unwrap_err()
            .iter()
            .any(|i| i.message.contains("create:super_glue"))
    );
}
#[test]
fn bad_structure_shape_indexes_positions_and_aliases_are_rejected() {
    let limits = Limits::default();
    for size in [[0, 2, 2], [-1, 2, 2], [129, 2, 2], [128, 128, 128]] {
        let mut s = fixture();
        s.size = size;
        assert!(Schematic::from_compound(&s.to_compound(), &limits).is_err());
        assert!(plan(&s).is_err());
    }
    let mut s = fixture();
    s.blocks[0].state_index = 99;
    assert!(Schematic::from_compound(&s.to_compound(), &limits).is_err());
    assert!(plan(&s).is_err());
    let mut s = fixture();
    s.blocks[0].relative_pos = [-1, 0, 0];
    assert!(Schematic::from_compound(&s.to_compound(), &limits).is_err());
    assert!(plan(&s).is_err());
    let mut s = fixture();
    s.blocks[1].relative_pos = s.blocks[0].relative_pos;
    assert!(Schematic::from_compound(&s.to_compound(), &limits).is_err());
    assert!(plan(&s).is_err());
    let mut root = fixture().to_compound();
    root.insert("surprise".into(), Tag::Int(1));
    assert!(Schematic::from_compound(&root, &limits).is_err());
    let mut root = fixture().to_compound();
    root.remove("DataVersion");
    assert!(Schematic::from_compound(&root, &limits).is_err());
    let mut root = fixture().to_compound();
    if let Tag::List { values, .. } = root.get_mut("palette").unwrap()
        && let Tag::Compound(c) = &mut values[0]
    {
        c.insert("id".into(), Tag::String("minecraft:air".into()));
    }
    assert!(Schematic::from_compound(&root, &limits).is_err());
}
#[test]
fn every_truncated_prefix_bad_crc_members_and_trailing_nbt_fail() {
    let limits = Limits::default();
    let root = fixture().to_compound();
    let bytes = nbt::encode_gzip(&root, &limits).unwrap();
    for n in 0..bytes.len() {
        assert!(
            nbt::decode_gzip(&bytes[..n], &limits).is_err(),
            "prefix {n}"
        );
    }
    let mut bad = bytes.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(nbt::decode_gzip(&bad, &limits).is_err());
    let mut bad = bytes.clone();
    bad.extend(&bytes);
    assert!(nbt::decode_gzip(&bad, &limits).is_err());
    let mut bad = bytes;
    bad.push(0);
    assert!(nbt::decode_gzip(&bad, &limits).is_err());
    let mut raw = nbt::encode_named(&root, &limits).unwrap();
    raw.push(0);
    assert!(nbt::decode_named(&raw, &limits).is_err());
    assert!(
        nbt::decode_named(
            &[
                10, 0, 0, 3, 0, 1, b'x', 0, 0, 0, 1, 3, 0, 1, b'x', 0, 0, 0, 2, 0
            ],
            &limits
        )
        .unwrap_err()
        .message
        .contains("duplicate")
    );
    assert!(nbt::decode_named(&[10, 0, 0, 9, 0, 0, 0, 0, 0, 0, 1, 0], &limits).is_err()); // end-tag list of length1.
}
#[test]
fn bounded_decompression_depth_nodes_and_collection_lengths_fail_before_large_allocation() {
    let root = fixture().to_compound();
    let limits = Limits::default();
    let gzip = nbt::encode_gzip(&root, &limits).unwrap();
    for small in [
        Limits {
            max_compressed_bytes: 10,
            ..limits.clone()
        },
        Limits {
            max_uncompressed_bytes: 32,
            ..limits.clone()
        },
        Limits {
            max_depth: 2,
            ..limits.clone()
        },
        Limits {
            max_nodes: 10,
            ..limits.clone()
        },
        Limits {
            max_collection_len: 2,
            ..limits.clone()
        },
        Limits {
            max_string_bytes: 2,
            ..limits.clone()
        },
    ] {
        assert!(nbt::decode_gzip(&gzip, &small).is_err());
    }
    let huge = [10, 0, 0, 7, 0, 0, 0x7f, 0xff, 0xff, 0xff, 0];
    assert!(
        nbt::decode_named(&huge, &limits)
            .unwrap_err()
            .message
            .contains("length exceeds")
    );
    let negative = [10, 0, 0, 7, 0, 0, 0xff, 0xff, 0xff, 0xff, 0];
    assert!(
        nbt::decode_named(&negative, &limits)
            .unwrap_err()
            .message
            .contains("negative")
    );
    let mut nested = Tag::Int(1);
    for _ in 0..70 {
        nested = Tag::Compound(Compound::from([("n".into(), nested)]));
    }
    assert!(nbt::encode_gzip(&Compound::from([("n".into(), nested)]), &limits).is_err());
}

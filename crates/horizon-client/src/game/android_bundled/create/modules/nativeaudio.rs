// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Client-only source SoundScapes and AirCurrentSound through SoundEngine.
use super::Game;
use super::create_engine::audio::{self as rules, FanSound, Group, Key, Sound, SoundScapes};
use horizon_client_render::sound_engine::{
    Attenuation, InstanceId, SoundEngine, SoundInstance, Sounds, Ticking,
};
use horizon_core::random::RandomSource;
use horizon_world::nbt::{Compound, Tag};
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct Client {
    game: usize,
    world: usize,
    dimension: String,
    ticks: i32,
    time: i64,
    scapes: SoundScapes,
    continuous: BTreeMap<(Key, usize), InstanceId>,
    fan: FanSound,
    fan_handle: Option<InstanceId>,
    cranks: BTreeMap<[i32; 3], Crank>,
}
struct Crank {
    in_use: i32,
    time: Option<i64>,
}
thread_local! {static CLIENT:RefCell<Client>=RefCell::new(Client::default());}
struct Prediction {
    game: usize,
    world: usize,
    dimension: String,
    pos: [i32; 3],
}
thread_local! {static PREDICTIONS:RefCell<Vec<Prediction>>=const{RefCell::new(Vec::new())};}
pub(super) fn unload(game: &mut Game<'_>) {
    CLIENT.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.game == std::ptr::from_ref(game) as usize {
            stop(game, &mut cache);
        }
        *cache = Client::default();
    });
    PREDICTIONS.with(|p| p.borrow_mut().clear());
}
fn client(game: &Game<'_>) -> bool {
    game.native_window() && game.remote.is_some() && game.native_on("create")
}
fn camera(game: &Game<'_>) -> Option<[f64; 3]> {
    game.camera_mob()
        .map(|m| [m.position.x, m.position.y, m.position.z])
        .or_else(|| {
            game.player
                .as_ref()
                .map(|p| [p.position.x, p.position.y, p.position.z])
        })
}
fn stop(game: &mut Game<'_>, cache: &mut Client) {
    for (_, id) in std::mem::take(&mut cache.continuous) {
        game.audio.stop_and_release(id);
    }
    if let Some(id) = cache.fan_handle.take() {
        game.audio.stop_and_release(id);
    }
}
fn prepare(game: &mut Game<'_>, cache: &mut Client) {
    let id = std::ptr::from_ref(game) as usize;
    let world = std::ptr::from_ref(&game.world) as usize;
    if cache.game == id
        && (cache.world != world
            || cache.dimension != game.dim.id()
            || cache.time > game.world.game_time)
    {
        stop(game, cache);
    }
    if cache.game != id
        || cache.world != world
        || cache.dimension != game.dim.id()
        || cache.time > game.world.game_time
    {
        // A different Game owns a different Audio: never use old numeric
        // handles to stop unrelated sounds in that new engine.
        *cache = Client {
            game: id,
            world,
            dimension: game.dim.id().into(),
            time: game.world.game_time,
            ..Client::default()
        };
    }
    cache.time = game.world.game_time;
}
fn trace(action: &str, sound: &Sound) {
    if std::env::var_os("CREATE_HORIZON_AUDIO_TRACE").is_some() {
        log::info!(
            "Create ambient: {}",
            serde_json::json!({"action":action,"event":sound.event,"key":format!("{:?}",sound.key),"position":sound.position,"volume":sound.volume,"pitch":sound.pitch,"looping":sound.looping,"distance_delay":sound.distance_delay,"source":8})
        );
    }
}
fn instance(sound: &Sound) -> SoundInstance {
    let mut instance = SoundInstance::new(
        sound.event,
        8,
        sound.volume,
        sound.pitch,
        SoundEngine::unseeded(),
        true,
        0,
        Attenuation::Linear,
        sound.position,
        false,
    );
    instance.ticking = Ticking::No;
    instance
}
fn play_local(
    game: &mut Game<'_>,
    event: &str,
    source: usize,
    position: [f64; 3],
    volume: f32,
    pitch: f32,
) {
    if std::env::var_os("CREATE_HORIZON_AUDIO_TRACE").is_some() {
        log::info!(
            "Create local sound: {}",
            serde_json::json!({"event":event,"source":source,"position":position,"volume":volume,"pitch":pitch,"distance_delay":true,"game_time":game.world.game_time})
        );
    }
    let random = horizon_core::random::LegacyRandom::new(game.particles.level_random().next_long());
    let instance = SoundInstance::at(event, source, volume, pitch, random, position);
    let distance_squared: f64 = (0..3)
        .map(|i| (game.audio.listener[i] - position[i]).powi(2))
        .sum();
    if distance_squared > 100.0 {
        let id = game.audio.engine.add(instance, false);
        game.audio
            .engine
            .play_delayed(id, (distance_squared.sqrt() / 40.0 * 20.0) as i32);
    } else {
        game.audio.play_instance(instance, false);
    }
}
pub(super) fn crank_packet(game: &mut Game<'_>, pos: [i32; 3], kind: i32, tag: &Tag) {
    if !client(game) {
        return;
    }
    let Some(name) = game
        .remote
        .as_ref()
        .and_then(|r| r.builtin.get("minecraft:block_entity_type")?.name(kind))
    else {
        return;
    };
    if name != "create:hand_crank" || game.native_block(pos) != "create:hand_crank" {
        return;
    }
    let Some(horizon_client::containers::BlockEntity::Mod(_)) =
        game.world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return;
    };
    let Tag::Compound(fields) = tag else {
        return;
    };
    if fields.is_empty() {
        return;
    }
    let in_use = super::create_port::integer(fields, "InUse");
    if std::env::var_os("CREATE_HORIZON_AUDIO_TRACE").is_some() {
        log::info!(
            "Create crank: {}",
            serde_json::json!({"event":"packet","position":pos,"in_use":in_use,"game_time":game.world.game_time})
        );
    }
    CLIENT.with(|c| {
        let mut cache = c.borrow_mut();
        prepare(game, &mut cache);
        // Repeated packets carrying10 refresh even if canonical NBT is still10.
        cache
            .cranks
            .entry(pos)
            .or_insert(Crank { in_use, time: None })
            .in_use = in_use;
    });
}
pub(super) fn predict_crank(game: &Game<'_>, pos: [i32; 3]) {
    if !client(game) || game.native_block(pos) != "create:hand_crank" {
        return;
    }
    if std::env::var_os("CREATE_HORIZON_AUDIO_TRACE").is_some() {
        log::info!(
            "Create crank: {}",
            serde_json::json!({"event":"prediction","position":pos,"in_use":10,"game_time":game.world.game_time})
        );
    }
    let prediction = Prediction {
        game: std::ptr::from_ref(game) as usize,
        world: std::ptr::from_ref(&game.world) as usize,
        dimension: game.dim.id().into(),
        pos,
    };
    CLIENT.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.game == prediction.game
            && cache.world == prediction.world
            && cache.dimension == prediction.dimension
        {
            cache
                .cranks
                .entry(pos)
                .or_insert(Crank {
                    in_use: 10,
                    time: None,
                })
                .in_use = 10;
        } else {
            // Input can precede the first audio tick of a newly joined level.
            // That tick owns stopping old handles, so defer only initialization.
            PREDICTIONS.with(|p| p.borrow_mut().push(prediction));
        }
    });
}
fn update_fan(game: &mut Game<'_>, cache: &mut Client) {
    if cache.fan.stopped {
        if let Some(id) = cache.fan_handle.take() {
            game.audio.stop_and_release(id);
        }
    } else if let Some(instance) = cache
        .fan_handle
        .and_then(|id| game.audio.engine.instance_mut(id))
    {
        instance.volume = cache.fan.volume;
        instance.pitch = cache.fan.pitch;
    }
}

/// Before fan client forces, as AirCurrent.Client.tickClientPlayerSounds.
pub(super) fn begin_tick(game: &mut Game<'_>) {
    if !client(game) || game.pause {
        return;
    }
    CLIENT.with(|c| {
        let mut cache = c.borrow_mut();
        prepare(game, &mut cache);
        PREDICTIONS.with(|p| {
            for prediction in p.borrow_mut().drain(..) {
                if cache.game == prediction.game
                    && cache.world == prediction.world
                    && cache.dimension == prediction.dimension
                {
                    cache
                        .cranks
                        .entry(prediction.pos)
                        .or_insert(Crank {
                            in_use: 10,
                            time: None,
                        })
                        .in_use = 10;
                }
            }
        });
        cache.ticks = (cache.ticks + 1) % 1_728_000;
        cache.fan.begin_tick();
        update_fan(game, &mut cache);
    });
}

/// Call immediately after applying the local player's current force. Source
/// enableClientPlayerSound ignores players while spectating another entity.
pub(super) fn enable_fan(game: &mut Game<'_>, rpm: f32, velocity: [f64; 3]) {
    if !client(game) || !game.controls_camera() {
        return;
    }
    CLIENT.with(|c| {
        let mut cache = c.borrow_mut();
        prepare(game, &mut cache);
        if cache.fan.enable(rpm, velocity) {
            if let Some(id) = cache.fan_handle.take() {
                game.audio.stop_and_release(id);
            }
            // AirCurrentSound starts at .01; enable immediately applies its
            // first fadeIn after SoundManager.play has resolved the event.
            let mut sound = SoundInstance::new(
                "minecraft:item.elytra.flying",
                4,
                0.01,
                cache.fan.pitch,
                SoundEngine::unseeded(),
                true,
                0,
                Attenuation::Linear,
                [0.0; 3],
                true,
            );
            sound.ticking = Ticking::No;
            cache.fan_handle = Some(game.audio.play_instance(sound, true).0);
        }
        update_fan(game, &mut cache);
    });
}

/// Reuse the presentation block traversal; no additional scan or retained NBT.
pub(super) fn block(game: &mut Game<'_>, pos: [i32; 3], block: &str, nbt: &Compound, rpm: f32) {
    if !client(game) || game.pause || game.clock.frozen() {
        return;
    }
    let Some(camera) = camera(game) else {
        return;
    };
    let camera_block = camera.map(|v| v.floor() as i32);
    if block == "create:hand_crank" {
        CLIENT.with(|c| {
            let mut cache = c.borrow_mut();
            prepare(game, &mut cache);
            let ticks = cache.ticks;
            let crank = cache.cranks.entry(pos).or_insert_with(|| Crank {
                in_use: super::create_port::integer(nbt, "InUse"),
                time: None,
            });
            if crank.time != Some(game.world.game_time) {
                crank.time = Some(game.world.game_time);
                // HandCrank.tick calls super.tick/tickAudio before InUse--.
                if let Some((volume, pitch)) = rules::crank_sound(ticks, crank.in_use) {
                    let position = pos.map(|v| f64::from(v) + 0.5);
                    play_local(
                        game,
                        "create:cranking",
                        4,
                        position,
                        volume * 0.075,
                        pitch * 0.5,
                    );
                    play_local(
                        game,
                        "create:cranking_compounded_1",
                        4,
                        position,
                        volume * 0.025,
                        pitch * 0.5,
                    );
                }
                if crank.in_use > 0 {
                    crank.in_use -= 1;
                }
            }
        });
    }
    if rpm == 0.0 {
        return;
    }
    let mut groups = Vec::with_capacity(2);
    let cog = block.contains("cogwheel") || block == "create:gearbox";
    let simple =
        block == "create:shaft" || block.ends_with("encased_shaft") || block.contains("cogwheel");
    if !simple && block != "create:gearbox" {
        groups.push(Group::Kinetic);
    }
    if cog {
        groups.push(Group::Cog);
    }
    if block == "create:millstone" && nbt.get(&"InputInventory".into())
        .and_then(|v| v.get("Items"))
        .is_some_and(|v| matches!(v, Tag::List(items) if items.iter().filter_map(horizon_items::item_nbt::item_from_nbt).any(|s|s.count>0)))
    { groups.push(Group::Milling); }
    CLIENT.with(|c| {
        let mut cache = c.borrow_mut();
        prepare(game, &mut cache);
        for group in groups {
            let pitch = rules::kinetic_pitch(rpm);
            let key = (group, rules::pitch_group(pitch));
            if cache.scapes.play(group, pos, pitch, camera_block, true) {
                for (layer, definition) in rules::layers(group).iter().enumerate() {
                    if definition.repeat_delay.is_none() {
                        let sound = cache
                            .scapes
                            .sound(key, layer, Some(camera), rules::DEFAULT_VOLUME_CAP)
                            .unwrap();
                        trace("start", &sound);
                        let id = game.audio.play_instance(instance(&sound), true).0;
                        cache.continuous.insert((key, layer), id);
                    }
                }
            }
        }
    });
}

/// After registering this tick's block audio, as SoundScapes.tick.
pub(super) fn end_tick(game: &mut Game<'_>) {
    if !client(game) || game.pause {
        return;
    }
    let camera = camera(game);
    CLIENT.with(|c| {
        let mut cache = c.borrow_mut();
        prepare(game, &mut cache);
        let handles: Vec<_> = cache
            .continuous
            .iter()
            .map(|(key, id)| (*key, *id))
            .collect();
        for ((key, layer), id) in handles {
            if let Some(sound) = cache
                .scapes
                .sound(key, layer, camera, rules::DEFAULT_VOLUME_CAP)
            {
                if let Some(instance) = game.audio.engine.instance_mut(id) {
                    instance.volume = sound.volume;
                    instance.pitch = sound.pitch;
                    instance.position = sound.position;
                }
                trace("update", &sound);
            }
        }
        let ticks = cache.ticks;
        let (repeating, removed) =
            cache
                .scapes
                .tick(ticks, camera, true, rules::DEFAULT_VOLUME_CAP);
        for sound in repeating {
            trace("repeat", &sound);
            // RepeatingSound.playLocalSound(...AMBIENT,...distanceDelay=true).
            play_local(
                game,
                sound.event,
                8,
                sound.position,
                sound.volume,
                sound.pitch,
            );
        }
        for key in removed {
            cache.continuous.retain(|(candidate, _), id| {
                if *candidate == key {
                    game.audio.stop_and_release(*id);
                    false
                } else {
                    true
                }
            });
        }
        cache
            .cranks
            .retain(|pos, _| game.native_block(*pos) == "create:hand_crank");
    });
}

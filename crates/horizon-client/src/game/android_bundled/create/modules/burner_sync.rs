// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Server-derived client fuel definitions, independent of resource-pack guesses.
use super::{Game, create_port_burner_sync_protocol as wire};
use horizon_client::items::ItemStack;
use super::create_engine::burner::Candidate;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const DATA: &str = "create:burner_data_v1";
const REQUEST: &str = "create:burner_data_request_v1";
const SPECIAL: &str = "create:superheated_blaze_burner_fuels";
const REGULAR: &str = "create:regular_blaze_burner_fuels";
const SPECIAL_TAG: &str = "create:blaze_burner_fuel/special";
const REGULAR_TAG: &str = "create:blaze_burner_fuel/regular";
const CAPTURABLE: &str = "create:blaze_burner_capturable";
static WORLD: AtomicU64 = AtomicU64::new(1);
struct Sending {
    frames: Arc<Vec<Vec<u8>>>,
    next: usize,
}
struct Server {
    key: wire::Generation,
    frames: Option<Arc<Vec<Vec<u8>>>>,
    compiled: bool,
    pending: HashMap<u128, Sending>,
    sent: HashMap<u128, u64>,
    last_tick: Option<u64>,
}
impl Server {
    fn new() -> Self {
        Self {
            key: wire::Generation {
                world: WORLD.fetch_add(1, Ordering::Relaxed),
                reload: 1,
            },
            frames: None,
            compiled: false,
            pending: HashMap::new(),
            sent: HashMap::new(),
            last_tick: None,
        }
    }
}
#[derive(Default)]
struct Client {
    receiver: wire::Receiver,
    attempted: Option<u64>,
}
thread_local! {
    static SERVERS: RefCell<HashMap<usize,Server>> = RefCell::new(HashMap::new());
    static CLIENTS: RefCell<HashMap<usize,Client>> = RefCell::new(HashMap::new());
}
fn id(game: &Game<'_>) -> usize {
    std::ptr::from_ref(game) as usize
}
fn full_id(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}
fn snapshot(game: &Game<'_>) -> Result<wire::Snapshot, String> {
    let map = |name| {
        game.native_data_map_entries("minecraft:item", name)
            .into_iter()
            .filter_map(|(id, value)| {
                let time = value
                    .as_i64()
                    .or_else(|| value["burn_time"].as_i64())
                    .and_then(|n| i32::try_from(n).ok())
                    .filter(|n| *n > 0)?;
                Some((full_id(&id), time))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let tag = |name| {
        game.items()
            .item_tags
            .get(name)
            .into_iter()
            .flatten()
            .map(|s| full_id(s))
            .collect::<BTreeSet<_>>()
    };
    let ordinary = game
        .items()
        .items
        .keys()
        .filter_map(|name| {
            let stack = ItemStack::new(name, 1);
            let time = game.res.providers().burn_time(game.items(), &stack, false);
            (time > 0).then(|| (name.to_string(), time))
        })
        .collect();
    let mut entity_tags = horizon_client::items::load_tags(
        &game.options.resources,
        "entity_type",
        &game.pack_files(),
    )
    .map_err(|error| error.to_string())?;
    Ok(wire::Snapshot {
        special: map(SPECIAL),
        regular: map(REGULAR),
        ordinary,
        special_tag: tag(SPECIAL_TAG),
        regular_tag: tag(REGULAR_TAG),
        capturable: entity_tags
            .remove(CAPTURABLE)
            .unwrap_or_default()
            .iter()
            .map(|s| full_id(s))
            .collect(),
    })
}
fn ready(game: &Game<'_>) -> bool {
    game.remote.is_none()
        && game.session_lock.is_some()
        && (game.serving.is_some() || game.options.dedicated.is_some())
}
fn compile(game: &Game<'_>) -> bool {
    if !ready(game) {
        return false;
    }
    let pointer = id(game);
    let needed = SERVERS.with(|s| s.borrow().get(&pointer).is_none_or(|s| !s.compiled));
    if !needed {
        return false;
    }
    let data = snapshot(game);
    SERVERS.with(|s| {
        let mut servers = s.borrow_mut();
        let state = servers.entry(pointer).or_insert_with(Server::new);
        state.compiled = true;
        match data
            .and_then(|data| wire::frames(state.key, &data).map_err(|error| format!("{error:?}")))
        {
            Ok(frames) => state.frames = Some(Arc::new(frames)),
            Err(error) => log::warn!("Create: fuel snapshot rejected: {error:?}"),
        }
    });
    true
}
fn queue(game: &Game<'_>, player: u128) {
    let pointer = id(game);
    let now = game.ticks;
    SERVERS.with(|s| {
        let mut servers = s.borrow_mut();
        let Some(state) = servers.get_mut(&pointer) else {
            return;
        };
        if state.pending.contains_key(&player)
            || state
                .sent
                .get(&player)
                .is_some_and(|last| now.saturating_sub(*last) < 100)
        {
            return;
        }
        if let Some(frames) = &state.frames {
            state.pending.insert(
                player,
                Sending {
                    frames: frames.clone(),
                    next: 0,
                },
            );
            state.sent.insert(player, now);
        }
    });
}
/// Cold cache is rebuilt once per server generation, never in a click handler.
pub(super) fn data_loaded(game: &mut Game<'_>, reload: bool) {
    let pointer = id(game);
    SERVERS.with(|s| {
        let mut servers = s.borrow_mut();
        if !reload || !servers.contains_key(&pointer) {
            servers.insert(pointer, Server::new());
        } else if let Some(state) = servers.get_mut(&pointer) {
            state.key.reload = state.key.reload.saturating_add(1);
            state.frames = None;
            state.compiled = false;
            state.pending.clear();
            state.sent.clear();
            state.last_tick = None;
        }
    });
    compile(game);
    for player in game.native_server_players() {
        queue(game, player.uuid);
    }
}
pub(super) fn logged_in(game: &mut Game<'_>, player: u128) {
    compile(game);
    queue(game, player);
}
pub(super) fn logged_out(game: &Game<'_>, player: u128) {
    SERVERS.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&id(game)) {
            state.pending.remove(&player);
            state.sent.remove(&player);
        }
    });
}
pub(super) fn clear(game: &Game<'_>) {
    // A replacement dimension installs heard_level before unloading the old
    // level. Definitions belong to the connection, rather than that dimension.
    if !game.create_burner_sync_has_level() {
        CLIENTS.with(|s| {
            s.borrow_mut().remove(&id(game));
        });
    }
    SERVERS.with(|s| {
        s.borrow_mut().remove(&id(game));
    });
}
pub(super) fn from_client(game: &mut Game<'_>, player: u128, channel: &str, data: &[u8]) {
    if channel != REQUEST
        || data != wire::REQUEST
        || !game.native_on("create")
        || !game
            .native_server_players()
            .iter()
            .any(|p| p.uuid == player)
    {
        return;
    }
    compile(game);
    queue(game, player);
}
pub(super) fn from_server(game: &Game<'_>, channel: &str, data: &[u8]) {
    if channel != DATA || game.remote.is_none() || !game.native_on("create") {
        return;
    }
    CLIENTS.with(|s| {
        let mut clients = s.borrow_mut();
        let state = clients.entry(id(game)).or_default();
        if let Err(error) = state.receiver.receive(data) {
            log::debug!("Create: rejected fuel snapshot: {error:?}");
        }
    });
}
pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_on("create") {
        return;
    }
    let pointer = id(game);
    let now = game.ticks;
    if game.remote.is_some() {
        let request = CLIENTS.with(|s| {
            let mut clients = s.borrow_mut();
            let state = clients.entry(pointer).or_default();
            if state.receiver.active.is_some()
                || state
                    .attempted
                    .is_some_and(|last| now.saturating_sub(last) < 100)
            {
                return false;
            }
            state.attempted = Some(now);
            true
        });
        if request {
            let _ = game.native_net_send_to_server(REQUEST, wire::REQUEST);
        }
        return;
    }
    if compile(game) {
        for player in game.native_server_players() {
            queue(game, player.uuid);
        }
    }
    let sends = SERVERS.with(|s| {
        let mut servers = s.borrow_mut();
        let Some(state) = servers.get_mut(&pointer) else {
            return Vec::new();
        };
        if state.last_tick == Some(now) {
            return Vec::new();
        }
        state.last_tick = Some(now);
        state
            .pending
            .iter()
            .map(|(player, send)| (*player, send.frames.clone(), send.next))
            .collect::<Vec<_>>()
    });
    for (player, frames, start) in sends {
        let mut next = start;
        for packet in frames.iter().skip(start).take(4) {
            if game
                .native_net_send_to_player(player, DATA, packet)
                .is_err()
            {
                break;
            }
            next += 1;
        }
        SERVERS.with(|s| {
            if let Some(state) = s.borrow_mut().get_mut(&pointer) {
                if next == frames.len() {
                    state.pending.remove(&player);
                } else if let Some(send) = state.pending.get_mut(&player) {
                    send.next = next;
                }
            }
        });
    }
}
pub(super) fn candidate(game: &Game<'_>, stack: &ItemStack) -> Option<Candidate> {
    CLIENTS.with(|s| {
        let clients = s.borrow();
        let data = &clients.get(&id(game))?.receiver.active.as_ref()?.1;
        let item = stack.item.as_ref();
        Some(Candidate::resolve(
            data.special.get(item).copied(),
            data.regular.get(item).copied(),
            data.special_tag.contains(item),
            data.ordinary.get(item).copied().unwrap_or(0),
            data.regular_tag.contains(item),
        ))
    })
}
pub(super) fn capturable(game: &Game<'_>, kind: &str) -> Option<bool> {
    CLIENTS.with(|s| {
        let clients = s.borrow();
        let data = &clients.get(&id(game))?.receiver.active.as_ref()?.1;
        Some(data.capturable.contains(&full_id(kind)))
    })
}

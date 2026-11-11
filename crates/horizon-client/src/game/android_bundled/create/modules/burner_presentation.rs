// Source burner presentation and opt-in observation of the actual client renderer input.
// No original resources are bundled; models/textures come from the player's pinned JAR.
use super::Game;
use super::native_block_entity_draw::BlockEntityDraw;
use crate::{
    containers::{BlockEntity, ModBlockEntity, ModRevision},
    mod_api::{
        NativeModelMaterial, NativeModelRenderOptions, NativeModelShade, NativeModelUvShift,
    },
    particles::{ParticleOptions, ParticleType},
};
use create_engine::{
    burner::Heat,
    burner_visual::{self as rules, Animation, FrameInput, Particle, ParticleKind, Visual},
    particles::ParticleRandom,
    placement::Direction,
};
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_world::registry::BlockStateId;
use std::{cell::RefCell, collections::BTreeMap, sync::OnceLock};

#[derive(Clone, PartialEq, Eq)]
struct ClientSnapshot {
    heat: String,
    fuel: i32,
    remaining: i32,
    creative: bool,
    goggles: bool,
    trainhat: bool,
    keys: Vec<String>,
}
#[derive(Default)]
struct TraceState {
    game: usize,
    dimension: String,
    time: i64,
    entries: BTreeMap<[i32; 3], (ModRevision, ClientSnapshot)>,
}
impl TraceState {
    fn prepare_for(&mut self, game: usize, dimension: &str, now: i64) {
        if self.game != game || self.dimension != dimension || now < self.time {
            *self = Self {
                game,
                dimension: dimension.into(),
                ..Self::default()
            };
        }
        self.time = now;
    }
    fn observe(
        &mut self,
        pos: [i32; 3],
        identity: ModRevision,
        snapshot: ClientSnapshot,
        mut live: impl FnMut([i32; 3]) -> Option<ModRevision>,
    ) -> bool {
        // World time stops during /tick freeze. An age limit therefore kept
        // every previously rendered, removed burner until time resumed. Bound
        // this opt-in diagnostic to actual live client entities instead.
        self.entries.retain(|pos, (identity, _)| {
            live(*pos).is_some_and(|current| identity.same_entity(current))
        });
        self.entries
            .insert(pos, (identity, snapshot.clone()))
            .is_none_or(|(_, previous)| previous != snapshot)
    }
}
thread_local! {static TRACE_STATE:RefCell<TraceState> = RefCell::new(TraceState::default());}

struct PartState {
    identity: ModRevision,
    animation: Animation,
    visual: Visual,
    phase: i32,
}
#[derive(Clone, Copy)]
struct Row {
    pos: [i32; 3],
    identity: ModRevision,
    facing: Direction,
    heat: Heat,
    above: bool,
}
#[derive(Default)]
struct Client {
    game: usize,
    dimension: String,
    next_phase: u32,
    entries: BTreeMap<[i32; 3], PartState>,
    rows_tick: Option<u64>,
    rows: Vec<Row>,
}
impl Client {
    fn prepare(&mut self, game: &Game<'_>) {
        self.prepare_for(std::ptr::from_ref(game) as usize, game.native_dimension());
    }
    fn prepare_for(&mut self, id: usize, dimension: &str) {
        if self.game != id || self.dimension != dimension {
            *self = Self {
                game: id,
                dimension: dimension.into(),
                ..Self::default()
            };
        }
    }
    fn retain_live(&mut self, mut live: impl FnMut([i32; 3]) -> Option<ModRevision>) {
        // Source visual deletion follows the client BE's lifetime, not its
        // position or every content revision. A same-ID /data load keeps the
        // BE; removing/replacing it or unloading its chunk does not.
        self.entries.retain(|pos, entry| {
            live(*pos).is_some_and(|identity| entry.identity.same_entity(identity))
        });
    }
    fn reload_visuals(&mut self, mut above: impl FnMut([i32; 3]) -> bool) {
        for (pos, entry) in &mut self.entries {
            entry.visual = Visual::new(above(*pos));
        }
    }
    fn entry(
        &mut self,
        pos: [i32; 3],
        identity: ModRevision,
        facing: Direction,
        above: bool,
    ) -> &mut PartState {
        if self
            .entries
            .get(&pos)
            .is_some_and(|e| !e.identity.same_entity(identity))
        {
            self.entries.remove(&pos);
        }
        self.entries.entry(pos).or_insert_with(|| {
            // Source phase is unsaved Object.hashCode()%13. Cross-runtime identity
            // hashes are not comparable; each actual client entity receives one
            // stable unsaved phase without consuming either level RNG.
            let phase = (self.next_phase % 13) as i32;
            self.next_phase = self.next_phase.wrapping_add(1);
            PartState {
                identity,
                animation: Animation::new(facing),
                visual: Visual::new(above),
                phase,
            }
        })
    }
}
thread_local! {static CLIENT:RefCell<Client> = RefCell::new(Client::default());}
struct Random<'a>(&'a mut LegacyRandom);
impl ParticleRandom for Random<'_> {
    fn next_float(&mut self) -> f32 {
        self.0.next_float()
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        self.0.next_int_bound(bound).unwrap_or(0)
    }
}
impl rules::Random for Random<'_> {
    fn next_double(&mut self) -> f64 {
        self.0.next_double()
    }
}

fn facing(game: &Game<'_>, pos: [i32; 3]) -> Direction {
    match game.native_block_property(pos, "facing").as_deref() {
        Some("north") => Direction::North,
        Some("west") => Direction::West,
        Some("east") => Direction::East,
        _ => Direction::South,
    }
}
fn heat(game: &Game<'_>, pos: [i32; 3]) -> Heat {
    game.native_block_property(pos, "blaze")
        .as_deref()
        .and_then(super::create_port_burner::heat)
        .unwrap_or(Heat::None)
}
fn valid_above(game: &Game<'_>, pos: [i32; 3]) -> bool {
    let above = [pos[0], pos[1] + 1, pos[2]];
    // BasinBlock.isBasin checks the actual entity type. FluidTankBlock is a
    // block-class test; tank mechanics themselves are outside the current slice.
    matches!(game.world.block_entities.get(&(above[0],above[1],above[2])),Some(BlockEntity::Mod(e)) if e.id=="create:basin")
        || game.native_block(above) == "create:fluid_tank"
}
fn emit(game: &mut Game<'_>, particle: Particle) {
    let kind = match particle.kind {
        ParticleKind::Smoke => ParticleType::LargeSmoke,
        ParticleKind::Flame => ParticleType::Flame,
        ParticleKind::Soul => ParticleType::SoulFireFlame,
    };
    game.particles.add(
        &ParticleOptions::simple(kind),
        particle.position,
        particle.velocity,
    );
}
fn rows(game: &Game<'_>) -> Vec<Row> {
    CLIENT.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.prepare(game);
        if cache.rows_tick != Some(game.ticks) {
            cache.rows = game
                .world
                .block_entities
                .iter()
                .filter_map(|(&(x, y, z), entity)| {
                    let BlockEntity::Mod(entity) = entity else {
                        return None;
                    };
                    let pos = [x, y, z];
                    (entity.id == "create:blaze_heater"
                        && game.native_block(pos) == "create:blaze_burner")
                        .then(|| Row {
                            pos,
                            identity: entity.revision(),
                            facing: facing(game, pos),
                            heat: heat(game, pos),
                            above: valid_above(game, pos),
                        })
                })
                .collect();
            let live: BTreeMap<_, _> = cache
                .rows
                .iter()
                .map(|row| (row.pos, row.identity))
                .collect();
            cache.retain_live(|pos| live.get(&pos).copied());
            cache.rows_tick = Some(game.ticks);
        }
        cache.rows.clone()
    })
}

/// Actual client BE tick boundary, before ParticleEngine.tick admits new requests.
pub(super) fn client_particles(game: &mut Game<'_>) {
    if !game.native_window()
        || game.remote.is_none()
        || !game.native_on("create")
        || game.pause
        || !game.clock.runs_normally()
    {
        return;
    }
    for row in rows(game) {
        let pos = row.pos;
        let center = pos.map(|v| f64::from(v) + 0.5);
        let Some(smoke_position) =
            rules::particle_start(row.heat, center, &mut Random(game.particles.level_random()))
        else {
            continue;
        };
        let empty = game
            .native_block_collision([pos[0], pos[1] + 1, pos[2]])
            .is_empty();
        let particles = rules::particle_finish(
            row.heat,
            center,
            smoke_position,
            empty,
            &mut Random(game.particles.level_random()),
        );
        for particle in particles.into_iter().flatten() {
            emit(game, particle);
        }
    }
}

/// Called from the normal post-level client tick, after the shared Catnip clock.
/// Flywheel's TickableVisual keeps chasing while /tick freeze stops BE particles.
pub(super) fn client_tick(game: &mut Game<'_>) {
    if !game.native_window() || game.remote.is_none() || !game.native_on("create") || game.pause {
        return;
    }
    for Row {
        pos,
        identity,
        facing,
        heat,
        above,
    } in rows(game)
    {
        let target = game
            .player
            .as_ref()
            .filter(|_| !game.own_invisible())
            .map_or(0.0, |player| {
                let dx = player.position.x - (f64::from(pos[0]) + 0.5);
                let dz = player.position.z - (f64::from(pos[2]) + 0.5);
                let angle = -horizon_core::geometry::trig::atan2(dz, dx);
                let degrees = if angle == 0.0 {
                    0.0
                } else {
                    (angle * 180.0 / std::f64::consts::PI) as f32
                };
                degrees - 90.0
            });
        CLIENT.with(|cache| {
            cache
                .borrow_mut()
                .entry(pos, identity, facing, above)
                .animation
                .tick(heat >= Heat::Fading && above, facing, target)
        });
    }
}

/// The original client-side tryUpdateFuel/applyCreativeFuel burst; callers must
/// use their actual source eligibility/prediction result, never a server echo.
pub(super) fn burst(game: &mut Game<'_>, pos: [i32; 3], soul: bool) {
    let mut particles = Vec::with_capacity(20);
    rules::particle_burst(
        soul,
        pos.map(|v| f64::from(v) + 0.5),
        &mut Random(game.particles.level_random()),
        |p| particles.push(p),
    );
    for particle in particles {
        emit(game, particle);
    }
}

fn part(
    game: &Game<'_>,
    draw: &mut BlockEntityDraw,
    model: &str,
    y: f32,
    angle: Option<f32>,
    json: bool,
    shade: NativeModelShade,
) {
    draw.push();
    draw.translate([0.0, y, 0.0]);
    if let Some(angle) = angle {
        draw.rotate_about_radians([0.0, 1.0, 0.0], angle, [0.5; 3]);
    }
    game.native_draw_terrain_model(
        draw,
        model,
        NativeModelRenderOptions {
            material: NativeModelMaterial::Model,
            shade,
            uv_shrink_ratio: if json { 4.0 / 2048.0 } else { 0.0 },
            ..Default::default()
        },
    );
    draw.pop();
}
pub(super) fn draw(
    game: &Game<'_>,
    pos: [i32; 3],
    state: BlockStateId,
    entity: &ModBlockEntity,
    draw: &mut BlockEntityDraw,
) {
    trace_client_state(game, pos, state, entity);
    let heat = heat(game, pos);
    let facing = facing(game, pos);
    let above = valid_above(game, pos);
    let time = super::create_port_particles::animation_time(draw.partial);
    let frame = CLIENT.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.prepare(game);
        let current = cache.entry(pos, entity.revision(), facing, above);
        current.visual.frame(
            FrameInput {
                heat,
                partial: draw.partial,
                time,
                phase: current.phase,
                goggles: entity.nbt.contains_key(&"Goggles".into()),
                hat: entity.nbt.contains_key(&"TrainHat".into()),
                stock_keeper: false,
            },
            current.animation,
        )
    });
    let old_light = draw.light;
    draw.light = 0xf0f0;
    part(
        game,
        draw,
        frame.head.path(),
        frame.head_y,
        Some(frame.angle),
        true,
        NativeModelShade::Entity,
    );
    if frame.goggles {
        part(
            game,
            draw,
            "create:block/blaze_burner/goggles_small",
            frame.head_y + 0.5,
            Some(frame.angle),
            true,
            NativeModelShade::Entity,
        );
    }
    if let Some(logistics) = frame.hat {
        draw.push();
        draw.translate([0.0, frame.head_y, 0.0]);
        draw.translate([0.0, 0.75, 0.0]);
        draw.rotate_about_radians(
            [0.0, 1.0, 0.0],
            frame.angle + std::f32::consts::PI,
            [0.5; 3],
        );
        draw.translate([0.5, 0.0, 0.5]);
        game.native_draw_terrain_model(
            draw,
            if logistics {
                "create:entity/logistics_hat"
            } else {
                "create:entity/train_hat"
            },
            NativeModelRenderOptions {
                material: NativeModelMaterial::Model,
                shade: NativeModelShade::Entity,
                uv_shrink_ratio: 4.0 / 2048.0,
                ..Default::default()
            },
        );
        draw.pop();
    }
    if let Some(soul) = frame.rods {
        part(
            game,
            draw,
            if soul {
                "create:block/blaze_burner/superheated_rods_small"
            } else {
                "create:block/blaze_burner/rods_small"
            },
            frame.small_y,
            None,
            false,
            // Actual pinned OBJ BakedQuads are unshaded; Flywheel's public
            // material reports cardinalLightingMode=OFF for both rod pairs.
            NativeModelShade::Off,
        );
        part(
            game,
            draw,
            if soul {
                "create:block/blaze_burner/superheated_rods_large"
            } else {
                "create:block/blaze_burner/rods_large"
            },
            frame.large_y,
            None,
            false,
            NativeModelShade::Off,
        );
    }
    if let Some(flame) = frame.flame {
        game.native_draw_terrain_model(
            draw,
            "create:block/blaze_burner/flame",
            NativeModelRenderOptions {
                material: NativeModelMaterial::Model,
                // The source OBJ flame likewise has shade=false/OFF.
                shade: NativeModelShade::Off,
                uv_shift: super::create_port::original_assets().then(|| NativeModelUvShift {
                    source_sprite: "create:block/blaze_burner_flame",
                    target_sprite: if flame.superheated {
                        "create:block/blaze_burner_flame_superheated_scroll"
                    } else {
                        "create:block/blaze_burner_flame_scroll"
                    },
                    scale: [0.5; 2],
                    offset: flame.uv(time),
                }),
                ..Default::default()
            },
        );
    }
    draw.light = old_light;
}

pub(super) fn reset() {
    TRACE_STATE.with(|state| *state.borrow_mut() = TraceState::default());
    CLIENT.with(|state| *state.borrow_mut() = Client::default());
}
/// Flywheel recreates visuals on resource reload, while their client block
/// entities (including both chasers and identity hashes) stay alive.
pub(super) fn resources_reloaded(game: &Game<'_>) {
    CLIENT.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.prepare(game);
        cache.reload_visuals(|pos| valid_above(game, pos));
    });
}

pub(super) fn sprites(out: &mut Vec<String>) {
    out.extend(
        [
            "create:block/blaze_burner_flame_scroll",
            "create:block/blaze_burner_flame_superheated_scroll",
        ]
        .map(str::to_owned),
    );
}

/// Called only by the actual client submit path. The opt-in gate precedes all collection,
/// string allocation and cache access; no arbitrary NBT values are logged.
pub(super) fn trace_client_state(
    game: &Game<'_>,
    pos: [i32; 3],
    state: BlockStateId,
    entity: &ModBlockEntity,
) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("CREATE_HORIZON_TRACE_POSES").is_some()) {
        return;
    }
    let number = |key| super::create_port::integer(&entity.nbt, key);
    let mut keys: Vec<_> = entity.nbt.keys().map(|key| key.to_string_lossy()).collect();
    keys.sort();
    let snapshot = ClientSnapshot {
        heat: game
            .native_block_state_property(state, "blaze")
            .unwrap_or("none")
            .into(),
        fuel: number("fuelLevel"),
        remaining: number("burnTimeRemaining"),
        creative: number("isCreative") != 0,
        // BlazeBurnerBlockEntity.read uses key presence, including an explicit 0b value.
        goggles: entity.nbt.contains_key(&"Goggles".into()),
        trainhat: entity.nbt.contains_key(&"TrainHat".into()),
        keys,
    };
    let now = game.world.game_time;
    let changed = TRACE_STATE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.prepare_for(
            std::ptr::from_ref(game) as usize,
            game.native_dimension(),
            now,
        );
        cache.observe(pos, entity.revision(), snapshot.clone(), |pos| {
            match game.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
                Some(BlockEntity::Mod(entity))
                    if entity.id == "create:blaze_heater"
                        && game.native_block(pos) == "create:blaze_burner" =>
                {
                    Some(entity.revision())
                }
                _ => None,
            }
        })
    });
    if changed {
        log::info!(
            "Create burner client state: {}",
            serde_json::json!({
                "event":"burner_client_state", "pos":pos, "time":now,
                "heat":snapshot.heat, "fuel":snapshot.fuel, "remaining":snapshot.remaining,
                "creative":snapshot.creative, "goggles":snapshot.goggles,
                "trainhat":snapshot.trainhat, "keys":snapshot.keys,
            })
        );
    }
}

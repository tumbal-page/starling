//! Native mod API types for the Create mod port.
//! These are placeholder types used by the Create mod's Android port modules.
//! Types that already exist in Horizon are re-exported; the rest are stubs.

pub use crate::game::native_hooks::NativeScreenView;
use horizon_client::{
    containers::ModBlockEntity,
    particles::ParticleEngine,
};
use horizon_core::random::{LegacyRandom, RandomSource};
use horizon_world::{nbt::Compound, registry::BlockStateId};
use std::ops::{BitOr, BitOrAssign};

/// A reference to a mod-tracked entity.
#[derive(Clone, Debug)]
pub struct NativeEntity {
    pub id: u32,
    pub position: [f64; 3],
}

/// A reference to the native camera.
#[derive(Clone, Debug)]
pub struct NativeCamera {
    pub position: [f64; 3],
}

/// A fluid stack for native mod fluid interactions.
#[derive(Clone, Debug)]
pub struct NativeFluidStack {
    pub fluid: String,
    pub amount: i32,
    pub components: Compound,
}

/// Action for fluid stack operations: simulate only, or execute for real.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeFluidAction {
    Simulate,
    Execute,
}

/// A drain request: any amount up to `maximum`, or an exact stack.
#[derive(Clone, Debug)]
pub enum NativeFluidDrain<'a> {
    Any(i32),
    Exact(&'a NativeFluidStack),
}

/// Render options for fluid boxes.
#[derive(Clone, Debug, Default)]
pub struct NativeFluidRenderOptions {
    pub argb: Option<u32>,
    pub render_bottom: bool,
    pub uv_shrink_ratio: f64,
}

/// A view of fluid tanks in a block entity.
#[derive(Clone, Debug)]
pub struct NativeFluidTankView {
    pub tanks: Vec<NativeFluidStack>,
    pub can_drain: bool,
    pub can_fill: bool,
    pub capacity: i32,
    pub content: i32,
    pub tank: i32,
}

/// Material for model rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeModelMaterial {
    Model,
}

impl Default for NativeModelMaterial {
    fn default() -> Self {
        NativeModelMaterial::Model
    }
}

/// Shade mode for model rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeModelShade {
    Entity,
    Off,
    Native,
}

impl Default for NativeModelShade {
    fn default() -> Self {
        NativeModelShade::Off
    }
}

/// UV shift mode for model rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeModelUvShift {
    pub source_sprite: &'static str,
    pub target_sprite: &'static str,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

/// Render options for terrain models.
#[derive(Clone, Debug, Default)]
pub struct NativeModelRenderOptions<'a> {
    pub material: NativeModelMaterial,
    pub shade: NativeModelShade,
    pub uv_shrink_ratio: f64,
    pub uv_shift: Option<NativeModelUvShift>,
    pub _phantom: std::marker::PhantomData<&'a ()>,
}

/// Block properties for reading/setting block state properties.
#[derive(Clone, Debug)]
pub struct BlockProperties {
    pub state: BlockStateId,
}
impl BlockProperties {
    pub fn get(&self, _key: &str) -> Option<String> {
        None
    }
}

/// A native command registered by a mod.
#[derive(Clone, Debug)]
pub struct NativeCommand {
    pub name: String,
    pub level: i32,
}

impl NativeCommand {
    pub fn op(name: &str, level: i32) -> Self {
        Self {
            name: name.to_string(),
            level,
        }
    }
}

/// A player on the server side, for native mod interactions.
#[derive(Clone, Debug)]
pub struct NativeServerPlayer {
    pub id: u32,
    pub game_mode: u8,
    pub permission: i32,
}

/// A revision/owner identifier for a mod block entity's container state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ModRevision(pub u64);
impl ModRevision {
    pub fn same_entity(&self, _other: &Self) -> bool {
        true
    }
}

/// Fields of a mod block entity: its id, NBT compound, item slots and menu data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModBlockEntityFields {
    pub id: String,
    pub nbt: horizon_world::nbt::Compound,
    pub items: Vec<Option<horizon_items::items::ItemStack>>,
    pub data: Vec<i32>,
}

/// Placeholder: HealthResources for damage tracking.
#[derive(Clone, Debug, Default)]
pub struct HealthResources {
    pub damage: Damage,
}

/// Placeholder: Damage type for health resources.
#[derive(Clone, Debug, Default)]
pub struct Damage {}
impl Damage {
    pub fn id(&self) -> u32 {
        0
    }
    pub fn in_tag(&self, _tag: &str) -> bool {
        false
    }
}

/// Placeholder: Create particle controller trait.
pub trait ParticleController {
    fn tick(&mut self, particle: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate;
}

/// Placeholder: Create particle type.
#[derive(Clone, Debug)]
pub struct Particle {
    pub previous: [f64; 3],
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub age: i32,
    pub lifetime: i32,
    pub rgba: [f32; 4],
    pub on_ground: bool,
    pub size: f32,
    pub gravity: f64,
}
impl Particle {
    pub fn remove(&mut self) {}
    pub fn advance_age(&mut self) -> bool {
        self.age += 1;
        self.age < self.lifetime
    }
    pub fn random(&mut self) -> &mut dyn ParticleRandom {
        unimplemented!()
    }
    pub fn set_full_bright(&mut self, _bright: bool) {}
    pub fn set_visible(&mut self, _visible: bool) {}
}

/// Placeholder: Create particle level context.
pub struct ParticleLevel<'a> {
    pub world: usize,
    _phantom: std::marker::PhantomData<&'a ()>,
}

/// Placeholder: Create particle update result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleUpdate {
    pub frame: Option<usize>,
    pub spawned: Vec<()>,
}

/// Placeholder: Create particle options.
#[derive(Clone, Debug, Default)]
pub struct ParticleOptions {
    pub kind: Option<ParticleType>,
    pub block: Option<(String, u32)>,
    pub color: [f32; 4],
}
impl ParticleOptions {
    pub fn simple(kind: ParticleType) -> Self {
        Self {
            kind: Some(kind),
            block: None,
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// Placeholder: Create particle type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleType {
    Flame,
    Block,
    SoulFireFlame,
    Smoke,
    LargeSmoke,
    Bubble,
    BubblePop,
    Poof,
    Dust,
    Spit,
    Cloud,
}

/// Placeholder: Create random source for particles.
pub trait ParticleRandom {
    fn next_float(&mut self) -> f32;
    fn next_int(&mut self, bound: i32) -> i32;
    fn next_gaussian(&mut self) -> f64;
}

/// Placeholder: Create extra particle data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extra {
    pub kind: u32,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

/// Placeholder: Create damage kind with named damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageKind {
    Drown,
    Named(NamedDamage),
    Generic,
}

/// Placeholder: Create named damage types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedDamage(pub u32);
impl NamedDamage {
    pub fn new(id: u32, flags: u32) -> Self {
        Self(id | (flags << 24))
    }
    pub const IS_FIRE: Self = Self(1 << 0);
    pub const BYPASSES_ARMOR: Self = Self(1 << 1);
    pub const NO_KNOCKBACK: Self = Self(1 << 2);
    pub const BYPASSES_INVULNERABILITY: Self = Self(1 << 3);
    pub const BYPASSES_EFFECTS: Self = Self(1 << 4);
    pub const BYPASSES_RESISTANCE: Self = Self(1 << 5);
    pub const PANIC_CAUSES: Self = Self(1 << 6);
    pub const IS_PROJECTILE: Self = Self(1 << 7);
    pub const IS_EXPLOSION: Self = Self(1 << 8);
    pub const IS_FALL: Self = Self(1 << 9);
    pub fn id(self) -> u32 {
        self.0 & 0xFFFFFF
    }
    pub fn flags(self) -> u32 {
        self.0 >> 24
    }
}
impl BitOr for NamedDamage {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl BitOrAssign for NamedDamage {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Extension trait for ModBlockEntity: methods used by Create port modules.
pub trait ModBlockEntityExt {
    fn revision(&self) -> ModRevision;
    fn clear_client_update_fields(&mut self);
    fn set_client_update_fields(&mut self);
}
impl ModBlockEntityExt for ModBlockEntity {
    fn revision(&self) -> ModRevision {
        ModRevision::default()
    }
    fn clear_client_update_fields(&mut self) {}
    fn set_client_update_fields(&mut self) {}
}

/// Extension trait for ParticleEngine: methods used by Create port modules.
pub trait ParticleEngineExt {
    fn controlled(&mut self, _name: &str, _pos: [f64; 3], _regardless: bool, _controller: impl ParticleController) -> Option<&mut Particle>;
    fn set_sprite_frame(&mut self, _particle: &mut Particle, _frame: usize);
    fn add(&mut self, _options: &ParticleOptions, _pos: [f64; 3], _vel: [f64; 3]);
    fn push(&mut self, _particle: Particle);
    fn snapshot(&self, _particle: &Particle) -> ParticleSnapshot;
    fn random(&mut self) -> &mut dyn ParticleRandom;
}
#[derive(Clone, Debug)]
pub struct ParticleSnapshot {
    pub position: [f64; 3],
    pub previous: [f64; 3],
    pub velocity: [f64; 3],
    pub age: i32,
    pub lifetime: i32,
    pub quad: Option<QuadSnapshot>,
}
#[derive(Clone, Debug)]
pub struct QuadSnapshot {
    pub sprite: String,
    pub size: f32,
    pub rgba: [f32; 4],
    pub uv: [f32; 4],
    pub light: u32,
}
impl ParticleEngineExt for ParticleEngine {
    fn controlled(&mut self, _name: &str, _pos: [f64; 3], _regardless: bool, _controller: impl ParticleController) -> Option<&mut Particle> {
        None
    }
    fn set_sprite_frame(&mut self, _particle: &mut Particle, _frame: usize) {}
    fn add(&mut self, _options: &ParticleOptions, _pos: [f64; 3], _vel: [f64; 3]) {}
    fn push(&mut self, _particle: Particle) {}
    fn snapshot(&self, _particle: &Particle) -> ParticleSnapshot {
        ParticleSnapshot {
            position: [0.0, 0.0, 0.0],
            previous: [0.0, 0.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            age: 0,
            lifetime: 0,
            quad: None,
        }
    }
    fn random(&mut self) -> &mut dyn ParticleRandom {
        unimplemented!()
    }
}

/// Extension trait for BlockEntityDraw: methods used by Create port modules.
pub trait BlockEntityDrawExt {
    fn rotate_about_radians(&mut self, _radians: f32, _axis: [f32; 3], _center: [f32; 3]);
}
impl BlockEntityDrawExt for crate::game::native_block_entity_draw::BlockEntityDraw {
    fn rotate_about_radians(&mut self, _radians: f32, _axis: [f32; 3], _center: [f32; 3]) {}
}

/// Extension trait for Remote: fields/methods used by Create port modules.
pub trait RemoteExt {
    fn use_target(&self) -> Option<[i32; 3]>;
    fn set_use_target(&mut self, _target: Option<[i32; 3]>);
    fn heard_level(&self) -> Option<()>;
}

impl ParticleRandom for LegacyRandom {
    fn next_float(&mut self) -> f32 {
        RandomSource::next_float(self)
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        RandomSource::next_int_bound(self, bound).unwrap_or(0)
    }
    fn next_gaussian(&mut self) -> f64 {
        RandomSource::next_gaussian(self)
    }
}

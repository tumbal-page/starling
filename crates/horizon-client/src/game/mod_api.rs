//! Native mod API types for the Create mod port.
//! These are placeholder types used by the Create mod's Android port modules.

use horizon_world::{nbt::Compound, registry::BlockStateId};
use serde_json::Value;

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
    None,
    Smooth,
    Flat,
}

impl Default for NativeModelShade {
    fn default() -> Self {
        NativeModelShade::None
    }
}

/// UV shift mode for model rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeModelUvShift {
    None,
    Shrink,
}

impl Default for NativeModelUvShift {
    fn default() -> Self {
        NativeModelUvShift::None
    }
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
}

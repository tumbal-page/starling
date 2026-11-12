//! Native world drawing types for the Create mod port.
//! Placeholder type used by `motor_value_world` for world-space rendering.

/// A drawing context for rendering quads and text in the world.
#[derive(Clone, Debug, Default)]
pub struct NativeWorldDraw {
    /// Current light level for shading.
    pub light: u32,
    /// Partial tick for interpolation.
    pub partial: f32,
}

impl NativeWorldDraw {
    /// Pushes the current transform onto the stack.
    pub fn push(&mut self) {}

    /// Pops the current transform from the stack.
    pub fn pop(&mut self) {}

    /// Translates the current transform by `offset`.
    pub fn translate(&mut self, _offset: [f64; 3]) {}

    /// Rotates the current transform around `axis` by `angle` radians, anchored at `center`.
    pub fn rotate_about(&mut self, _axis: [f32; 3], _angle: f32, _center: [f32; 3]) {}

    /// Scales the current transform by `factor`.
    pub fn scale(&mut self, _factor: [f32; 3]) {}

    /// Translates the transform to world position `pos`; returns false if the
    /// position is outside the renderable range.
    pub fn translate_world(&mut self, _pos: [f64; 3]) -> bool {
        true
    }
}

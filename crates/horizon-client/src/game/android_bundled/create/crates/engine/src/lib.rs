//! Game-independent parts, also compiled as the native bundle's engine crate.
#[path = "core/chain_drive.rs"]
pub mod chain_drive;
#[path = "core/kinetics.rs"]
pub mod kinetics;
#[path = "core/placement.rs"]
pub mod placement;
#[path = "core/pressing.rs"]
pub mod pressing;

#[path = "core/depot.rs"]
pub mod depot;

#[path = "core/controls.rs"]
pub mod controls;

#[path = "core/visual.rs"]
pub mod visual;

#[path = "core/waterwheel.rs"]
pub mod waterwheel;

#[path = "core/milling.rs"]
pub mod milling;

#[path = "core/fan.rs"]
pub mod fan;

#[path = "core/fan_entities.rs"]
pub mod fan_entities;

#[path = "core/particles.rs"]
pub mod particles;

#[path = "core/transport.rs"]
pub mod transport;

#[path = "core/drilling.rs"]
pub mod drilling;

#[path = "core/audio.rs"]
pub mod audio;

#[path = "core/basin.rs"]
pub mod basin;

#[path = "core/burner.rs"]
pub mod burner;

#[path = "core/burner_visual.rs"]
pub mod burner_visual;

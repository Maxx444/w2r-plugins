//! Unit Color: custom colours for the eight teams, local only. `model` and `ini` are pure and
//! tested everywhere; the game-facing layer is 32-bit Windows only, like the game.

pub mod ini;
pub mod model;

#[cfg(target_arch = "x86")]
mod plugin;

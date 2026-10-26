//! Unit Color: custom colours for the eight teams, local only. `model`, `profiles`, `json` and
//! `ini` are pure and tested everywhere; the game-facing layer is 32-bit Windows only, like the
//! game.

pub mod ini;
pub mod json;
pub mod model;
pub mod profiles;

#[cfg(target_arch = "x86")]
mod plugin;

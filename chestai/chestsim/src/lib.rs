//! Headless simulator of a player farming mob chests on a 1.8.9 server:
//! maps that are never the same, chests with hologram timers, custom mobs,
//! and the latency between the client and the server. The player's
//! physics come from the PvP simulator, which matches the game.
pub mod aabb;
mod atan_tab;
pub mod body;
pub mod env;
pub mod ffi;
pub mod game;
pub mod living;
pub mod mth;
pub mod terrain;
pub mod world;

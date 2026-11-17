//! Headless simulator of 1.8.9 sword combat between two players, written
//! against the decompiled 1.8.9 client and server so that the numbers match
//! the game.
pub mod aabb;
pub mod agent;
mod atan_tab;
pub mod body;
pub mod bot;
pub mod client;
pub mod env;
pub mod ffi;
pub mod living;
pub mod mth;
pub mod net;
pub mod replay;
pub mod server;
pub mod sim;
pub mod world;

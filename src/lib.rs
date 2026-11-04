//! Black & White (Lionhead 2001) recompiled to native code with Cranelift.
//!
//! The player's runblack.exe and Lionhead's DLLs are decoded (iced-x86), every function is
//! lifted to Cranelift IR and compiled to host code; game pointers stay 32-bit in a 4 GB
//! guest address space.

pub mod discover;
pub mod game;
pub mod image;
pub mod iscab;
pub mod iso;
pub mod jit;
pub mod lift;
pub mod rt;
pub mod setup;
pub mod smc;
pub mod platform;

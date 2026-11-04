//! C runtime functions of the game's two programs done by the host: they are called so often
//! that the recompiled code costs much of a frame.
//!
//! `__ftol` (0x4ae390 in the EXE, 0x10014210 in the DLL; 1.5 million calls a second in a
//! mission): st(0) to a 64-bit integer, truncated, popped, in edx:eax. The original switches
//! the FPU's rounding mode twice per call (fnstcw, fldcw), which is slow in recompiled x87.

use crate::rt::{Cpu, EAX, EDX, ESP};

pub const FTOL_EXE: u32 = 0x4ae390;
pub const FTOL_DLL: u32 = 0x1001_4210;

pub fn ftol(c: &mut Cpu) {
    let v = c.st(0);
    c.fpop();
    // out of range and NaN: the "integer indefinite" value fistp stores
    let i = if v.is_nan() || v >= 9.223_372_036_854_775_807e18 || v < -9.223_372_036_854_775_808e18 { i64::MIN } else { v.trunc() as i64 };
    c.r[EAX] = i as u32;
    c.r[EDX] = (i >> 32) as u32;
    // ret
    c.r[ESP] += 4;
}

/// `_chkstk` / `__alloca_probe` (eax: bytes of locals): makes room on the stack below the return
/// address and returns. Done by the host where the original returns with `jmp eax`, which the
/// recompiled code cannot follow (the return address is no function entry): patchw32.dll's.
pub fn chkstk(c: &mut Cpu) {
    let m = super::mem();
    let ret = m.rd32(c.r[ESP]);
    c.r[ESP] = c.r[ESP].wrapping_add(4).wrapping_sub(c.r[EAX]);
    c.r[EAX] = ret;
}

/// The bytes `_chkstk` starts with in Visual C++ 6's runtime that returns with `jmp eax`.
pub const CHKSTK_JMP: &[u8] = &[0x51, 0x8b, 0xcc, 0x83, 0xc1, 0x08, 0x3d, 0x00, 0x10, 0x00, 0x00];

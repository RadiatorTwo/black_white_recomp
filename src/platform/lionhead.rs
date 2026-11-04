//! Lionhead's DLLs (or parts of them) done by the host instead of being recompiled.
//!
//! LHDialogLib.dll: the EXE imports only its two `LHEditBox` functions (a modal text input
//! box), and the DLL needs the debug MFC (MFC42D.DLL, MSVCRTD.DLL). Until a call shows what
//! the game wants them for, they report themselves and return 0 (cancelled).

use super::{Import, cstr, arg, log};
use crate::rt::Cpu;

/// `int LHEditBox(const char *title, const char *prompt, const char *text, int size, char *out)`
fn edit_box(c: &mut Cpu) -> u32 {
    log!("LHEditBox({:?}, {:?}, {:?}): cancelled (not implemented yet)", cstr(arg(c, 0)), cstr(arg(c, 1)), cstr(arg(c, 2)));
    0
}

/// `int LHEditBox(const char *title, x, y, w, h, const char *text, int size, char *out)`
fn edit_box_at(c: &mut Cpu) -> u32 {
    log!("LHEditBox({:?} at {}, {}): cancelled (not implemented yet)", cstr(arg(c, 0)), arg(c, 1), arg(c, 2));
    0
}

pub const DIALOG_IMPORTS: &[Import] = &[
    ("?LHEditBox@@YAHPBD00HPAD@Z", -1, edit_box),
    ("?LHEditBox@@YAHPBDKKKK0HPAD@Z", -1, edit_box_at),
];

/// DSETUP.dll #11, DirectXSetupGetVersion(&version, &revision): DirectX 7.0a.
fn directx_setup_get_version(c: &mut Cpu) -> u32 {
    let m = super::mem();
    if arg(c, 0) != 0 {
        m.wr32(arg(c, 0), 0x0004_0007);
    }
    if arg(c, 1) != 0 {
        m.wr32(arg(c, 1), 700);
    }
    1
}

pub const DSETUP_IMPORTS: &[Import] = &[("#11", 2, directx_setup_get_version)];

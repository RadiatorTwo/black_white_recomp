//! Small system DLLs: IMM32 (input method editors: none, so the game's text input takes the
//! plain key messages), COMCTL32's `_TrackMouseEvent`, SHELL32's `ShellExecuteA`, VERSION
//! (no version resources are read).

use super::{Import, arg, cstr, log};
use crate::rt::Cpu;

fn ret0(_: &mut Cpu) -> u32 {
    0
}
fn ret1(_: &mut Cpu) -> u32 {
    1
}

pub const IMM32_IMPORTS: &[Import] = &[
    // no input context: the window has none
    ("ImmGetContext", 1, ret0),
    ("ImmReleaseContext", 2, ret1),
    ("ImmCreateContext", 0, ret0),
    ("ImmDestroyContext", 1, ret1),
    ("ImmAssociateContext", 2, ret0),
    ("ImmGetOpenStatus", 1, ret0),
    ("ImmSetOpenStatus", 2, ret0),
    ("ImmGetProperty", 2, ret0),
    ("ImmGetDescriptionA", 3, ret0),
    ("ImmGetCompositionStringA", 4, ret0),
    ("ImmSetCompositionStringA", 6, ret0),
    ("ImmSetCompositionWindow", 2, ret0),
    ("ImmGetCandidateListA", 4, ret0),
    ("ImmNotifyIME", 4, ret0),
];

/// _TrackMouseEvent: WM_MOUSELEAVE never comes (the mouse does not leave the game).
pub const COMCTL32_IMPORTS: &[Import] = &[("_TrackMouseEvent", 1, ret1)];

/// ShellExecuteA(window, verb, file, parameters, folder, show): web pages and other programs
/// are not opened.
fn shell_execute(c: &mut Cpu) -> u32 {
    log!("ShellExecuteA({}, {}): not opened", cstr(arg(c, 1)), cstr(arg(c, 2)));
    2 // ERROR_FILE_NOT_FOUND
}

pub const SHELL32_IMPORTS: &[Import] = &[("ShellExecuteA", 6, shell_execute)];

pub const VERSION_IMPORTS: &[Import] = &[
    ("GetFileVersionInfoSizeA", 2, ret0),
    ("GetFileVersionInfoA", 4, ret0),
    ("VerQueryValueA", 4, ret0),
];

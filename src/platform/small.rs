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

/// CoFileTimeToDosDateTime(FILETIME*, &date, &time): MS-DOS date and time (2-second steps).
fn co_file_time_to_dos_date_time(c: &mut Cpu) -> u32 {
    let m = super::mem();
    let ft = arg(c, 0);
    let t = m.rd32(ft) as u64 | (m.rd32(ft + 4) as u64) << 32;
    let secs = t / 10_000_000;
    let days = (secs / 86400) as i64 - 134_774; // 1601-01-01 to 1970-01-01
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    if !(1980..=2107).contains(&year) {
        return 0;
    }
    let s = secs % 86400;
    let date = ((year - 1980) << 9 | month << 5 | day) as u16;
    let time = ((s / 3600) << 11 | (s / 60 % 60) << 5 | (s % 60) / 2) as u16;
    m.wr16(arg(c, 1), date);
    m.wr16(arg(c, 2), time);
    1
}

/// CoCreateGuid(GUID*): a random one.
fn co_create_guid(c: &mut Cpu) -> u32 {
    let m = super::mem();
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let mut x = seed | 1;
    for k in 0..4 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        m.wr32(arg(c, 0) + 4 * k, x as u32);
    }
    0
}

/// CoTaskMemFree: our memory.
fn co_task_mem_free(c: &mut Cpu) -> u32 {
    super::free(arg(c, 0));
    0
}

pub const OLE32_IMPORTS: &[Import] = &[
    ("CoFileTimeToDosDateTime", 3, co_file_time_to_dos_date_time),
    ("CoCreateGuid", 1, co_create_guid),
    ("CoTaskMemFree", 1, co_task_mem_free),
    ("CoFreeUnusedLibraries", 0, ret0),
];

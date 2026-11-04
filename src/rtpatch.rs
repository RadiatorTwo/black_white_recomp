//! Applying an RTPatch file (patch 1.20's `BWPatch-V100-V102.rtp`) with the patch's own
//! `patchw32.dll`, recompiled and run like the game's DLLs, in a process of its own: RTPatch
//! ends by raising an exception for its own handler, which our runtime cannot catch (the
//! process stops there). `setup::patch_120` starts it and reads RTPatch's messages, which go
//! to standard output.

use crate::discover::{Code, Program, find_functions};
use crate::image::Image;
use crate::jit::compile;
use crate::platform::{self, INSTALL_DIR, guest_call, new_cstr};
use crate::rt::Hook;
use crate::smc::Smc;
use std::path::PathBuf;

/// The first argument that makes a program run `child` (its arguments: patchw32.dll, the
/// patch file's name inside the game folder, the game folder).
pub const CHILD_ARG: &str = "--rtpatch-child";

/// If this process was started as the RTPatch child: apply the patch and exit.
pub fn child_if_asked() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(CHILD_ARG) {
        return;
    }
    let (dll, rtp, dir) = (PathBuf::from(&args[2]), args[3].clone(), PathBuf::from(&args[4]));
    // guest calls nest on the host stack: a thread with a large one
    let code = std::thread::Builder::new().stack_size(256 << 20).spawn(move || child(&dll, &rtp, dir)).unwrap().join().unwrap();
    std::process::exit(code as i32);
}

/// Run `RTPatchApply32NoCall("<old dir>" "<patch file>")` with the game folder as `C:\Black & White`.
fn child(dll: &std::path::Path, rtp: &str, dir: PathBuf) -> u32 {
    let img = Image::load(dll).unwrap_or_else(|e| panic!("{}: {e}", dll.display()));
    let mut code = Code::new(&img);
    let mut seeds = vec![(img.entry, "DllMain".to_string())];
    seeds.extend(img.exports());
    let ranges = find_functions(&mut code, &seeds);
    let prog = Program::build(&mut code, &ranges);
    let smc = Smc::find(&prog, &mut code);
    // its _chkstk returns with `jmp eax`: done by the host
    let pattern = platform::crt::CHKSTK_JMP;
    let hooks: Vec<(u32, Hook)> = img
        .sections
        .iter()
        .filter(|s| s.exec)
        .flat_map(|s| (s.va..s.va + s.vsize).filter(|&a| img.read(a, pattern.len()).is_some_and(|b| b == pattern)).collect::<Vec<_>>())
        .map(|a| (a, platform::crt::chkstk as Hook))
        .collect();
    let compiled = compile(&prog, &smc, &mut code, &hooks);
    platform::user32::PRINT_FORMATTED.store(true, std::sync::atomic::Ordering::Relaxed);
    let cmd = format!("\"{INSTALL_DIR}\" \"{INSTALL_DIR}\\{rtp}\"");
    let r = platform::run_library(&img, "patchw32.dll", std::fs::canonicalize(&dir).unwrap_or(dir), |c, m| {
        let p = new_cstr(&cmd);
        guest_call(c, m.export("RTPatchApply32NoCall"), &[p])
    });
    drop(compiled);
    r
}

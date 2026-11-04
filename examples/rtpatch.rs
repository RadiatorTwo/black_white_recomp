//! Applies an RTPatch file (patch 1.20's `BWPatch-V100-V102.rtp`) to a game folder with the
//! patch's own `patchw32.dll`, recompiled and run like the game's DLLs:
//! `rtpatch <patchw32.dll> <patch file> <game folder> [command line]`.
//! The command line given to RTPatchApply32NoCall defaults to `<old dir> <patch file>` as the
//! game sees them (the game folder is `C:\Black & White`, the patch file is copied into it).

use black_white_recomp::discover::{Code, Program, find_functions};
use black_white_recomp::image::Image;
use black_white_recomp::jit::compile;
use black_white_recomp::platform::{self, INSTALL_DIR, guest_call, new_cstr};
use black_white_recomp::smc::Smc;
use std::path::PathBuf;

fn main() {
    // guest calls nest on the host stack: a thread with a large one
    let code = std::thread::Builder::new().stack_size(256 << 20).spawn(run).unwrap().join().unwrap();
    std::process::exit(code as i32);
}

fn run() -> u32 {
    let args: Vec<String> = std::env::args().collect();
    let (dll, rtp, dir) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]), PathBuf::from(&args[3]));
    let dir = std::fs::canonicalize(&dir).unwrap();
    let name = rtp.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::copy(&rtp, dir.join(&name)).expect("cannot copy the patch file into the game folder");
    let cmd = args.get(4).cloned().unwrap_or_else(|| format!("\"{INSTALL_DIR}\" \"{INSTALL_DIR}\\{name}\""));

    let img = Image::load(&dll).unwrap();
    let mut code = Code::new(&img);
    let mut seeds = vec![(img.entry, "DllMain".to_string())];
    seeds.extend(img.exports());
    let ranges = find_functions(&mut code, &seeds);
    let prog = Program::build(&mut code, &ranges);
    let smc = Smc::find(&prog, &mut code);
    // its _chkstk returns with `jmp eax`: done by the host
    let hooks: Vec<(u32, black_white_recomp::rt::Hook)> = img
        .sections
        .iter()
        .filter(|s| s.exec)
        .flat_map(|s| (s.va..s.va + s.vsize).filter(|&a| img.read(a, platform::crt::CHKSTK_JMP.len()).is_some_and(|b| b == platform::crt::CHKSTK_JMP)).collect::<Vec<_>>())
        .map(|a| (a, platform::crt::chkstk as black_white_recomp::rt::Hook))
        .collect();
    eprintln!("_chkstk at {:x?}", hooks.iter().map(|h| h.0).collect::<Vec<_>>());
    let compiled = compile(&prog, &smc, &mut code, &hooks);
    eprintln!("compiled {} functions, {} complete, missing {:?}", compiled.stats.functions, compiled.stats.complete, compiled.stats.missing);

    let r = platform::run_library(&img, "patchw32.dll", dir, |c, m| {
        let f = m.export("RTPatchApply32NoCall");
        eprintln!("RTPatchApply32NoCall({cmd:?})");
        let p = new_cstr(&cmd);
        guest_call(c, f, &[p])
    });
    eprintln!("result {r}");
    drop(compiled);
    r
}

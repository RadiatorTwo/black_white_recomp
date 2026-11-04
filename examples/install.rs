//! Makes the game folder from the CD image and the patches:
//! `cargo run --release --example install -- [image.bin] [patch 1.20 exe] [patch folder] [out]`
//! (defaults: `Black & White (Germany).bin`, `black_white_patch_v1_20.exe`, `Patch1.42`,
//! `game_files/BlackWhite`; `-` for a patch skips it: `- -` leaves the disc's files only).

use black_white_recomp::{rtpatch, setup};
use std::path::PathBuf;

fn main() {
    rtpatch::child_if_asked();
    let mut args = std::env::args().skip(1);
    let disc = PathBuf::from(args.next().unwrap_or("Black & White (Germany).bin".into()));
    let p120 = PathBuf::from(args.next().unwrap_or("black_white_patch_v1_20.exe".into()));
    let patch = PathBuf::from(args.next().unwrap_or("Patch1.42".into()));
    let out = PathBuf::from(args.next().unwrap_or("game_files/BlackWhite".into()));
    let mut shown = 0;
    let mut progress = |_: &std::path::Path| {
        shown += 1;
        if shown % 100 == 0 {
            eprintln!("{shown} files");
        }
    };
    let r = setup::install(&disc, &out, &mut progress).unwrap_or_else(|e| panic!("{e}"));
    println!("disc: {} files, {} bytes", r.files, r.bytes);
    if p120.as_os_str() != "-" {
        let report = setup::patch_120(&p120, &out, &mut progress).unwrap_or_else(|e| panic!("{e}"));
        let summary: Vec<&str> = report.lines().filter(|l| l.contains("....")).map(str::trim).collect();
        println!("patch 1.20: {}", summary.join("; "));
    }
    if patch.as_os_str() != "-" {
        let r = setup::overlay(&patch, &out, &mut progress).unwrap_or_else(|e| panic!("{e}"));
        println!("patch: {} files, {} bytes", r.files, r.bytes);
    }
}

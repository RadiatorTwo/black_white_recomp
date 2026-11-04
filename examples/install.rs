//! Makes the game folder from the CD image and a patch folder:
//! `cargo run --release --example install -- [image.bin] [patch folder] [out]`
//! (defaults: `Black & White (Germany).bin`, `Patch1.42`, `game_files/BlackWhite`; `-` as the patch
//! folder: the disc's files only, as the installer leaves them).

use black_white_recomp::setup;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let disc = PathBuf::from(args.next().unwrap_or("Black & White (Germany).bin".into()));
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
    if patch.as_os_str() != "-" {
        let r = setup::overlay(&patch, &out, &mut progress).unwrap_or_else(|e| panic!("{e}"));
        println!("patch: {} files, {} bytes", r.files, r.bytes);
    }
}

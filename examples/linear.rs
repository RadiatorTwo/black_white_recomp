//! Disassembles bytes in a straight line, whatever discovery thinks of them:
//! `linear <game folder> <addr> [bytes]` (default 64 bytes).
use black_white_recomp::game::Game;
use iced_x86::{Decoder, DecoderOptions};
use std::path::Path;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let game = Game::load(Path::new(&args[1])).unwrap();
    let hex = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
    let addr = hex(&args[2]);
    let n = args.get(3).map_or(64, |s| s.parse().unwrap());
    let bytes = game.all.read(addr, n).expect("not in the programs");
    let mut d = Decoder::with_ip(32, &bytes, addr as u64, DecoderOptions::NONE);
    for i in &mut d {
        let at = (i.ip() as u32 - addr) as usize;
        let raw: String = bytes[at..at + i.len()].iter().map(|b| format!("{b:02x}")).collect();
        println!("{:08x}  {raw:20}  {i}", i.ip());
    }
}

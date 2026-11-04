//! Disassembles one function as discovery sees it:  dis <game folder> <addr>
//! (`all`: every function, each headed by its address; an address inside a function shows that
//! function). Names from data/symbols.txt are shown
//! in the heading and after the targets of calls and jumps.
use black_white_recomp::discover::{Code, Program};
use black_white_recomp::game::Game;
use black_white_recomp::image::{named, symbols};
use std::path::Path;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let game = Game::load(Path::new(&args[1])).unwrap();
    let mut code = Code::new(&game.all);
    let ranges = game.functions(&mut code);
    let prog = Program::build(&mut code, &ranges);
    let line = |a: u32, i: &iced_x86::Instruction| {
        let target = (i.near_branch_target() as u32 != 0).then(|| symbols().get(&(i.near_branch_target() as u32))).flatten();
        match target {
            Some(n) => println!("{a:08x}  {i}  ; {n}"),
            None => println!("{a:08x}  {i}"),
        }
    };
    if args[2] == "all" {
        for (addr, f) in &prog.fns {
            println!("== {addr:08x} {}", f.name);
            for &a in &f.ins {
                line(a, &code.at(a).unwrap());
            }
        }
        return;
    }
    let addr = u32::from_str_radix(args[2].trim_start_matches("0x"), 16).unwrap();
    // an address inside a function: the function it is in
    let (&start, f) = prog.fns.range(..=addr).next_back().expect("no function there");
    println!("== {}", named(start));
    for &a in &f.ins {
        line(a, &code.at(a).unwrap());
    }
}

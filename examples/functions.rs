//! Finds and compiles the code of both programs:  functions <game folder>
//! Prints how many functions were found, how much of the code sections they cover, and which
//! functions use instructions the lifter does not support. `--gaps`: the bytes no function
//! decodes.
use black_white_recomp::discover::{Code, Program};
use black_white_recomp::game::{self, Game};
use black_white_recomp::jit::compile;
use black_white_recomp::smc::Smc;
use std::path::Path;
use std::time::Instant;
fn main() {
    let dir = std::env::args().nth(1).expect("path to the game folder");
    let game = Game::load(Path::new(&dir)).unwrap();
    let t = Instant::now();
    let mut code = Code::new(&game.all);
    let ranges = game.functions(&mut code);
    let prog = Program::build(&mut code, &ranges);
    let smc = Smc::find(&prog, &mut code);
    println!("{} functions, {} entries in all, {} code fields, {} ms", ranges.len(), prog.fns.len(), smc.imm.len() + smc.disp.len(), t.elapsed().as_millis());
    for s in game.all.sections.iter().filter(|s| s.exec) {
        let end = s.va + s.vsize;
        let mut covered = std::collections::HashSet::new();
        for f in prog.fns.values().filter(|f| f.entry >= s.va && f.entry < end) {
            for &a in &f.ins {
                covered.extend(a..a + code.at(a).unwrap().len() as u32);
            }
        }
        // bytes no function decodes, except padding
        let gaps = (s.va..end).filter(|a| !covered.contains(a) && !matches!(game.all.byte(*a), 0xcc | 0x90 | 0x00)).count();
        println!("{} {:#x}..{end:#x}: {} bytes decoded, {gaps} other bytes not", s.name, s.va, covered.len());
        if std::env::args().any(|a| a == "--gaps") {
            // runs of bytes no function decodes, with their first bytes
            let mut a = s.va;
            while a < end {
                let lo = a;
                while a < end && !covered.contains(&a) {
                    a += 1;
                }
                let real = (lo..a).filter(|b| !matches!(game.all.byte(*b), 0xcc | 0x90 | 0x00)).count();
                if real >= 16 {
                    println!("  gap {lo:#x}..{a:#x} ({real} bytes): {:02x?}", game.all.read(lo, 12).unwrap());
                }
                a += 1;
            }
        }
    }
    let c = compile(&prog, &smc, &mut code, &[]);
    println!("compiled: {} complete, missing {:?}", c.stats.complete, c.stats.missing);
    // which program the incomplete functions are in
    let mut by: std::collections::BTreeMap<&str, Vec<u32>> = Default::default();
    for (a, _) in &c.stats.incomplete {
        let name = std::iter::once((game::EXE, game.exe.base))
            .chain(game::DLLS)
            .filter(|&(_, base)| base <= *a)
            .max_by_key(|&(_, base)| base)
            .map_or("?", |(n, _)| n);
        by.entry(name).or_default().push(*a);
        if std::env::args().any(|x| x == "--missing") {
            let mut m = c.stats.incomplete.iter().find(|(b, _)| b == a).unwrap().1.clone();
            m.sort();
            m.dedup();
            println!("  {a:#x} {name}: {m:?}");
        }
    }
    for (name, fns) in by {
        println!("  {name}: {} incomplete: {:x?}", fns.len(), &fns[..fns.len().min(12)]);
    }
}

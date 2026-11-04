//! Lists the files of the Microsoft cabinet inside a file: `mscab <file>`.
fn main() {
    let path = std::env::args().nth(1).expect("path to a file with a cabinet");
    let data = std::fs::read(path).unwrap();
    for (name, bytes) in black_white_recomp::mscab::files(&data).unwrap_or_else(|e| panic!("{e}")) {
        println!("{:>9}  {name}", bytes.len());
    }
}

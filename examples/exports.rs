//! Lists a program's exports: `exports <exe or dll>`.
use black_white_recomp::image::Image;
use std::path::Path;
fn main() {
    let img = Image::load(Path::new(&std::env::args().nth(1).expect("path to an exe or dll"))).unwrap();
    for (a, name) in img.exports() {
        println!("{a:#010x}  {name}");
    }
}

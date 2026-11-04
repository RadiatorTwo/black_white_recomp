//! The game's programs: runblack.exe (patch 1.42, without SafeDisc) and Lionhead's DLLs it
//! imports or loads, recompiled together. Most of the DLLs want 0x10000000: each gets its own
//! megabyte from there (moved by its relocations), binkw32 too; QMixer keeps its base.

use crate::discover::Code;
use crate::image::Image;
use std::path::Path;

pub const EXE: &str = "runblack.exe";

/// The DLLs, each with the base it is loaded at. Not here: LHDialogLib.dll (needs the debug
/// MFC; `platform::lionhead` does its two functions) and the system DLLs in the game folder.
pub const DLLS: [(&str, u32); 8] = [
    ("LHLogR.dll", 0x1000_0000),
    ("LHaudiodllR.dll", 0x1010_0000),
    ("LHMultiplayerR.dll", 0x1020_0000),
    ("IFC22.dll", 0x1040_0000),
    ("wearasr.dll", 0x1050_0000),
    ("Plug Ins/ScriptLibraryR.dll", 0x1060_0000),
    ("QMixer.dll", 0x1800_0000),
    // wants 0x30000000, inside the guest heap (0x20000000-0x70000000)
    ("binkw32.dll", 0x1070_0000),
];

pub struct Game {
    pub exe: Image,
    /// in the order of `DLLS`
    pub dlls: Vec<Image>,
    /// all as one image, for finding and compiling the code
    pub all: Image,
}

impl Game {
    /// From the game's folder (made by `setup::install`).
    pub fn load(dir: &Path) -> std::io::Result<Game> {
        let mut exe = Image::load(&dir.join(EXE))?;
        // SELFMOD is code in a data section (read/write, not executable): the pixel
        // converters, called from .text (0x7dda5b, 0x7de610), rewriting their own constants
        for s in exe.sections.iter_mut().filter(|s| s.name == "SELFMOD") {
            s.exec = true;
        }
        let mut dlls = Vec::new();
        for (name, base) in DLLS {
            let mut dll = Image::load(&dir.join(name))?;
            if dll.base != base {
                dll.rebase(base);
            }
            dlls.push(dll);
        }
        let parts: Vec<&Image> = std::iter::once(&exe).chain(&dlls).collect();
        let all = Image::combine(&parts);
        Ok(Game { exe, dlls, all })
    }

    /// Where finding the functions starts: the entry points and the DLLs' exports.
    pub fn seeds(&self) -> Vec<(u32, String)> {
        let mut s = vec![(self.exe.entry, "entry".to_string())];
        for (dll, (name, _)) in self.dlls.iter().zip(DLLS) {
            let stem = name.rsplit('/').next().unwrap().trim_end_matches(".dll");
            s.push((dll.entry, format!("{stem}_DllMain")));
            s.extend(dll.exports());
        }
        s
    }

    /// Every function of the programs: sorted (start, end, name).
    pub fn functions(&self, code: &mut Code) -> Vec<(u32, u32, String)> {
        crate::discover::find_functions(code, &self.seeds())
    }
}

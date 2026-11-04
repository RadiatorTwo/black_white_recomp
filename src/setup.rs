//! Making the game folder without running the installer: the files of the InstallShield
//! cabinets on the CD image go where the setup puts them (the file groups below
//! `<TARGETDIR>`), the files beside the cabs that the groups list are copied from the disc,
//! then a patch folder (its files as installed, such as the unofficial v1.42 without SafeDisc)
//! is copied over them.

use crate::iscab::{self, Header, INVALID};
use crate::iso::{Entry, Iso};
use std::path::{Path, PathBuf};

/// Files written and their bytes.
#[derive(Default, Debug)]
pub struct Report {
    pub files: usize,
    pub bytes: u64,
}

/// `<TARGETDIR>\a\b` + `c\d` + `name` below `out`; None for other destinations (shared
/// InstallShield files, the setup's own support files).
fn target(out: &Path, dest: &str, dir: &str, name: &str) -> Option<PathBuf> {
    let rest = dest.strip_prefix("<TARGETDIR>")?;
    let mut p = out.to_path_buf();
    for part in rest.split('\\').chain(dir.split('\\')).chain([name]) {
        if !part.is_empty() {
            p.push(part);
        }
    }
    Some(p)
}

/// Unpack the game from the CD image `disc` into `out`.
pub fn install(disc: &Path, out: &Path, mut progress: impl FnMut(&Path)) -> Result<Report, String> {
    let mut iso = Iso::open(disc).ok_or_else(|| format!("{}: not a CD image", disc.display()))?;
    let root = iso.root();
    let whole = |iso: &mut Iso, name: &str| -> Result<(Entry, Vec<u8>), String> {
        let (_, e) = iso.find(&root, name, 2).ok_or_else(|| format!("{name} not on the disc"))?;
        let data = iso.read_range(&e, 0, e.size as u64).map_err(|err| format!("{name}: {err}"))?;
        Ok((e, data))
    };
    let header = Header::parse(&whole(&mut iso, "DATA1.HDR")?.1)?;
    let mut cabs: Vec<Option<Entry>> = Vec::new();
    let mut report = Report::default();
    for g in &header.groups {
        for k in g.first..=g.last {
            let Some(f) = header.files.get(k as usize) else { continue };
            let Some(path) = target(out, &g.dest, &f.dir, &f.name) else { break };
            if f.flags & INVALID != 0 {
                continue;
            }
            let data = if f.beside() {
                whole(&mut iso, &f.name)?.1
            } else {
                let v = f.volume as usize;
                if cabs.len() <= v {
                    cabs.resize(v + 1, None);
                }
                if cabs[v].is_none() {
                    let name = format!("DATA{v}.CAB");
                    cabs[v] = Some(iso.find(&root, &name, 0).ok_or_else(|| format!("{name} not on the disc"))?.1);
                }
                let cab = cabs[v].as_ref().unwrap();
                let packed = iso.read_range(cab, f.offset, f.packed).map_err(|e| format!("{}: {e}", f.name))?;
                iscab::unpack(f, &packed)?
            };
            write(&path, &data)?;
            progress(&path);
            report.files += 1;
            report.bytes += data.len() as u64;
        }
    }
    Ok(report)
}

/// Copy the folder `from` with everything in it over `out`.
pub fn overlay(from: &Path, out: &Path, progress: &mut impl FnMut(&Path)) -> Result<Report, String> {
    let mut report = Report::default();
    let list = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for e in list {
        let e = e.map_err(|e| e.to_string())?;
        let (src, dst) = (e.path(), out.join(e.file_name()));
        if src.is_dir() {
            let r = overlay(&src, &dst, progress)?;
            report.files += r.files;
            report.bytes += r.bytes;
        } else {
            let data = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            write(&dst, &data)?;
            progress(&dst);
            report.files += 1;
            report.bytes += data.len() as u64;
        }
    }
    Ok(report)
}

fn write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))
}

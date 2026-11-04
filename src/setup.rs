//! Making the game folder without running any installer: the files of the InstallShield
//! cabinets on the CD image go where the setup puts them (the file groups below
//! `<TARGETDIR>`), the files beside the cabs that the groups list are copied from the disc;
//! then the official patch 1.20 (`patch_120`), then a patch folder (its files as installed,
//! the unofficial v1.42 without SafeDisc, which needs 1.20 first) is copied over them.

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

/// Put every file of `header`'s groups below `<TARGETDIR>` where the setup would, `data` giving
/// a file's contents.
fn install_groups(
    header: &Header,
    out: &Path,
    mut data: impl FnMut(&iscab::File) -> Result<Vec<u8>, String>,
    progress: &mut impl FnMut(&Path),
) -> Result<Report, String> {
    let mut report = Report::default();
    for g in &header.groups {
        for k in g.first..=g.last {
            let Some(f) = header.files.get(k as usize) else { continue };
            let Some(path) = target(out, &g.dest, &f.dir, &f.name) else { break };
            if f.flags & INVALID != 0 {
                continue;
            }
            let bytes = data(f)?;
            write(&path, &bytes)?;
            progress(&path);
            report.files += 1;
            report.bytes += bytes.len() as u64;
        }
    }
    Ok(report)
}

/// Unpack the game from the CD image `disc` into `out`.
pub fn install(disc: &Path, out: &Path, mut progress: impl FnMut(&Path)) -> Result<Report, String> {
    let mut iso = Iso::open(disc).ok_or_else(|| format!("{}: not a CD image", disc.display()))?;
    let root = iso.root();
    let whole = |iso: &mut Iso, name: &str| -> Result<Vec<u8>, String> {
        let (_, e) = iso.find(&root, name, 2).ok_or_else(|| format!("{name} not on the disc"))?;
        iso.read_range(&e, 0, e.size as u64).map_err(|err| format!("{name}: {err}"))
    };
    let header = Header::parse(&whole(&mut iso, "DATA1.HDR")?)?;
    let mut cabs: Vec<Option<Entry>> = Vec::new();
    install_groups(
        &header,
        out,
        |f| {
            if f.beside() {
                return whole(&mut iso, &f.name);
            }
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
            iscab::unpack(f, &packed)
        },
        &mut progress,
    )
}

/// Apply the official patch 1.20 (`black_white_patch_v1_20.exe`) to the installed game in
/// `out`, without its installer: the EXE holds a cabinet (`mscab`) with the InstallShield setup;
/// its files for the game folder are copied (InfoScriptPatch2.txt, LHLogR.dll) and its RTPatch
/// file is applied by the patch's own `patchw32.dll`, recompiled and run in a child process
/// (`rtpatch::child`; RTPatch ends with an exception our runtime cannot catch). Returns
/// RTPatch's report.
pub fn patch_120(exe: &Path, out: &Path, mut progress: impl FnMut(&Path)) -> Result<String, String> {
    let bytes = std::fs::read(exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let disk: std::collections::HashMap<String, Vec<u8>> = crate::mscab::files(&bytes)?
        .into_iter()
        .map(|(name, data)| (name.rsplit('\\').next().unwrap().to_ascii_lowercase(), data))
        .collect();
    let get = |name: &str| disk.get(name).ok_or_else(|| format!("{name} not in the patch"));
    let header = Header::parse(get("data1.hdr")?)?;
    let unpack = |f: &iscab::File| -> Result<Vec<u8>, String> {
        let cab = get(&format!("data{}.cab", f.volume))?;
        let packed = cab.get(f.offset as usize..(f.offset + f.packed) as usize).ok_or_else(|| format!("{}: cut", f.name))?;
        iscab::unpack(f, packed)
    };
    install_groups(&header, out, unpack, &mut progress)?;

    let file = |pred: &dyn Fn(&str) -> bool| header.files.iter().find(|f| f.flags & INVALID == 0 && pred(&f.name.to_ascii_lowercase()));
    let dll = file(&|n| n == "patchw32.dll").ok_or("patchw32.dll not in the patch")?;
    let rtp = file(&|n| n.ends_with(".rtp")).ok_or("no .rtp file in the patch")?;
    let (dll_path, rtp_path) = (out.join("~patchw32.dll"), out.join(&rtp.name));
    write(&dll_path, &unpack(dll)?)?;
    write(&rtp_path, &unpack(rtp)?)?;
    let run = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg(crate::rtpatch::CHILD_ARG)
        .arg(&dll_path)
        .arg(&rtp.name)
        .arg(out)
        .output();
    let _ = std::fs::remove_file(&dll_path);
    let _ = std::fs::remove_file(&rtp_path);
    clear_read_only(out);
    let run = run.map_err(|e| format!("RTPatch: {e}"))?;
    let text = String::from_utf8_lossy(&run.stdout).into_owned();
    if !text.contains("Patch File Processing Complete") || text.contains("error ept") {
        return Err(format!("RTPatch failed:\n{text}{}", String::from_utf8_lossy(&run.stderr)));
    }
    Ok(text)
}

/// Make every file below `dir` writable (RTPatch leaves the patched ones read-only).
fn clear_read_only(dir: &Path) {
    let Ok(list) = std::fs::read_dir(dir) else { return };
    for e in list.flatten() {
        let p = e.path();
        if p.is_dir() {
            clear_read_only(&p);
        } else if let Ok(md) = p.metadata() {
            let mut perm = md.permissions();
            if perm.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                let _ = std::fs::set_permissions(&p, perm);
            }
        }
    }
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

//! Microsoft cabinets (MSCF) with MSZIP compression, as InstallShield's "Package for the Web"
//! wraps the official patches (`black_white_patch_v1_20.exe`: an EXE with the cabinet
//! appended, holding the `Disk1` of an InstallShield 6 setup).
//!
//! MSZIP: each data block is `CK` + a deflate stream whose back references may reach into
//! the blocks before it of the same folder, so a folder is inflated into one buffer.

/// The files of the first cabinet in `data` (searched for its signature): (name, contents).
pub fn files(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let at = find(data).ok_or("no cabinet found")?;
    let cab = &data[at..];
    read(cab).ok_or_else(|| "damaged cabinet".into()).and_then(|r| r)
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// The first `MSCF` whose header is consistent (reserved fields 0, version 1.3).
fn find(data: &[u8]) -> Option<usize> {
    (0..data.len().saturating_sub(36)).find(|&o| {
        &data[o..o + 4] == b"MSCF" && u32_at(data, o + 4) == Some(0) && data[o + 24] == 3 && data[o + 25] == 1 && u32_at(data, o + 8).is_some_and(|n| o + n as usize <= data.len())
    })
}

fn read(cab: &[u8]) -> Option<Result<Vec<(String, Vec<u8>)>, String>> {
    let (nfolders, nfiles, flags) = (u16_at(cab, 26)? as usize, u16_at(cab, 28)? as usize, u16_at(cab, 30)?);
    let first_file = u32_at(cab, 16)? as usize;
    // optional reserved areas (flag 4): header, folder and data block reserves
    let (mut o, folder_res, data_res) = if flags & 4 != 0 {
        (36 + 4 + u16_at(cab, 36)? as usize, *cab.get(38)? as usize, *cab.get(39)? as usize)
    } else {
        (36, 0, 0)
    };
    if flags & 3 != 0 {
        return Some(Err("cabinets spanning several files are not supported".into()));
    }
    let mut folders = Vec::new();
    for _ in 0..nfolders {
        let (first, blocks, kind) = (u32_at(cab, o)? as usize, u16_at(cab, o + 4)? as usize, u16_at(cab, o + 6)? & 0xf);
        if kind > 1 {
            return Some(Err(format!("compression {kind} is not supported (MSZIP only)")));
        }
        folders.push(inflate_folder(cab, first, blocks, kind == 1, data_res)?);
        o += 8 + folder_res;
    }
    let mut out = Vec::with_capacity(nfiles);
    let mut o = first_file;
    for _ in 0..nfiles {
        let (size, at, folder) = (u32_at(cab, o)? as usize, u32_at(cab, o + 4)? as usize, u16_at(cab, o + 8)? as usize);
        let name_at = o + 16;
        let len = cab.get(name_at..)?.iter().position(|&b| b == 0)?;
        let name: String = cab[name_at..name_at + len].iter().map(|&b| b as char).collect();
        let bytes = folders.get(folder)?.get(at..at + size)?.to_vec();
        out.push((name, bytes));
        o = name_at + len + 1;
    }
    Some(Ok(out))
}

/// A folder's blocks, inflated (MSZIP) or copied (stored) into one buffer.
fn inflate_folder(cab: &[u8], mut o: usize, blocks: usize, mszip: bool, data_res: usize) -> Option<Vec<u8>> {
    use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF};
    // the sizes first: one buffer the deflate streams can look back into
    let mut total = 0;
    let mut p = o;
    for _ in 0..blocks {
        total += u16_at(cab, p + 6)? as usize;
        p += 8 + data_res + u16_at(cab, p + 4)? as usize;
    }
    let mut out = vec![0u8; total];
    let mut pos = 0;
    for _ in 0..blocks {
        let (packed, size) = (u16_at(cab, o + 4)? as usize, u16_at(cab, o + 6)? as usize);
        let data = cab.get(o + 8 + data_res..o + 8 + data_res + packed)?;
        if mszip {
            let stream = data.strip_prefix(b"CK")?;
            let mut d = DecompressorOxide::new();
            let (_, _, written) = decompress(&mut d, stream, &mut out[..pos + size], pos, TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF);
            if written != size {
                return None;
            }
        } else {
            out.get_mut(pos..pos + size)?.copy_from_slice(data.get(..size)?);
        }
        pos += size;
        o += 8 + data_res + packed;
    }
    Some(out)
}

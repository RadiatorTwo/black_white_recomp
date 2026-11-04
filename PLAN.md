# PLAN — Black & White recompile

## Done

- 2026-11-04: Disc analysis: InstallShield 6 cabs, SafeDisc 2 on both retail EXEs; patch 1.42
  gives an unprotected `runblack.exe`.
- 2026-11-04: Game folder without the installer: `iso.rs`, `iscab.rs`, `setup.rs`,
  `examples/install.rs`. Output `game_files/BlackWhite` (980 files, 491 MB), MD5 of every cab
  file checked.

- 2026-11-04: Recompiler core from rustschweine; `game.rs` loads runblack.exe + 9 DLLs.
  35,025 functions found, all compiled, 97 with unsupported instructions (CLAUDE.md, "The
  code"). `SELFMOD` analysed: only `sub_fc1285` is reachable (stores pitches); the rest,
  with its opcode rewriting, is dead code here.
- 2026-11-04: CPUID without MMX (binkw32, QMixer take plain paths; the EXE's SSE paths need
  family 6); x87 environment instructions. 92 functions left with unsupported instructions,
  none reachable.

- 2026-11-04: Platform layer from rustschweine with general DLL loading; the game runs up
  to its first BeginScene and the audio start (CLAUDE.md, "Platform layer").

## Current state (2026-11-05): not running yet

The game loads far (scripts, ~7,000 German texts, creatures, animations, meshes) and crashes
at "GetNumberOfProfiles" (0x56e160 reads the empty multiplayer text table [0xD17C98]):
`Scripts\InfoScriptMultiplayer2.txt` is missing. Patch 1.42 expects patch 1.20 to be
installed first; 1.20 adds `Scripts\InfoScriptMultiplayer2.txt`, `Scripts\InfoScriptPatch2.txt`,
`Online Maps\ConditionTemplate.txt` and changes more files, inside its RTPatch file
(`BWPatch-V100-V102.rtp`, undocumented compression). `InfoScriptPatch2.txt` was copied into
`game_files/BlackWhite/Scripts` by hand for a test (it is also a plain file in the 1.20 cab).

Chosen way: run the real 1.20 installer on a clean 1.00 copy.
1. Done: `cargo run --release --example install -- "Black & White (Germany).bin" - game_files/bw_v100`
   (`-`: disc files only).
2. Done (2026-11-05): the 1.20 installer reads `GameDir` from **HKCU** (its setup.inx sets
   the default root to HKEY_CURRENT_USER), not HKLM: set
   `HKCU\Software\Lionhead Studios Ltd\Black & White` GameDir = `C:\!Code2\black_white_recomp\game_files\bw_v100`
   (no admin needed). The HKLM key from the first try is not needed (remove it as admin).
   The user's 1.42 install had left `GameVersion = 142` (DWORD) there; deleted so 1.20 sees
   a 1.00 install (the disc installer writes no GameVersion; 1.20 writes 120 afterwards).
   Restore later with `reg add ... /v GameVersion /t REG_DWORD /d 142`.
3. To do by the user: run `black_white_patch_v1_20.exe`, target `game_files\bw_v100`.
4. Then: `setup.rs` takes the 1.20 result (disc, then 1.20, then 1.42) and the game runs on.
   Delete the registry key afterwards.

## Next

1. Patch 1.20 files into the game folder (above), then keep running the game.
2. Audio: QMixer and binkw32 find no waveOut device (silent); real output later.
3. Settings the patch 1.42 Setup.exe writes (resolution, windowed mode).

## Later

- Creature Isle: `CreatureIsle.exe` is SafeDisc 2.51; needs an unprotected EXE or our own
  decryption. Installs to `<TARGETDIR>\CreatureIsle`.

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

## Current state (2026-11-05): runs to the name login

Patch 1.20 is applied without its installer: `examples/rtpatch.rs` recompiles the patch's own
`patchw32.dll` and calls `RTPatchApply32NoCall("<old dir>" "<patch file>")` on a copy of the
disc install (12 files modified, 8 added, 3 deleted). The game folder is now: disc
(`game_files/bw_v100`) -> 1.20 (`game_files/bw_v120`) -> `InfoScriptPatch2.txt` (plain file in
the 1.20 cab) -> Patch1.42. Built by hand for now (read-only flags cleared before copying
1.42 over it). With it the game gets past its loading to the name login.

The 1.20 installer could not be used: it reads GameDir and GameVersion from HKCU and still
said BW_NOT_INSTALLED (cause unknown, maybe its elevated engine). Registry values changed by
us for that attempt, under `HKCU\Software\Lionhead Studios Ltd\Black & White`: GameDir set
to `game_filesw_v100`, GameVersion 142 -> 100; HKLM key with GameDir added by the user.

To do: automate it in `setup.rs` (the patch EXEs are an InstallShield PFTW wrapper around an
MS-CAB (MSZIP) with an IS6 setup: patchw32.dll, the .rtp, InfoScriptPatch2.txt).

## Next

1. Keep running the game from the name login on.
2. Automate patch 1.20 in `setup.rs` (above).
3. Audio: QMixer and binkw32 find no waveOut device (silent); real output later.
4. Settings the patch 1.42 Setup.exe writes (resolution, windowed mode).

## Later

- Creature Isle: `CreatureIsle.exe` is SafeDisc 2.51; needs an unprotected EXE or our own
  decryption. Installs to `<TARGETDIR>\CreatureIsle`.

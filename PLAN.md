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

- 2026-11-05: Mouse and keyboard reach the game (name login: typing, symbol picker); the
  name login shows after ~5 s instead of 48 s (skipped intro videos are missing for
  GetFileAttributesA too).
- 2026-11-05: Patch 1.20 applied without its installer (`setup::patch_120`, `mscab.rs`,
  `rtpatch.rs`); the game reaches the name login.
- 2026-11-04: Platform layer from rustschweine with general DLL loading; the game runs up
  to its first BeginScene and the audio start (CLAUDE.md, "Platform layer").

## Current state (2026-11-05): name login takes input

`examples/install` makes the whole game folder: disc -> patch 1.20 (`setup::patch_120`, its
own patchw32.dll recompiled) -> patch 1.42. The game loads in ~5 s (code cache) and shows the name login, which takes
mouse and keyboard. The
registry values changed for the failed 1.20-installer attempt are restored (HKCU GameVersion
142, GameDir removed); an HKLM GameDir key the user added as admin may remain.

## Next

1. Keep running the game from the name login on.
2. Audio: QMixer and binkw32 find no waveOut device (silent); real output later.
3. Settings the patch 1.42 Setup.exe writes (resolution, windowed mode).

## Later

- Creature Isle: `CreatureIsle.exe` is SafeDisc 2.51; needs an unprotected EXE or our own
  decryption. Installs to `<TARGETDIR>\CreatureIsle`.

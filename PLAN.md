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

## Next

1. Audio: QMixer's OpenFile and waveOut (or what QMixer really outputs to).
2. Keep running the game: the next missing imports, then the first picture.
3. Settings the patch 1.42 Setup.exe writes (resolution, windowed mode).

## Later

- Creature Isle: `CreatureIsle.exe` is SafeDisc 2.51; needs an unprotected EXE or our own
  decryption. Installs to `<TARGETDIR>\CreatureIsle`.

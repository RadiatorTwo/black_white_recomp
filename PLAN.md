# PLAN — Black & White recompile

## Done

- 2026-11-04: Disc analysis: InstallShield 6 cabs, SafeDisc 2 on both retail EXEs; patch 1.42
  gives an unprotected `runblack.exe`.
- 2026-11-04: Game folder without the installer: `iso.rs`, `iscab.rs`, `setup.rs`,
  `examples/install.rs`. Output `game_files/BlackWhite` (980 files, 491 MB), MD5 of every cab
  file checked.

## Next

1. Take the recompiler core from rustschweine (`image.rs`, `discover.rs`, `jit.rs`, `lift*`,
   `rt.rs`, `smc.rs`, `platform/`) and load `runblack.exe` plus its DLLs.
2. Function discovery on `runblack.exe`; look at the `SELFMOD` section (who writes it, when).
3. List the imports actually called, start with kernel32/user32/ddraw.
4. Registry keys and config the setup writes (`SETUP.INX`, `Setup.exe` of patch 1.42).

## Later

- Creature Isle: `CreatureIsle.exe` is SafeDisc 2.51; needs an unprotected EXE or our own
  decryption. Installs to `<TARGETDIR>\CreatureIsle`.

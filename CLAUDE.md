# CLAUDE.md — Black & White in Rust

Hand-over for future sessions. What is done and what is open: **PLAN.md** (keep it current).

## Goal (the user's)

A native port of Black & White (Lionhead 2001, German PC version) in Rust, built like the
user's earlier ports `C:\!Code2\future_rust_cop` and `C:\!Code2\rustschweine`: the game's own
x86 code is recompiled with Cranelift when the game starts, and the Windows/DirectX functions
it calls are implemented in Rust. First the main game; the add-on (Insel der Kreaturen /
Creature Isle) later.

Only code is ever shared; players supply their own game files. **Never commit anything
derived from the EXE or DLLs** (translated code, extracted data). `game_files/`, the CD
images, the patch EXEs and `Patch*/` are git-ignored.

## The user

- Speaks German; explain in plain words, give a recommendation, not a survey.
- Confirm before anything outward-facing or destructive (pushing, deleting).
- No AI attribution lines in commits.

## The game files

In the project root (user's copies):
- `Black & White (Germany).bin/.cue`: main game, one MODE1/2352 data track, ISO 9660 + Joliet.
- `Black & White - Insel der Kreaturen (Germany).bin/.cue`: the add-on, same layout.
- `black_white_patch_v1_20.exe`: official patch, an RTPatch diff (still SafeDisc). Not used.
- `black_white_patch_v1_42.exe`: unofficial patch (Handsome Matt, bwfiles.com, 2019), installed
  by the user into `Patch1.42/` (18 files). Its `runblack.exe` has no SafeDisc.

Both discs hold InstallShield 6 cabinets (`DATA1.HDR`, `DATA1.CAB`, `DATA2.CAB`), not loose
files. The retail `runblack.exe` (SafeDisc 2.10.030) and `CreatureIsle.exe` (SafeDisc 2.51.020)
have encrypted `.text`/`.data`, so they cannot be recompiled; SafeDisc also does not run on
modern Windows.

The recompile target: `game_files/BlackWhite/runblack.exe` from patch 1.42 (8,663,040 bytes,
base 0x400000, entry 0x3CA49F; sections `.text .rdata .data .data1 SELFMOD .rsrc`; the
`SELFMOD` section suggests self-modifying code). Imports WSOCK32, KERNEL32, USER32, GDI32,
comdlg32, ADVAPI32, SHELL32, DDRAW, DINPUT, WINMM, DSETUP, IMM32, ole32, COMCTL32 and the
game's DLLs IFC22, LHLogR, LHaudiodllR, LHMultiplayerR, LHDialogLib, wearasr, binkw32; loads
`Plug Ins/LanguageR.dll` and `Plug Ins/ScriptLibraryR.dll`.

## How it works

- `src/iso.rs`: CD images (from future_rust_cop), plus `read_range` for parts of a file.
- `src/iscab.rs`: InstallShield 6 header (file table, file groups) and file data (chunks of
  `[u16 length][raw deflate]`), MD5-checked. Offsets are documented in the code.
- `src/setup.rs`: `install` puts every file of the groups below `<TARGETDIR>` where the
  setup would (files beside the cabs come from the disc); `overlay` copies the patch folder
  over it.
- `cargo run --release --example install -- [image] [patch folder] [out]` (defaults: main disc,
  `Patch1.42`, `game_files/BlackWhite`): 986 files from the disc, 18 from the patch, ~5 s.

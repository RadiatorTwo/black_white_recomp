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
- `src/game.rs`: loads `runblack.exe` and the DLLs (`DLLS`: each at its own base, moved by
  its relocations) as one image; seeds are the entries and the DLLs' exports. `SELFMOD` is
  marked as code (see below).
- Core from rustschweine (2026-11-04), unchanged except names: `image.rs`, `discover.rs`,
  `lift.rs` + `lift/x87.rs`, `rt.rs`, `jit.rs`, `smc.rs`; see rustschweine's CLAUDE.md for how
  they work. `ResId::from_guest` (needs guest memory) is left out until the platform layer
  comes. `BW_COUNT=<file>`: functions count their calls. `jit::Stats::incomplete` lists the
  functions with unsupported instructions.
- `cargo run --release --example install -- [image] [patch folder] [out]` (defaults: main disc,
  `Patch1.42`, `game_files/BlackWhite`): 986 files from the disc, 18 from the patch, ~5 s.

## The code (2026-11-04)

`cargo run --release --example functions -- game_files/BlackWhite [--missing] [--gaps]`:
35,025 functions, 37,591 entries, all compiled; 97 contain instructions the lifter lacks:
- binkw32 (43) and QMixer (20): MMX. The EXE (24, 0x7a1400-0x7aca00 and 0x8a25d0-0x8a30a0):
  SSE. These are CPU-specific paths; `rt_cpuid` reports MMX but no SSE, so either MMX gets
  lifted or CPUID stops reporting it (check whether the game insists on MMX first).
- `mov [eax], es` / `fs` / 16-bit addressing (0x424734, 0x465fe8, 0x4a4a4a, 0x7d1a47,
  0x102495e0): data decoded as code, never run.
- x87 environment (fnsave/frstor/fldenv/fnstenv) in a few runtime functions.

`SELFMOD` (0xfc1000, 0xbed bytes, a read/write section) is hand-written assembler: pixel
converters and span/line drawing. It rewrites itself: `sub_fc1285` stores a pitch into
immediates (handled by `smc.rs` as fields), 0xfc1225 stores strides into displacements, and
the line drawer near 0xfc1b42 writes opcode bytes: `mov byte [0xfc1ba5], 47h/4Fh` (and
0xfc1be1) turns a placeholder byte (0x00 in the file) into `inc edi` / `dec edi`. With the
placeholder, discovery decodes `00 03` as `add [ebx], al` and loses the instruction stream
(warnings for fields 0xfc1776, 0xfc1bcf). Needed: decode with the written opcode, and
future_rust_cop's rewritten-instruction variants (`Smc::variants`, `Lifter::code_is`), which
rustschweine dropped. Called from .text at 0x7dda5b and 0x7de610.

Tools: `examples/dis.rs <folder> <addr|all>` (an address inside a function shows that
function), `examples/linear.rs <folder> <addr> [bytes]` (straight-line disassembly),
`examples/imports.rs <exe or dll>`. `cargo test --release` needs no game files.

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
- `src/platform/` (from rustschweine, 2026-11-04) and `src/main.rs`: see below.
- `cargo run --release --example install -- [image] [patch folder] [out]` (defaults: main disc,
  `Patch1.42`, `game_files/BlackWhite`): 986 files from the disc, 18 from the patch, ~5 s.

## The code (2026-11-04)

`cargo run --release --example functions -- game_files/BlackWhite [--missing] [--gaps]`:
35,025 functions, 37,591 entries, all compiled; 92 contain instructions the lifter lacks,
none on a path that runs:
- binkw32 (42) and QMixer (20): MMX. They test CPUID bit 23; `rt_cpuid` reports a Pentium
  (family 5) without MMX, so they take their plain paths. The EXE uses no MMX itself (its
  `test ..., 800000h` are object flags).
- The EXE (24, 0x7a1400-0x7aca00 and 0x8a25d0-0x8a30a0): SSE, plus `fcmovnb` and
  `ldmxcsr`/`stmxcsr` there. The Intel maths library's dispatcher (0x8a2610, result in
  [0xfac840]) picks them only for family 6/15 with SSE; family 5 gets the Pentium path.
- `mov [eax], es` / `fs` / 16-bit addressing (0x424734, 0x465fe8, 0x4a4a4a, 0x7d1a47,
  0x102495e0): data decoded as code, never run.
If one of them runs after all, the generated code reports the unsupported instruction.
`fnstenv`/`fldenv`/`fnsave`/`frstor` (the C runtime's maths error paths, 0x7c7c12,
0x7d3bf3, and the same in binkw32 and LHaudiodllR) go to `rt_x87` (`tests/x87env.rs`).

`SELFMOD` (0xfc1000, 0xbed bytes, a read/write section) is hand-written assembler: 11
functions (prologues at 0xfc1144, 0xfc120c, 0xfc1225, 0xfc1285, 0xfc12f4, 0xfc145d, 0xfc17a8,
0xfc1a16, 0xfc1a47, 0xfc1a74, 0xfc1b37): pixel converters, span and line drawing. They rewrite
themselves: 0xfc1225 and 0xfc1285 store pitches and strides into immediates and
displacements (`smc.rs` reads those from guest memory), and the line drawer 0xfc1b37 writes
opcode bytes: `mov byte [0xfc1ba5], 47h/4Fh` (and 0xfc1be1) turns a placeholder byte (0x00 in
the file) into `inc edi` / `dec edi` (the only opcode writes: a scan of the raw bytes of
SELFMOD and .text).
Only `sub_fc1285` is reachable: .text calls it at 0x7dda5b and 0x7de610 (it only stores the
pitch into the others' code). No other function is referenced anywhere: no relative call or
jump, no 32-bit address in any section (the EXE's export table, 13 ordinals named
`Black.exe`, has every address 0: emptied, probably by the SafeDisc removal). So the rest is
dead code here, and the rewritten-instruction variants of future_rust_cop are not needed. If
one of them ever runs, it shows as a call to an unknown address (0xfc1b37 is no entry).
Discovery still decodes some of the dead code from its "code address in data" rule, hence the
warnings for fields 0xfc1776 and 0xfc1bcf (harmless).

## Platform layer (`src/platform/`)

Taken from rustschweine (not future_rust_cop: Black & White uses DirectDraw 7 + Direct3D 7
like Hogs of War, whose `d3d.rs` draws a Direct3D 7 HAL device with OpenGL; Future Cop has
Glide and a software renderer). Hogs-only modules are gone (ground, objects, dplay) or
stubs for now (`fps.rs`: original pacing, `wide.rs`: no widescreen, `dialog/extend.rs`:
no own settings). Env variables are `BW_*` (`BW_TRACE_API=1`, `BW_TRACE_REG=1`, ...).

- Modules (`mod.rs`): every DLL of `game::DLLS` is a `Module`; imports between the EXE and
  the DLLs are bound to the DLLs' exports (by name or `#ordinal`), the rest to our functions
  (`find_import`). The DllMains of the DLLs the EXE imports run before its entry, each after
  the DLLs it imports; LoadLibraryA runs the others' (`kernel32.rs`). Not recompiled:
  LHDialogLib.dll (debug MFC; `lionhead.rs` does its two `LHEditBox` functions, which only
  report themselves) and the system DLLs in the game folder.
- Paths: the game sees itself installed in `C:\Black & White` (`INSTALL_DIR`); every path
  maps into the game folder (`host_path`). Registry (`advapi32.rs`): GameDir in
  `HKLM\Software\Lionhead Studios Ltd\Black & White` as the installer writes it (SETUP.INX);
  the game and LHLogR keep the rest under HKCU (`registry.txt` in the game folder).
- Guest memory: the DLLs at 0x10000000-0x10800000 (binkw32 moved there: it wants 0x30000000,
  inside the guest heap 0x20000000-0x70000000), QMixer at 0x18000000.
- Added for Black & White: KERNEL32 (`MORE_IMPORTS`: version, find files, folders, time,
  memory status, ini files, file mappings as copies, ...), USER32 (`MORE_IMPORTS`: strings,
  key names, display settings, capture, ...), `winsock.rs` (no network: sockets fail),
  `small.rs` (IMM32, COMCTL32, SHELL32, VERSION), DSETUP's DirectXSetupGetVersion (7.0a),
  DirectDraw caps with DDCAPS_3D (the game insists), GetObjectA of DIB sections (their bits
  in guest memory), `xlat` in the lifter (the C runtime's maths dispatcher).
- Discovery fix: the code addresses in the operands of what a candidate decoded are taken
  before the next candidate (a word of data pointing into a function came first otherwise:
  the atexit callback 0x6806d0).

How far it runs (2026-11-04): all DLLs start, WinMain runs, DirectDraw/Direct3D 7 are set up
(state blocks, textures), the first BeginScene comes, then audio starts (QMixer: OpenFile,
waveOut are next). `IDirectDrawSurface7::QueryInterface(IID_IDirect3DTexture2)` is asked for
every texture and refused (warnings), apparently without harm.

Tools: `examples/dis.rs <folder> <addr|all>` (an address inside a function shows that
function), `examples/linear.rs <folder> <addr> [bytes]` (straight-line disassembly),
`examples/imports.rs <exe or dll>`, `examples/exports.rs <exe or dll>`. `cargo test --release` needs no game files.

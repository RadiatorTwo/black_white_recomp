//! KERNEL32: process and C runtime start-up, memory, files, threads and synchronisation.

use super::{Import, ResId, alloc, alloc_aligned, arg, block_size, cstr, free, guest_call, host_path, log, mem, new_cstr, put_cstr, realloc, set_last_error};
use crate::rt::Cpu;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

const INVALID_HANDLE: u32 = 0xffff_ffff;
const WAIT_TIMEOUT: u32 = 0x102;
const INFINITE: u32 = 0xffff_ffff;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_ACCESS_DENIED: u32 = 5;

// ---- kernel objects -----------------------------------------------------------------------------

pub enum Obj {
    File(Arc<Mutex<std::fs::File>>),
    /// standard input/output/error (-10, -11, -12)
    Std(u32),
    Event { manual: bool, set: bool },
    Mutex { owner: u32, count: u32 },
    Thread { id: u32, done: bool, code: u32, resume: Arc<(Mutex<bool>, Condvar)> },
    Heap,
    /// FindFirstFileA's entries and the next one to give
    Find { entries: Vec<FindEntry>, next: usize },
    /// CreateFileMappingA of a file
    Mapping(Arc<Mutex<std::fs::File>>),
}

pub struct Objects {
    map: BTreeMap<u32, Obj>,
    next: u32,
    /// critical sections by guest address: owner thread, recursion count
    cs: BTreeMap<u32, (u32, u32)>,
}

static OBJS: Mutex<Objects> = Mutex::new(Objects { map: BTreeMap::new(), next: 0x100, cs: BTreeMap::new() });
/// notified whenever an object becomes signalled
static SIGNAL: Condvar = Condvar::new();

pub fn objects() -> MutexGuard<'static, Objects> {
    OBJS.lock().unwrap_or_else(|e| e.into_inner())
}

impl Objects {
    pub fn add(&mut self, o: Obj) -> u32 {
        self.next += 4;
        self.map.insert(self.next, o);
        self.next
    }
}

fn wait_signal(g: MutexGuard<'static, Objects>, until: Option<Instant>) -> (MutexGuard<'static, Objects>, bool) {
    match until {
        None => (SIGNAL.wait(g).unwrap_or_else(|e| e.into_inner()), false),
        Some(t) => {
            let now = Instant::now();
            if now >= t {
                return (g, true);
            }
            let (g, r) = SIGNAL.wait_timeout(g, t - now).unwrap_or_else(|e| e.into_inner());
            (g, r.timed_out() && Instant::now() >= t)
        }
    }
}

static NEXT_TID: AtomicU32 = AtomicU32::new(1);
thread_local! {
    static TID: u32 = NEXT_TID.fetch_add(1, Ordering::Relaxed);
    static TLS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static EXIT_CODE: Cell<u32> = const { Cell::new(0) };
}

pub fn thread_id() -> u32 {
    TID.with(|t| *t)
}

// ---- process ------------------------------------------------------------------------------------

fn get_version(_: &mut Cpu) -> u32 {
    0xc000_0a04 // Windows 98
}

fn get_command_line(_: &mut Cpu) -> u32 {
    static P: OnceLock<u32> = OnceLock::new();
    *P.get_or_init(|| new_cstr(EXE_PATH))
}

fn get_startup_info(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    mem().slice(p, 68).fill(0);
    mem().wr32(p, 68);
    0
}

fn get_module_handle(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    if p == 0 {
        return super::IMAGE_BASE;
    }
    let name = cstr(p);
    if let Some(m) = super::module(&name).filter(|m| super::attached(m)) {
        return m.base;
    }
    super::system_module(&name).unwrap_or(0)
}

/// The installed game (see `host_path`).
pub const EXE_PATH: &str = "C:\\Black & White\\runblack.exe";

fn get_module_file_name(c: &mut Cpu) -> u32 {
    let (module, buf, n) = (arg(c, 0), arg(c, 1), arg(c, 2));
    match super::module_at(module) {
        Some(m) => put_cstr(buf, &m.guest_path(), n),
        None => put_cstr(buf, EXE_PATH, n),
    }
}

fn get_environment_strings(_: &mut Cpu) -> u32 {
    alloc(4) // empty block: two NULs (also as UTF-16)
}

fn exit_process(c: &mut Cpu) -> u32 {
    let code = arg(c, 0);
    super::advapi32::save();
    log!("ExitProcess({code})");
    std::process::exit(code as i32)
}

fn exit_thread(c: &mut Cpu) -> u32 {
    EXIT_CODE.set(arg(c, 0));
    c.stop = 1;
    0
}

fn get_system_info(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    let m = mem();
    m.slice(p, 36).fill(0);
    m.wr32(p + 4, 0x1000); // page size
    m.wr32(p + 8, 0x10000);
    m.wr32(p + 12, 0x7ffe_ffff);
    m.wr32(p + 16, 1);
    m.wr32(p + 20, 1); // processors
    m.wr32(p + 24, 586); // Pentium
    m.wr32(p + 28, 0x10000);
    0
}

fn get_computer_name(c: &mut Cpu) -> u32 {
    let (buf, len) = (arg(c, 0), arg(c, 1));
    let n = mem().rd32(len);
    let k = put_cstr(buf, "FCOP", n);
    mem().wr32(len, k);
    1
}

fn output_debug_string(c: &mut Cpu) -> u32 {
    log!("debug: {}", cstr(arg(c, 0)).trim_end());
    0
}

fn raise_exception(c: &mut Cpu) -> u32 {
    log!("RaiseException {:#x}", arg(c, 0));
    std::process::exit(102)
}

// ---- drives (the CD check) ----------------------------------------------------------------------

fn get_logical_drives(_: &mut Cpu) -> u32 {
    1 << 2 | 1 << 3 // C: and D:
}

fn get_drive_type(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    match if p == 0 { b'C' } else { mem().rd8(p).to_ascii_uppercase() } {
        b'C' => 3, // fixed
        b'D' => 5, // CD-ROM
        _ => 1,    // no root directory
    }
}

fn get_volume_information(c: &mut Cpu) -> u32 {
    let root = arg(c, 0);
    let cd = root != 0 && mem().rd8(root).eq_ignore_ascii_case(&b'D');
    let m = mem();
    put_cstr(arg(c, 1), if cd { "BW_GERMAN" } else { "SYSTEM" }, arg(c, 2));
    for (k, v) in [(3, 0x1998_0001u32), (4, 255), (5, 0)] {
        if arg(c, k) != 0 {
            m.wr32(arg(c, k), v);
        }
    }
    put_cstr(arg(c, 6), if cd { "CDFS" } else { "FAT32" }, arg(c, 7));
    1
}

// ---- strings and code pages ---------------------------------------------------------------------

fn get_acp(_: &mut Cpu) -> u32 {
    1252
}
fn get_oemcp(_: &mut Cpu) -> u32 {
    437
}

fn get_cp_info(c: &mut Cpu) -> u32 {
    let p = arg(c, 1);
    let m = mem();
    m.slice(p, 20).fill(0);
    m.wr32(p, 1); // single-byte
    m.wr8(p + 4, b'?');
    1
}

/// Length of a string argument given as (pointer, count), count -1 meaning NUL-terminated
/// including the NUL.
fn str_len(p: u32, n: u32, wide: bool) -> u32 {
    if n != 0xffff_ffff {
        return n;
    }
    let m = mem();
    let mut k = 0;
    while if wide { m.rd16(p + 2 * k) != 0 } else { m.rd8(p + k) != 0 } {
        k += 1;
    }
    k + 1
}

fn multi_byte_to_wide_char(c: &mut Cpu) -> u32 {
    let (src, n, dst, dn) = (arg(c, 2), arg(c, 3), arg(c, 4), arg(c, 5));
    let n = str_len(src, n, false);
    if dn == 0 {
        return n;
    }
    let m = mem();
    let k = n.min(dn);
    for i in 0..k {
        m.wr16(dst + 2 * i, m.rd8(src + i) as u16);
    }
    k
}

fn wide_char_to_multi_byte(c: &mut Cpu) -> u32 {
    let (src, n, dst, dn) = (arg(c, 2), arg(c, 3), arg(c, 4), arg(c, 5));
    let n = str_len(src, n, true);
    if dn == 0 {
        return n;
    }
    let m = mem();
    let k = n.min(dn);
    for i in 0..k {
        let w = m.rd16(src + 2 * i);
        m.wr8(dst + i, if w < 256 { w as u8 } else { b'?' });
    }
    k
}

fn ctype(ch: u16) -> u16 {
    let Ok(b) = u8::try_from(ch) else { return 0x100 };
    let c = b as char;
    let mut t = 0;
    if c.is_ascii_uppercase() || (0xc0..=0xde).contains(&b) && b != 0xd7 {
        t |= 0x101;
    }
    if c.is_ascii_lowercase() || (0xdf..=0xff).contains(&b) && b != 0xf7 {
        t |= 0x102;
    }
    if c.is_ascii_digit() {
        t |= 4;
    }
    if matches!(b, 9..=13 | 32) {
        t |= 8;
    }
    if c.is_ascii_punctuation() {
        t |= 0x10;
    }
    if b < 32 || b == 127 {
        t |= 0x20;
    }
    if b == 9 || b == 32 {
        t |= 0x40;
    }
    if c.is_ascii_hexdigit() {
        t |= 0x80;
    }
    t
}

fn get_string_type_a(c: &mut Cpu) -> u32 {
    let (src, n, out) = (arg(c, 2), arg(c, 3), arg(c, 4));
    let n = str_len(src, n, false);
    for i in 0..n {
        mem().wr16(out + 2 * i, ctype(mem().rd8(src + i) as u16));
    }
    1
}

fn get_string_type_w(c: &mut Cpu) -> u32 {
    let (src, n, out) = (arg(c, 1), arg(c, 2), arg(c, 3));
    let n = str_len(src, n, true);
    for i in 0..n {
        mem().wr16(out + 2 * i, ctype(mem().rd16(src + 2 * i)));
    }
    1
}

fn lc_map(c: &mut Cpu, wide: bool) -> u32 {
    let (flags, src, n, dst, dn) = (arg(c, 1), arg(c, 2), arg(c, 3), arg(c, 4), arg(c, 5));
    let n = str_len(src, n, wide);
    if dn == 0 {
        return n;
    }
    let m = mem();
    let k = n.min(dn);
    for i in 0..k {
        let v = if wide { m.rd16(src + 2 * i) as u8 } else { m.rd8(src + i) };
        let v = match flags & 0x300 {
            0x100 => v.to_ascii_lowercase(),
            0x200 => v.to_ascii_uppercase(),
            _ => v,
        };
        if wide { m.wr16(dst + 2 * i, v as u16) } else { m.wr8(dst + i, v) }
    }
    k
}
fn lc_map_string_a(c: &mut Cpu) -> u32 {
    lc_map(c, false)
}
fn lc_map_string_w(c: &mut Cpu) -> u32 {
    lc_map(c, true)
}

fn compare(c: &mut Cpu, wide: bool) -> u32 {
    let (flags, a, an, b, bn) = (arg(c, 1), arg(c, 2), arg(c, 3), arg(c, 4), arg(c, 5));
    let get = |p: u32, n: u32| -> Vec<u8> {
        let n = str_len(p, n, wide);
        let mut v: Vec<u8> = (0..n).map(|i| if wide { mem().rd16(p + 2 * i) as u8 } else { mem().rd8(p + i) }).collect();
        while v.last() == Some(&0) {
            v.pop();
        }
        if flags & 1 != 0 {
            v.make_ascii_lowercase();
        }
        v
    };
    match get(a, an).cmp(&get(b, bn)) {
        std::cmp::Ordering::Less => 1,
        std::cmp::Ordering::Equal => 2,
        std::cmp::Ordering::Greater => 3,
    }
}
fn compare_string_a(c: &mut Cpu) -> u32 {
    compare(c, false)
}
fn compare_string_w(c: &mut Cpu) -> u32 {
    compare(c, true)
}

fn locale_info(kind: u32) -> &'static str {
    match kind & 0xffff {
        0x1004 => "1252", // LOCALE_IDEFAULTANSICODEPAGE
        0xb => "437",     // LOCALE_IDEFAULTCODEPAGE
        0x1 => "0409",    // LOCALE_ILANGUAGE
        0x3 => "ENU",
        0x7 => "USA",
        0xe => ".",
        0xf => ",",
        _ => "",
    }
}

fn get_locale_info_a(c: &mut Cpu) -> u32 {
    let (kind, buf, n) = (arg(c, 1), arg(c, 2), arg(c, 3));
    let s = locale_info(kind);
    if n == 0 { s.len() as u32 + 1 } else { put_cstr(buf, s, n) + 1 }
}

fn get_locale_info_w(c: &mut Cpu) -> u32 {
    let (kind, buf, n) = (arg(c, 1), arg(c, 2), arg(c, 3));
    let s = locale_info(kind);
    if n == 0 {
        return s.len() as u32 + 1;
    }
    let k = (s.len() as u32).min(n - 1);
    for (i, b) in s.bytes().take(k as usize).enumerate() {
        mem().wr16(buf + 2 * i as u32, b as u16);
    }
    mem().wr16(buf + 2 * k, 0);
    k + 1
}

fn lstrlen(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    if p == 0 { 0 } else { str_len(p, 0xffff_ffff, false) - 1 }
}

fn lstrcpy(c: &mut Cpu) -> u32 {
    let (d, s) = (arg(c, 0), arg(c, 1));
    let n = str_len(s, 0xffff_ffff, false);
    let b = mem().slice(s, n as usize).to_vec();
    mem().slice(d, n as usize).copy_from_slice(&b);
    d
}

// ---- memory -------------------------------------------------------------------------------------

fn heap_create(_: &mut Cpu) -> u32 {
    objects().add(Obj::Heap)
}

fn heap_alloc(c: &mut Cpu) -> u32 {
    alloc(arg(c, 2))
}

fn heap_free(c: &mut Cpu) -> u32 {
    free(arg(c, 2)) as u32
}

fn heap_realloc(c: &mut Cpu) -> u32 {
    // HEAP_REALLOC_IN_PLACE_ONLY: only when it fits already
    let (flags, p, n) = (arg(c, 1), arg(c, 2), arg(c, 3));
    if flags & 0x10 != 0 {
        return if block_size(p) >= n { p } else { 0 };
    }
    realloc(p, n)
}

fn heap_size(c: &mut Cpu) -> u32 {
    let p = arg(c, 2);
    match block_size(p) {
        0 => 0xffff_ffff,
        n => n,
    }
}

fn virtual_alloc(c: &mut Cpu) -> u32 {
    let (p, n) = (arg(c, 0), arg(c, 1));
    if p != 0 {
        // committing pages of an earlier reservation: ours are committed already
        return p;
    }
    alloc_aligned(n, 0x1000)
}

fn virtual_free(c: &mut Cpu) -> u32 {
    let (p, kind) = (arg(c, 0), arg(c, 2));
    if kind & 0x8000 != 0 {
        free(p);
    }
    1
}

fn virtual_protect(c: &mut Cpu) -> u32 {
    let old = arg(c, 3);
    if old != 0 {
        mem().wr32(old, 4); // PAGE_READWRITE
    }
    1
}

fn global_alloc(c: &mut Cpu) -> u32 {
    alloc(arg(c, 1))
}
fn global_free(c: &mut Cpu) -> u32 {
    free(arg(c, 0));
    0
}
/// GlobalLock, GlobalUnlock, GlobalHandle: our handles are the memory itself
fn global_same(c: &mut Cpu) -> u32 {
    arg(c, 0)
}
fn global_unlock(_: &mut Cpu) -> u32 {
    0
}

// ---- thread-local storage, critical sections, interlocked ---------------------------------------

static TLS_NEXT: AtomicU32 = AtomicU32::new(0);

fn tls_alloc(_: &mut Cpu) -> u32 {
    TLS_NEXT.fetch_add(1, Ordering::Relaxed)
}

fn tls_get_value(c: &mut Cpu) -> u32 {
    let k = arg(c, 0) as usize;
    set_last_error(0);
    TLS.with(|t| t.borrow().get(k).copied().unwrap_or(0))
}

fn tls_set_value(c: &mut Cpu) -> u32 {
    let (k, v) = (arg(c, 0) as usize, arg(c, 1));
    TLS.with(|t| {
        let mut t = t.borrow_mut();
        if t.len() <= k {
            t.resize(k + 1, 0);
        }
        t[k] = v;
    });
    1
}

fn init_cs(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    mem().slice(p, 24).fill(0);
    objects().cs.insert(p, (0, 0));
    0
}

fn delete_cs(c: &mut Cpu) -> u32 {
    objects().cs.remove(&arg(c, 0));
    0
}

fn enter_cs(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    let me = thread_id();
    let mut g = objects();
    loop {
        let e = g.cs.entry(p).or_insert((0, 0));
        if e.0 == 0 || e.0 == me {
            e.0 = me;
            e.1 += 1;
            return 0;
        }
        g = wait_signal(g, None).0;
    }
}

fn leave_cs(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    let mut g = objects();
    if let Some(e) = g.cs.get_mut(&p) {
        e.1 = e.1.saturating_sub(1);
        if e.1 == 0 {
            e.0 = 0;
            SIGNAL.notify_all();
        }
    }
    0
}

fn interlocked(c: &mut Cpu, d: u32) -> u32 {
    let p = arg(c, 0);
    let _g = objects();
    let v = mem().rd32(p).wrapping_add(d);
    mem().wr32(p, v);
    v
}
fn interlocked_increment(c: &mut Cpu) -> u32 {
    interlocked(c, 1)
}
fn interlocked_decrement(c: &mut Cpu) -> u32 {
    interlocked(c, u32::MAX)
}

// ---- events, mutexes, waiting -------------------------------------------------------------------

fn create_event(c: &mut Cpu) -> u32 {
    objects().add(Obj::Event { manual: arg(c, 1) != 0, set: arg(c, 2) != 0 })
}

/// Set or reset event `h` (also from host threads: `dplay.rs`); 0 if it is no event.
pub fn set_event_state(h: u32, on: bool) -> u32 {
    let mut g = objects();
    match g.map.get_mut(&h) {
        Some(Obj::Event { set, .. }) => {
            *set = on;
            SIGNAL.notify_all();
            1
        }
        _ => 0,
    }
}
fn set_event(c: &mut Cpu) -> u32 {
    set_event_state(arg(c, 0), true)
}
fn reset_event(c: &mut Cpu) -> u32 {
    set_event_state(arg(c, 0), false)
}
fn pulse_event(c: &mut Cpu) -> u32 {
    // wakes the waiters there are now: approximated by a short signalled moment
    let h = arg(c, 0);
    set_event_state(h, true);
    std::thread::yield_now();
    set_event_state(h, false)
}

fn create_mutex(c: &mut Cpu) -> u32 {
    let owner = if arg(c, 1) != 0 { thread_id() } else { 0 };
    objects().add(Obj::Mutex { owner, count: (owner != 0) as u32 })
}

/// Whether `h` is signalled for this thread; with `take`, also what a successful wait does to it.
fn signalled(g: &mut Objects, h: u32, take: bool) -> bool {
    let me = thread_id();
    match g.map.get_mut(&h) {
        Some(Obj::Event { manual, set }) => {
            let s = *set;
            if s && take && !*manual {
                *set = false;
            }
            s
        }
        Some(Obj::Mutex { owner, count }) => {
            let s = *owner == 0 || *owner == me;
            if s && take {
                *owner = me;
                *count += 1;
            }
            s
        }
        Some(Obj::Thread { done, .. }) => *done,
        _ => true,
    }
}

fn wait(handles: &[u32], all: bool, ms: u32) -> u32 {
    let until = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
    let mut g = objects();
    loop {
        if all {
            if handles.iter().all(|&h| signalled(&mut g, h, false)) {
                for &h in handles {
                    signalled(&mut g, h, true);
                }
                return 0;
            }
        } else if let Some(k) = handles.iter().position(|&h| signalled(&mut g, h, false)) {
            signalled(&mut g, handles[k], true);
            return k as u32;
        }
        let (ng, timed_out) = wait_signal(g, until);
        g = ng;
        if timed_out {
            return WAIT_TIMEOUT;
        }
    }
}

fn wait_for_single_object(c: &mut Cpu) -> u32 {
    wait(&[arg(c, 0)], false, arg(c, 1))
}

fn wait_for_multiple_objects(c: &mut Cpu) -> u32 {
    let (n, p, all, ms) = (arg(c, 0), arg(c, 1), arg(c, 2), arg(c, 3));
    let hs: Vec<u32> = (0..n).map(|k| mem().rd32(p + 4 * k)).collect();
    wait(&hs, all != 0, ms)
}

fn close_handle(c: &mut Cpu) -> u32 {
    let h = arg(c, 0);
    let mut g = objects();
    // a thread handle stays until the thread ends (it only marks itself done)
    if matches!(g.map.get(&h), Some(Obj::Thread { done: false, .. })) {
        return 1;
    }
    if g.map.remove(&h).is_some() {
        1
    } else {
        set_last_error(ERROR_INVALID_HANDLE);
        0
    }
}

fn duplicate_handle(c: &mut Cpu) -> u32 {
    let (src, out) = (arg(c, 1), arg(c, 3));
    let h = if src == 0xffff_fffe { thread_handle_of(thread_id()) } else { src };
    if out != 0 {
        mem().wr32(out, h);
    }
    1
}

// ---- threads ------------------------------------------------------------------------------------

fn thread_handle_of(id: u32) -> u32 {
    let g = objects();
    g.map.iter().find(|(_, o)| matches!(o, Obj::Thread { id: i, .. } if *i == id)).map(|(&h, _)| h).unwrap_or(0xffff_fffe)
}

fn create_thread(c: &mut Cpu) -> u32 {
    let (start, param, flags, id_out) = (arg(c, 2), arg(c, 3), arg(c, 4), arg(c, 5));
    let resume = Arc::new((Mutex::new(flags & 4 == 0), Condvar::new()));
    let r2 = resume.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let id = thread_id();
            tx.send(id).unwrap();
            {
                let (m, cv) = &*r2;
                let mut go = m.lock().unwrap();
                while !*go {
                    go = cv.wait(go).unwrap();
                }
            }
            let mut tc = super::new_thread_cpu();
            let r = guest_call(&mut tc, start, &[param]);
            let code = if tc.stop != 0 { EXIT_CODE.get() } else { r };
            let mut g = objects();
            for o in g.map.values_mut() {
                if let Obj::Thread { id: i, done, code: c, .. } = o {
                    if *i == id {
                        *done = true;
                        *c = code;
                    }
                }
            }
            SIGNAL.notify_all();
        })
        .expect("cannot start a thread");
    let id = rx.recv().unwrap();
    if id_out != 0 {
        mem().wr32(id_out, id);
    }
    objects().add(Obj::Thread { id, done: false, code: 0, resume })
}

fn resume_thread(c: &mut Cpu) -> u32 {
    let g = objects();
    if let Some(Obj::Thread { resume, .. }) = g.map.get(&arg(c, 0)) {
        let (m, cv) = &**resume;
        let was = !*m.lock().unwrap();
        *m.lock().unwrap() = true;
        cv.notify_all();
        return was as u32;
    }
    0xffff_ffff
}

fn terminate_thread(c: &mut Cpu) -> u32 {
    log!("warn: TerminateThread({:#x}) ignored", arg(c, 0));
    1
}

fn get_current_thread_id(_: &mut Cpu) -> u32 {
    thread_id()
}

/// The one process's id (GetWindowThreadProcessId gives the same).
pub const PROCESS_ID: u32 = 0x1000;

// ---- time ---------------------------------------------------------------------------------------

pub fn start_time() -> Instant {
    static T: OnceLock<Instant> = OnceLock::new();
    *T.get_or_init(Instant::now)
}

pub fn ms_since_start() -> u32 {
    start_time().elapsed().as_millis() as u32
}

fn get_tick_count(_: &mut Cpu) -> u32 {
    ms_since_start()
}

fn query_performance_frequency(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    mem().wr32(p, 1_000_000);
    mem().wr32(p + 4, 0);
    1
}

fn query_performance_counter(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    let t = start_time().elapsed().as_micros() as u64;
    mem().wr32(p, t as u32);
    mem().wr32(p + 4, (t >> 32) as u32);
    1
}

fn sleep(c: &mut Cpu) -> u32 {
    let ms = arg(c, 0);
    if ms == 0 { std::thread::yield_now() } else { std::thread::sleep(Duration::from_millis(ms as u64)) }
    0
}

// ---- files --------------------------------------------------------------------------------------

fn create_file(c: &mut Cpu) -> u32 {
    let (name, access, disp) = (cstr(arg(c, 0)), arg(c, 1), arg(c, 4));
    let path = host_path(&name);
    let write = access & 0x4000_0000 != 0;
    if write && super::on_cd(&name) {
        set_last_error(ERROR_ACCESS_DENIED);
        return INVALID_HANDLE;
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(access & 0x8000_0000 != 0 || !write).write(write);
    match disp {
        1 => o.create_new(true),
        2 => o.create(true).truncate(true),
        4 => o.create(true),
        5 => o.truncate(true),
        _ => &mut o,
    };
    match o.open(&path) {
        Ok(f) => {
            if super::trace_api() {
                log!("CreateFileA {name} (access {access:#x}, disposition {disp})");
            }
            objects().add(Obj::File(Arc::new(Mutex::new(f))))
        }
        Err(e) => {
            if super::trace_api() {
                log!("CreateFileA {name}: {e}");
            }
            set_last_error(if e.kind() == std::io::ErrorKind::AlreadyExists { 80 } else { ERROR_FILE_NOT_FOUND });
            INVALID_HANDLE
        }
    }
}

fn file_of(h: u32) -> Option<Arc<Mutex<std::fs::File>>> {
    match objects().map.get(&h) {
        Some(Obj::File(f)) => Some(f.clone()),
        _ => None,
    }
}

fn read_file(c: &mut Cpu) -> u32 {
    let (h, buf, n, done) = (arg(c, 0), arg(c, 1), arg(c, 2), arg(c, 3));
    let Some(f) = file_of(h) else {
        set_last_error(ERROR_INVALID_HANDLE);
        return 0;
    };
    let dst = mem().slice(buf, n as usize);
    let mut f = f.lock().unwrap();
    let mut got = 0;
    while got < dst.len() {
        match f.read(&mut dst[got..]) {
            Ok(0) => break,
            Ok(k) => got += k,
            Err(_) => return 0,
        }
    }
    if done != 0 {
        mem().wr32(done, got as u32);
    }
    1
}

fn write_file(c: &mut Cpu) -> u32 {
    let (h, buf, n, done) = (arg(c, 0), arg(c, 1), arg(c, 2), arg(c, 3));
    let data = mem().slice(buf, n as usize).to_vec();
    let ok = match objects().map.get(&h) {
        Some(Obj::Std(_)) => {
            eprint!("{}", String::from_utf8_lossy(&data));
            true
        }
        Some(Obj::File(f)) => f.lock().unwrap().write_all(&data).is_ok(),
        _ => false,
    };
    if done != 0 {
        mem().wr32(done, if ok { n } else { 0 });
    }
    ok as u32
}

fn set_file_pointer(c: &mut Cpu) -> u32 {
    let (h, lo, hi_p, how) = (arg(c, 0), arg(c, 1), arg(c, 2), arg(c, 3));
    let Some(f) = file_of(h) else {
        set_last_error(ERROR_INVALID_HANDLE);
        return 0xffff_ffff;
    };
    let d = if hi_p != 0 { ((mem().rd32(hi_p) as u64) << 32 | lo as u64) as i64 } else { lo as i32 as i64 };
    let pos = match how {
        0 => SeekFrom::Start(d as u64),
        1 => SeekFrom::Current(d),
        _ => SeekFrom::End(d),
    };
    match f.lock().unwrap().seek(pos) {
        Ok(p) => {
            if hi_p != 0 {
                mem().wr32(hi_p, (p >> 32) as u32);
            }
            p as u32
        }
        Err(_) => {
            set_last_error(131);
            0xffff_ffff
        }
    }
}

fn set_end_of_file(c: &mut Cpu) -> u32 {
    let Some(f) = file_of(arg(c, 0)) else { return 0 };
    let mut f = f.lock().unwrap();
    let Ok(p) = f.stream_position() else { return 0 };
    f.set_len(p).is_ok() as u32
}

fn flush_file_buffers(c: &mut Cpu) -> u32 {
    file_of(arg(c, 0)).map_or(0, |f| f.lock().unwrap().flush().is_ok() as u32)
}

fn get_std_handle(c: &mut Cpu) -> u32 {
    let n = arg(c, 0);
    let mut g = objects();
    if let Some((&h, _)) = g.map.iter().find(|(_, o)| matches!(o, Obj::Std(k) if *k == n)) {
        return h;
    }
    g.add(Obj::Std(n))
}

fn get_file_type(c: &mut Cpu) -> u32 {
    match objects().map.get(&arg(c, 0)) {
        Some(Obj::File(_)) => 1,
        Some(Obj::Std(_)) => 2,
        _ => 0,
    }
}

// ---- simple ones --------------------------------------------------------------------------------

fn ret0(_: &mut Cpu) -> u32 {
    0
}
fn ret1(_: &mut Cpu) -> u32 {
    1
}
fn get_last_error(_: &mut Cpu) -> u32 {
    super::LAST_ERROR.get()
}
fn set_last_error_imp(c: &mut Cpu) -> u32 {
    set_last_error(arg(c, 0));
    0
}
fn get_current_process(_: &mut Cpu) -> u32 {
    0xffff_ffff
}
fn get_current_thread(_: &mut Cpu) -> u32 {
    0xffff_fffe
}
fn first_arg(c: &mut Cpu) -> u32 {
    arg(c, 0)
}
/// LoadLibraryA: the game's DLLs (recompiled with the EXE, their DllMain run now if not yet)
/// and the system DLLs we implement.
fn load_library(c: &mut Cpu) -> u32 {
    let name = cstr(arg(c, 0));
    if let Some(m) = super::module(&name) {
        if !super::attach(c, m) {
            return 0;
        }
        return m.base;
    }
    if let Some(h) = super::system_module(&name) {
        return h;
    }
    log!("LoadLibraryA({name}): not available");
    set_last_error(126);
    0
}

fn get_proc_address(c: &mut Cpu) -> u32 {
    let (module, name) = (arg(c, 0), arg(c, 1));
    let name = if name > 0xffff { cstr(name) } else { format!("#{name}") };
    if let Some(m) = super::module_at(module) {
        let a = m.export(&name);
        if a == 0 {
            log!("GetProcAddress({}, {name}): no such export", m.file());
        }
        return a;
    }
    let a = super::system_proc(module, &name);
    if a != 0 {
        return a;
    }
    log!("GetProcAddress({module:#x}, {name}): not available");
    0
}

// ---- resources ----------------------------------------------------------------------------------

/// FindResourceA(module, name, type): a guest record of the data's address and size.
fn find_resource(c: &mut Cpu) -> u32 {
    let base = if arg(c, 0) == 0 { super::IMAGE_BASE } else { arg(c, 0) };
    let (name, ty) = (ResId::from_guest(arg(c, 1)), ResId::from_guest(arg(c, 2)));
    match super::resource_in(base, ty, name) {
        Some((a, n)) => {
            let r = alloc(8);
            mem().wr32(r, a);
            mem().wr32(r + 4, n);
            r
        }
        None => {
            log!("FindResourceA({name:?}, {ty:?}): not found");
            set_last_error(1814); // ERROR_RESOURCE_NAME_NOT_FOUND
            0
        }
    }
}
fn load_resource(c: &mut Cpu) -> u32 {
    let r = arg(c, 1);
    if r == 0 { 0 } else { mem().rd32(r) }
}

// ---- old 16-bit style files (_lopen) --------------------------------------------------------------

fn lopen(c: &mut Cpu) -> u32 {
    let (name, mode) = (cstr(arg(c, 0)), arg(c, 1) & 3);
    if mode != 0 && super::on_cd(&name) {
        set_last_error(ERROR_ACCESS_DENIED);
        return INVALID_HANDLE;
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(mode != 1).write(mode != 0);
    match o.open(host_path(&name)) {
        Ok(f) => objects().add(Obj::File(Arc::new(Mutex::new(f)))),
        Err(_) => {
            if super::trace_api() {
                log!("_lopen {name}: not found");
            }
            set_last_error(ERROR_FILE_NOT_FOUND);
            INVALID_HANDLE
        }
    }
}
fn lread(c: &mut Cpu) -> u32 {
    let (h, buf, n) = (arg(c, 0), arg(c, 1), arg(c, 2));
    let Some(f) = file_of(h) else { return INVALID_HANDLE };
    let dst = mem().slice(buf, n as usize);
    let mut f = f.lock().unwrap();
    let mut got = 0;
    while got < dst.len() {
        match f.read(&mut dst[got..]) {
            Ok(0) => break,
            Ok(k) => got += k,
            Err(_) => return INVALID_HANDLE,
        }
    }
    got as u32
}
fn lclose(c: &mut Cpu) -> u32 {
    objects().map.remove(&arg(c, 0));
    0
}

fn get_file_size(c: &mut Cpu) -> u32 {
    let (h, hi) = (arg(c, 0), arg(c, 1));
    let Some(f) = file_of(h) else {
        set_last_error(ERROR_INVALID_HANDLE);
        return INVALID_HANDLE;
    };
    let n = f.lock().unwrap().metadata().map_or(0, |m| m.len());
    if hi != 0 {
        mem().wr32(hi, (n >> 32) as u32);
    }
    n as u32
}
fn delete_file(c: &mut Cpu) -> u32 {
    let name = cstr(arg(c, 0));
    if !super::on_cd(&name) && std::fs::remove_file(host_path(&name)).is_ok() {
        1
    } else {
        set_last_error(ERROR_FILE_NOT_FOUND);
        0
    }
}
fn set_current_directory(c: &mut Cpu) -> u32 {
    if super::trace_api() {
        log!("SetCurrentDirectoryA {}", cstr(arg(c, 0)));
    }
    1
}

// ---- misc ---------------------------------------------------------------------------------------

fn interlocked_exchange(c: &mut Cpu) -> u32 {
    let (p, v) = (arg(c, 0), arg(c, 1));
    let old = mem().rd32(p);
    mem().wr32(p, v);
    old
}
fn get_time_zone_information(c: &mut Cpu) -> u32 {
    mem().slice(arg(c, 0), 172).fill(0);
    0 // TIME_ZONE_ID_UNKNOWN: UTC
}
/// Now in UTC as a SYSTEMTIME: year, month, weekday, day, hour, minute, second, ms.
fn utc_now() -> [u16; 8] {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let (secs, ms) = (d.as_secs(), d.subsec_millis());
    let days = (secs / 86400) as i64;
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    let weekday = (days + 4).rem_euclid(7); // 1970-01-01 was a Thursday
    let s = secs % 86400;
    [year, month, weekday, day, (s / 3600) as i64, (s / 60 % 60) as i64, (s % 60) as i64, ms as i64].map(|v| v as u16)
}

/// GetLocalTime: the time in UTC (as the time zone says).
fn get_local_time(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    for (k, v) in utc_now().into_iter().enumerate() {
        mem().wr16(p + 2 * k as u32, v);
    }
    0
}

// ---- more for Black & White ---------------------------------------------------------------------

/// GetVersionExA(OSVERSIONINFOA*): Windows 98 (as GetVersion).
fn get_version_ex(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    let m = mem();
    let size = m.rd32(p).clamp(148, 156);
    m.slice(p + 4, size as usize - 4).fill(0);
    m.wr32(p + 4, 4); // major
    m.wr32(p + 8, 10); // minor
    m.wr32(p + 12, 0x040a_0b6c); // build 2924 (98 SE) with the version in the high word
    m.wr32(p + 16, 1); // VER_PLATFORM_WIN32_WINDOWS
    put_cstr(p + 20, " A ", 128);
    1
}

/// LocalAlloc(flags, size): LMEM_ZEROINIT (0x40) zeroes; our handles are the memory.
fn local_alloc(c: &mut Cpu) -> u32 {
    let (flags, size) = (arg(c, 0), arg(c, 1));
    let p = alloc(size);
    if flags & 0x40 != 0 && p != 0 {
        mem().slice(p, size as usize).fill(0);
    }
    p
}
fn local_free(c: &mut Cpu) -> u32 {
    free(arg(c, 0));
    0
}

/// GetEnvironmentVariableA: there are none.
fn get_environment_variable(_: &mut Cpu) -> u32 {
    set_last_error(203); // ERROR_ENVVAR_NOT_FOUND
    0
}

fn attributes(md: &std::fs::Metadata) -> u32 {
    let mut a = if md.is_dir() { 0x10 } else { 0 };
    if md.permissions().readonly() {
        a |= 1;
    }
    if a == 0 { 0x80 } else { a }
}

fn get_file_attributes(c: &mut Cpu) -> u32 {
    let name = cstr(arg(c, 0));
    match std::fs::metadata(host_path(&name)) {
        Ok(md) => attributes(&md),
        Err(_) => {
            set_last_error(ERROR_FILE_NOT_FOUND);
            INVALID_HANDLE
        }
    }
}
fn set_file_attributes(c: &mut Cpu) -> u32 {
    let (name, a) = (cstr(arg(c, 0)), arg(c, 1));
    let p = host_path(&name);
    let Ok(md) = std::fs::metadata(&p) else {
        set_last_error(ERROR_FILE_NOT_FOUND);
        return 0;
    };
    let mut perm = md.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(a & 1 != 0);
    std::fs::set_permissions(&p, perm).is_ok() as u32
}

/// `*` and `?` as in Windows file names, any case.
fn wildcard(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.to_lowercase().chars().collect(), name.to_lowercase().chars().collect());
    // "*.*" is everything, also names without a dot
    if pattern == "*.*" {
        return true;
    }
    fn go(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|k| go(rest, &n[k..])),
            Some(('?', rest)) => !n.is_empty() && go(rest, &n[1..]),
            Some((ch, rest)) => n.first() == Some(ch) && go(rest, &n[1..]),
        }
    }
    go(&p, &n)
}

/// A file time (100 ns since 1601) of a host time.
fn file_time(t: std::time::SystemTime) -> u64 {
    let unix = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    116_444_736_000_000_000 + unix.as_nanos() as u64 / 100
}

/// WIN32_FIND_DATAA (320 bytes) for one entry.
fn write_find_data(p: u32, e: &FindEntry) {
    let m = mem();
    m.slice(p, 320).fill(0);
    m.wr32(p, e.attributes);
    for o in [4, 12, 20] {
        m.wr32(p + o, e.time as u32);
        m.wr32(p + o + 4, (e.time >> 32) as u32);
    }
    m.wr32(p + 28, (e.size >> 32) as u32);
    m.wr32(p + 32, e.size as u32);
    put_cstr(p + 44, &e.name, 260);
    let short: String = if e.name.len() <= 12 { e.name.to_ascii_uppercase() } else { String::new() };
    put_cstr(p + 304, &short, 14);
}

pub struct FindEntry {
    name: String,
    attributes: u32,
    size: u64,
    time: u64,
}

fn find_first_file(c: &mut Cpu) -> u32 {
    let (pattern, data) = (cstr(arg(c, 0)), arg(c, 1));
    let (dir, mask) = match pattern.rfind(['\\', '/']) {
        Some(k) => (&pattern[..k], &pattern[k + 1..]),
        None => ("", pattern.as_str()),
    };
    let mut entries = Vec::new();
    if let Ok(list) = std::fs::read_dir(host_path(if dir.is_empty() { "." } else { dir })) {
        for e in list.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !wildcard(mask, &name) {
                continue;
            }
            let Ok(md) = e.metadata() else { continue };
            entries.push(FindEntry { name, attributes: attributes(&md), size: md.len(), time: md.modified().map_or(0, file_time) });
        }
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    if super::trace_api() {
        log!("FindFirstFileA {pattern}: {} found", entries.len());
    }
    let Some(first) = entries.first() else {
        set_last_error(ERROR_FILE_NOT_FOUND);
        return INVALID_HANDLE;
    };
    write_find_data(data, first);
    objects().add(Obj::Find { entries, next: 1 })
}
fn find_next_file(c: &mut Cpu) -> u32 {
    let (h, data) = (arg(c, 0), arg(c, 1));
    let mut g = objects();
    let Some(Obj::Find { entries, next }) = g.map.get_mut(&h) else {
        set_last_error(ERROR_INVALID_HANDLE);
        return 0;
    };
    match entries.get(*next) {
        Some(e) => {
            write_find_data(data, e);
            *next += 1;
            1
        }
        None => {
            drop(g);
            set_last_error(18); // ERROR_NO_MORE_FILES
            0
        }
    }
}
fn find_close(c: &mut Cpu) -> u32 {
    objects().map.remove(&arg(c, 0)).is_some() as u32
}

/// GetCurrentDirectoryA(n, buf): the installed game's folder (relative paths are the game
/// folder's, `host_path`).
fn get_current_directory(c: &mut Cpu) -> u32 {
    let (n, buf) = (arg(c, 0), arg(c, 1));
    let dir = super::INSTALL_DIR;
    if (dir.len() as u32) >= n {
        return dir.len() as u32 + 1;
    }
    put_cstr(buf, dir, n)
}

/// GetFullPathNameA(name, n, buf, &file part).
fn get_full_path_name(c: &mut Cpu) -> u32 {
    let (name, n, buf, part) = (cstr(arg(c, 0)), arg(c, 1), arg(c, 2), arg(c, 3));
    let full = if name.len() >= 2 && name.as_bytes()[1] == b':' {
        name.clone()
    } else if let Some(rest) = name.strip_prefix(['\\', '/']) {
        format!("C:\\{rest}")
    } else {
        format!("{}\\{}", super::INSTALL_DIR, name.trim_start_matches(".\\"))
    };
    if (full.len() as u32) >= n {
        return full.len() as u32 + 1;
    }
    let len = put_cstr(buf, &full, n);
    if part != 0 {
        let k = full.rfind('\\').map_or(0, |k| k + 1) as u32;
        mem().wr32(part, buf + k);
    }
    len
}

fn create_directory(c: &mut Cpu) -> u32 {
    let name = cstr(arg(c, 0));
    if super::on_cd(&name) {
        set_last_error(ERROR_ACCESS_DENIED);
        return 0;
    }
    let p = host_path(&name);
    if p.exists() {
        set_last_error(183); // ERROR_ALREADY_EXISTS
        return 0;
    }
    std::fs::create_dir(&p).is_ok() as u32
}
fn remove_directory(c: &mut Cpu) -> u32 {
    std::fs::remove_dir(host_path(&cstr(arg(c, 0)))).is_ok() as u32
}
fn move_file(c: &mut Cpu) -> u32 {
    std::fs::rename(host_path(&cstr(arg(c, 0))), host_path(&cstr(arg(c, 1)))).is_ok() as u32
}
/// CopyFileA(from, to, fail if it exists)
fn copy_file(c: &mut Cpu) -> u32 {
    let (from, to) = (host_path(&cstr(arg(c, 0))), host_path(&cstr(arg(c, 1))));
    if arg(c, 2) != 0 && to.exists() {
        set_last_error(80); // ERROR_FILE_EXISTS
        return 0;
    }
    std::fs::copy(from, to).is_ok() as u32
}

/// GetSystemTime: UTC, as GetLocalTime (the time zone is UTC).
fn get_system_time(c: &mut Cpu) -> u32 {
    get_local_time(c)
}
fn get_system_time_as_file_time(c: &mut Cpu) -> u32 {
    let t = file_time(std::time::SystemTime::now());
    mem().wr32(arg(c, 0), t as u32);
    mem().wr32(arg(c, 0) + 4, (t >> 32) as u32);
    0
}
/// FileTimeToLocalFileTime: the same (UTC).
fn file_time_to_local_file_time(c: &mut Cpu) -> u32 {
    let m = mem();
    let (from, to) = (arg(c, 0), arg(c, 1));
    let (lo, hi) = (m.rd32(from), m.rd32(from + 4));
    m.wr32(to, lo);
    m.wr32(to + 4, hi);
    1
}
fn file_time_to_system_time(c: &mut Cpu) -> u32 {
    let m = mem();
    let (ft, st) = (arg(c, 0), arg(c, 1));
    let t = m.rd32(ft) as u64 | (m.rd32(ft + 4) as u64) << 32;
    let ms_total = t / 10_000;
    let (ms, secs) = (ms_total % 1000, ms_total / 1000);
    let days = (secs / 86400) as i64 - 134_774; // 1601-01-01 to 1970-01-01
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    let weekday = (days + 4).rem_euclid(7);
    let s = secs % 86400;
    for (k, v) in [year, month, weekday, day, (s / 3600) as i64, (s / 60 % 60) as i64, (s % 60) as i64, ms as i64].into_iter().enumerate() {
        m.wr16(st + 2 * k as u32, v as u16);
    }
    1
}

/// GetUserDefaultLCID: German (Germany).
fn get_user_default_lcid(_: &mut Cpu) -> u32 {
    0x0407
}
fn returns_1(_: &mut Cpu) -> u32 {
    1
}
fn returns_0(_: &mut Cpu) -> u32 {
    0
}

/// MulDiv(a, b, c): a * b / c rounded, -1 when c is 0.
fn mul_div(c: &mut Cpu) -> u32 {
    let (a, b, d) = (arg(c, 0) as i32 as i64, arg(c, 1) as i32 as i64, arg(c, 2) as i32 as i64);
    if d == 0 {
        return u32::MAX;
    }
    let p = a * b;
    let half = d.abs() / 2;
    let r = if (p < 0) != (d < 0) { (p - half * p.signum()) / d } else { (p + half * p.signum()) / d };
    r as i32 as u32
}

fn lstrcat(c: &mut Cpu) -> u32 {
    let (dst, src) = (arg(c, 0), arg(c, 1));
    let s = cstr(src);
    let at = dst + cstr(dst).len() as u32;
    put_cstr(at, &s, s.len() as u32 + 1);
    dst
}

/// GlobalMemoryStatus(MEMORYSTATUS*): 1 GB of memory, 768 MB free, 2 GB address space.
fn global_memory_status(c: &mut Cpu) -> u32 {
    let p = arg(c, 0);
    let m = mem();
    for (k, v) in [32, 25, 1 << 30, 768 << 20, 2 << 30, 2 << 30, 0x7ffe_0000, 0x6000_0000].into_iter().enumerate() {
        m.wr32(p + 4 * k as u32, v as u32);
    }
    0
}

fn get_windows_directory(c: &mut Cpu) -> u32 {
    put_cstr(arg(c, 0), "C:\\WINDOWS", arg(c, 1))
}
fn get_system_directory(c: &mut Cpu) -> u32 {
    put_cstr(arg(c, 0), "C:\\WINDOWS\\SYSTEM", arg(c, 1))
}
/// GetTempPathA(n, buf): the game folder.
fn get_temp_path(c: &mut Cpu) -> u32 {
    put_cstr(arg(c, 1), &format!("{}\\", super::INSTALL_DIR), arg(c, 0))
}
/// GetTempFileNameA(dir, prefix, unique, buf)
fn get_temp_file_name(c: &mut Cpu) -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let (dir, prefix, unique, buf) = (cstr(arg(c, 0)), cstr(arg(c, 1)), arg(c, 2), arg(c, 3));
    let n = if unique != 0 { unique } else { NEXT.fetch_add(1, Ordering::Relaxed) };
    let prefix: String = prefix.chars().take(3).collect();
    let name = format!("{}\\{prefix}{:04X}.TMP", dir.trim_end_matches('\\'), n & 0xffff);
    put_cstr(buf, &name, 260);
    if unique == 0 {
        let _ = std::fs::File::create(host_path(&name));
    }
    n
}

fn fatal_app_exit(c: &mut Cpu) -> u32 {
    log!("FatalAppExitA: {}", cstr(arg(c, 1)));
    std::process::exit(3)
}

/// GetDiskFreeSpaceA(root, &sectors per cluster, &bytes per sector, &free clusters, &clusters):
/// 2 GB, 1 GB of it free.
fn get_disk_free_space(c: &mut Cpu) -> u32 {
    let m = mem();
    for (k, v) in [8u32, 512, 0x4_0000, 0x8_0000].into_iter().enumerate() {
        let p = arg(c, 1 + k as u32);
        if p != 0 {
            m.wr32(p, v);
        }
    }
    1
}

/// SizeofResource(module, FindResourceA's record)
fn sizeof_resource(c: &mut Cpu) -> u32 {
    let r = arg(c, 1);
    if r == 0 { 0 } else { mem().rd32(r + 4) }
}

fn release_mutex(c: &mut Cpu) -> u32 {
    let h = arg(c, 0);
    let me = thread_id();
    let mut g = objects();
    let ok = match g.map.get_mut(&h) {
        Some(Obj::Mutex { owner, count }) if *owner == me && *count > 0 => {
            *count -= 1;
            if *count == 0 {
                *owner = 0;
            }
            true
        }
        _ => false,
    };
    drop(g);
    SIGNAL.notify_all();
    if !ok {
        set_last_error(288); // ERROR_NOT_OWNER
    }
    ok as u32
}

/// VirtualQuery(address, MEMORY_BASIC_INFORMATION*, size): one committed read/write region
/// around the address (the page).
fn virtual_query(c: &mut Cpu) -> u32 {
    let (a, p) = (arg(c, 0), arg(c, 1));
    let m = mem();
    let page = a & !0xfff;
    for (k, v) in [page, page, 4, 0x1000, 0x1000, 4, 0x2_0000].into_iter().enumerate() {
        m.wr32(p + 4 * k as u32, v);
    }
    28
}

/// The `[section]` `key` of an .ini file in the game folder.
fn profile_value(file: &str, section: &str, key: &str) -> Option<String> {
    let path = if file.contains(['\\', '/', ':']) { host_path(file) } else { host_path(&format!("C:\\WINDOWS\\{file}")) };
    let text = std::fs::read(path).ok()?;
    let text: String = text.iter().map(|&b| b as char).collect();
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = s.eq_ignore_ascii_case(section);
        } else if inside {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// GetPrivateProfileStringA(section, key, default, buf, n, file)
fn get_private_profile_string(c: &mut Cpu) -> u32 {
    let (section, key, default, buf, n, file) = (arg(c, 0), arg(c, 1), arg(c, 2), arg(c, 3), arg(c, 4), cstr(arg(c, 5)));
    if section == 0 || key == 0 {
        // listing sections or keys: none
        if n >= 2 {
            mem().wr8(buf, 0);
            mem().wr8(buf + 1, 0);
        }
        return 0;
    }
    let v = profile_value(&file, &cstr(section), &cstr(key)).unwrap_or_else(|| if default == 0 { String::new() } else { cstr(default) });
    put_cstr(buf, &v, n)
}
/// GetPrivateProfileIntA(section, key, default, file)
fn get_private_profile_int(c: &mut Cpu) -> u32 {
    let (section, key, default, file) = (cstr(arg(c, 0)), cstr(arg(c, 1)), arg(c, 2), cstr(arg(c, 3)));
    profile_value(&file, &section, &key).and_then(|v| {
        let digits: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
        digits.parse::<i32>().ok()
    }).map_or(default, |v| v as u32)
}

pub const MORE_IMPORTS: &[Import] = &[
    ("GetVersionExA", 1, get_version_ex),
    ("LocalAlloc", 2, local_alloc),
    ("LocalFree", 1, local_free),
    ("GetEnvironmentVariableA", 3, get_environment_variable),
    ("GetFileAttributesA", 1, get_file_attributes),
    ("SetFileAttributesA", 2, set_file_attributes),
    ("FindFirstFileA", 2, find_first_file),
    ("FindNextFileA", 2, find_next_file),
    ("FindClose", 1, find_close),
    ("GetCurrentDirectoryA", 2, get_current_directory),
    ("GetFullPathNameA", 4, get_full_path_name),
    ("CreateDirectoryA", 2, create_directory),
    ("RemoveDirectoryA", 1, remove_directory),
    ("MoveFileA", 2, move_file),
    ("CopyFileA", 3, copy_file),
    ("GetSystemTime", 1, get_system_time),
    ("GetSystemTimeAsFileTime", 1, get_system_time_as_file_time),
    ("FileTimeToLocalFileTime", 2, file_time_to_local_file_time),
    ("FileTimeToSystemTime", 2, file_time_to_system_time),
    ("GetUserDefaultLCID", 0, get_user_default_lcid),
    ("IsValidCodePage", 1, returns_1),
    ("IsValidLocale", 2, returns_1),
    ("EnumSystemLocalesA", 2, returns_1),
    ("IsDBCSLeadByte", 1, returns_0),
    ("SetConsoleCtrlHandler", 2, returns_1),
    ("MulDiv", 3, mul_div),
    ("lstrcatA", 2, lstrcat),
    ("GlobalMemoryStatus", 1, global_memory_status),
    ("GetWindowsDirectoryA", 2, get_windows_directory),
    ("GetSystemDirectoryA", 2, get_system_directory),
    ("GetTempPathA", 2, get_temp_path),
    ("GetTempFileNameA", 4, get_temp_file_name),
    ("FatalAppExitA", 2, fatal_app_exit),
    ("GetDiskFreeSpaceA", 5, get_disk_free_space),
    ("SizeofResource", 2, sizeof_resource),
    ("ReleaseMutex", 1, release_mutex),
    ("VirtualQuery", 3, virtual_query),
    ("GetPrivateProfileStringA", 6, get_private_profile_string),
    ("GetPrivateProfileIntA", 4, get_private_profile_int),
];


/// GetFileInformationByHandle(file, BY_HANDLE_FILE_INFORMATION*)
fn get_file_information_by_handle(c: &mut Cpu) -> u32 {
    let (h, p) = (arg(c, 0), arg(c, 1));
    let Some(f) = file_of(h) else {
        set_last_error(ERROR_INVALID_HANDLE);
        return 0;
    };
    let Ok(md) = f.lock().unwrap().metadata() else { return 0 };
    let m = mem();
    m.slice(p, 52).fill(0);
    m.wr32(p, attributes(&md));
    let t = md.modified().map_or(0, file_time);
    for o in [4, 12, 20] {
        m.wr32(p + o, t as u32);
        m.wr32(p + o + 4, (t >> 32) as u32);
    }
    m.wr32(p + 28, 0x1234_5678); // volume serial number
    m.wr32(p + 32, (md.len() >> 32) as u32);
    m.wr32(p + 36, md.len() as u32);
    m.wr32(p + 40, 1); // links
    m.wr32(p + 48, h); // file index
    1
}

/// CreateFileMappingA(file, security, protect, size high, size low, name): a mapping of a
/// file we opened (read only: views are copies).
fn create_file_mapping(c: &mut Cpu) -> u32 {
    let h = arg(c, 0);
    let Some(f) = file_of(h) else {
        log!("CreateFileMappingA({h:#x}): only files can be mapped");
        set_last_error(ERROR_INVALID_HANDLE);
        return 0;
    };
    objects().add(Obj::Mapping(f))
}

/// MapViewOfFile(mapping, access, offset high, offset low, bytes): the file's bytes copied
/// into guest memory.
fn map_view_of_file(c: &mut Cpu) -> u32 {
    let (h, hi, lo, n) = (arg(c, 0), arg(c, 2), arg(c, 3), arg(c, 4));
    let f = match objects().map.get(&h) {
        Some(Obj::Mapping(f)) => f.clone(),
        _ => {
            set_last_error(ERROR_INVALID_HANDLE);
            return 0;
        }
    };
    let mut f = f.lock().unwrap();
    let at = (hi as u64) << 32 | lo as u64;
    let len = f.metadata().map_or(0, |m| m.len()).saturating_sub(at);
    let n = if n == 0 { len } else { (n as u64).min(len) } as u32;
    let p = alloc(n.max(1));
    if f.seek(SeekFrom::Start(at)).is_err() || f.read_exact(mem().slice(p, n as usize)).is_err() {
        free(p);
        return 0;
    }
    p
}

fn unmap_view_of_file(c: &mut Cpu) -> u32 {
    free(arg(c, 0)) as u32
}

/// FormatMessageA: no message texts.
fn format_message(c: &mut Cpu) -> u32 {
    log!("FormatMessageA(flags {:#x}, message {:#x}): no texts", arg(c, 0), arg(c, 2));
    0
}

/// SYSTEMTIME at `p`, or now (UTC) when `p` is 0: year, month, day, hour, minute, second.
fn system_time_at(p: u32) -> [u16; 6] {
    let m = mem();
    let s = if p != 0 { std::array::from_fn(|k| m.rd16(p + 2 * k as u32)) } else { utc_now() };
    [s[0], s[1], s[3], s[4], s[5], s[6]]
}

/// GetDateFormatA(locale, flags, SYSTEMTIME*, format, buf, n): "dd.MM.yyyy" (German), or
/// the format given with d, M and y.
fn get_date_format(c: &mut Cpu) -> u32 {
    let (st, fmt, buf, n) = (arg(c, 2), arg(c, 3), arg(c, 4), arg(c, 5));
    let [y, mo, d, ..] = system_time_at(st);
    let fmt = if fmt == 0 { "dd.MM.yyyy".to_string() } else { cstr(fmt) };
    let s = fmt.replace("yyyy", &format!("{y:04}")).replace("yy", &format!("{:02}", y % 100)).replace("MM", &format!("{mo:02}")).replace("dd", &format!("{d:02}"));
    if n == 0 {
        return s.len() as u32 + 1;
    }
    put_cstr(buf, &s, n) + 1
}

/// GetTimeFormatA(locale, flags, SYSTEMTIME*, format, buf, n): "HH:mm:ss" or the format given.
fn get_time_format(c: &mut Cpu) -> u32 {
    let (st, fmt, buf, n) = (arg(c, 2), arg(c, 3), arg(c, 4), arg(c, 5));
    let [.., h, mi, s] = system_time_at(st);
    let fmt = if fmt == 0 { "HH:mm:ss".to_string() } else { cstr(fmt) };
    let out = fmt.replace("HH", &format!("{h:02}")).replace("hh", &format!("{:02}", if h % 12 == 0 { 12 } else { h % 12 })).replace("mm", &format!("{mi:02}")).replace("ss", &format!("{s:02}"));
    if n == 0 {
        return out.len() as u32 + 1;
    }
    put_cstr(buf, &out, n) + 1
}

/// OpenProcess: other processes are not there.
fn open_process(_: &mut Cpu) -> u32 {
    set_last_error(87);
    0
}

pub const MORE_IMPORTS_2: &[Import] = &[
    ("GetFileInformationByHandle", 2, get_file_information_by_handle),
    ("CreateFileMappingA", 6, create_file_mapping),
    ("MapViewOfFile", 5, map_view_of_file),
    ("UnmapViewOfFile", 1, unmap_view_of_file),
    ("FormatMessageA", 7, format_message),
    ("GetDateFormatA", 6, get_date_format),
    ("GetTimeFormatA", 6, get_time_format),
    ("OpenProcess", 3, open_process),
];

pub const IMPORTS: &[Import] = &[
    ("FindResourceA", 3, find_resource),
    ("LoadResource", 2, load_resource),
    ("LockResource", 1, first_arg),
    ("_lopen", 2, lopen),
    ("_lread", 3, lread),
    ("_lclose", 1, lclose),
    ("GetFileSize", 2, get_file_size),
    ("DeleteFileA", 1, delete_file),
    ("SetCurrentDirectoryA", 1, set_current_directory),
    ("InterlockedExchange", 2, interlocked_exchange),
    ("SetUnhandledExceptionFilter", 1, ret0),
    ("IsBadReadPtr", 2, ret0),
    ("IsBadWritePtr", 2, ret0),
    ("IsBadCodePtr", 1, ret0),
    ("GetTimeZoneInformation", 1, get_time_zone_information),
    ("GetLocalTime", 1, get_local_time),
    ("TlsFree", 1, ret1),
    ("GetVersion", 0, get_version),
    ("GetCommandLineA", 0, get_command_line),
    ("GetStartupInfoA", 1, get_startup_info),
    ("GetModuleHandleA", 1, get_module_handle),
    ("GetModuleFileNameA", 3, get_module_file_name),
    ("GetEnvironmentStrings", 0, get_environment_strings),
    ("GetEnvironmentStringsW", 0, get_environment_strings),
    ("FreeEnvironmentStringsA", 1, ret1),
    ("FreeEnvironmentStringsW", 1, ret1),
    ("SetEnvironmentVariableA", 2, ret1),
    ("SetEnvironmentVariableW", 2, ret1),
    ("ExitProcess", 1, exit_process),
    ("TerminateProcess", 2, exit_process_from_terminate),
    ("ExitThread", 1, exit_thread),
    ("GetSystemInfo", 1, get_system_info),
    ("GetComputerNameA", 2, get_computer_name),
    ("OutputDebugStringA", 1, output_debug_string),
    ("RaiseException", 4, raise_exception),
    ("RtlUnwind", 4, ret0),
    ("UnhandledExceptionFilter", 1, ret0),
    ("DebugBreak", 0, ret0),
    ("Beep", 2, ret1),
    ("SetErrorMode", 1, ret0),
    ("GetLastError", 0, get_last_error),
    ("SetLastError", 1, set_last_error_imp),
    ("GetCurrentProcess", 0, get_current_process),
    ("GetCurrentThread", 0, get_current_thread),
    ("GetCurrentThreadId", 0, get_current_thread_id),
    ("GetCurrentProcessId", 0, |_| PROCESS_ID),
    ("DisableThreadLibraryCalls", 1, |_| 1),
    ("SetHandleCount", 1, first_arg),
    ("SetStdHandle", 2, ret1),
    ("LoadLibraryA", 1, load_library),
    ("FreeLibrary", 1, ret1),
    ("GetProcAddress", 2, get_proc_address),
    ("GetLogicalDrives", 0, get_logical_drives),
    ("GetDriveTypeA", 1, get_drive_type),
    ("GetVolumeInformationA", 8, get_volume_information),
    ("GetACP", 0, get_acp),
    ("GetOEMCP", 0, get_oemcp),
    ("GetCPInfo", 2, get_cp_info),
    ("MultiByteToWideChar", 6, multi_byte_to_wide_char),
    ("WideCharToMultiByte", 8, wide_char_to_multi_byte),
    ("GetStringTypeA", 5, get_string_type_a),
    ("GetStringTypeW", 4, get_string_type_w),
    ("LCMapStringA", 6, lc_map_string_a),
    ("LCMapStringW", 6, lc_map_string_w),
    ("CompareStringA", 6, compare_string_a),
    ("CompareStringW", 6, compare_string_w),
    ("GetLocaleInfoA", 4, get_locale_info_a),
    ("GetLocaleInfoW", 4, get_locale_info_w),
    ("lstrlenA", 1, lstrlen),
    ("lstrcpyA", 2, lstrcpy),
    ("HeapCreate", 3, heap_create),
    ("HeapDestroy", 1, ret1),
    ("HeapAlloc", 3, heap_alloc),
    ("HeapFree", 3, heap_free),
    ("HeapReAlloc", 4, heap_realloc),
    ("HeapSize", 3, heap_size),
    ("VirtualAlloc", 4, virtual_alloc),
    ("VirtualFree", 3, virtual_free),
    ("VirtualProtect", 4, virtual_protect),
    ("GlobalAlloc", 2, global_alloc),
    ("GlobalFree", 1, global_free),
    ("GlobalLock", 1, global_same),
    ("GlobalHandle", 1, global_same),
    ("GlobalUnlock", 1, global_unlock),
    ("TlsAlloc", 0, tls_alloc),
    ("TlsGetValue", 1, tls_get_value),
    ("TlsSetValue", 2, tls_set_value),
    ("InitializeCriticalSection", 1, init_cs),
    ("DeleteCriticalSection", 1, delete_cs),
    ("EnterCriticalSection", 1, enter_cs),
    ("LeaveCriticalSection", 1, leave_cs),
    ("InterlockedIncrement", 1, interlocked_increment),
    ("InterlockedDecrement", 1, interlocked_decrement),
    ("CreateEventA", 4, create_event),
    ("SetEvent", 1, set_event),
    ("ResetEvent", 1, reset_event),
    ("PulseEvent", 1, pulse_event),
    ("CreateMutexA", 3, create_mutex),
    ("WaitForSingleObject", 2, wait_for_single_object),
    ("WaitForMultipleObjects", 4, wait_for_multiple_objects),
    ("WaitForMultipleObjectsEx", 5, wait_for_multiple_objects),
    ("CloseHandle", 1, close_handle),
    ("DuplicateHandle", 7, duplicate_handle),
    ("CreateThread", 6, create_thread),
    ("ResumeThread", 1, resume_thread),
    ("TerminateThread", 2, terminate_thread),
    ("SetThreadPriority", 2, ret1),
    ("GetThreadPriority", 1, ret0),
    ("GetTickCount", 0, get_tick_count),
    ("QueryPerformanceFrequency", 1, query_performance_frequency),
    ("QueryPerformanceCounter", 1, query_performance_counter),
    ("Sleep", 1, sleep),
    ("SleepEx", 2, sleep),
    ("CreateFileA", 7, create_file),
    ("ReadFile", 5, read_file),
    ("WriteFile", 5, write_file),
    ("SetFilePointer", 4, set_file_pointer),
    ("SetEndOfFile", 1, set_end_of_file),
    ("FlushFileBuffers", 1, flush_file_buffers),
    ("GetStdHandle", 1, get_std_handle),
    ("GetFileType", 1, get_file_type),
];

fn exit_process_from_terminate(c: &mut Cpu) -> u32 {
    let code = arg(c, 1);
    super::advapi32::save();
    log!("TerminateProcess({code})");
    std::process::exit(code as i32)
}

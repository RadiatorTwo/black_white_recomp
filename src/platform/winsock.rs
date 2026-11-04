//! WSOCK32 / WS2_32 (Winsock, by ordinal; LHMultiplayerR loads WS2_32 when it starts the
//! network): no network. Startup works, the byte order helpers and the host's name too;
//! sockets cannot be made (WSAEAFNOSUPPORT), so the game finds no network play.

use super::{Import, alloc, arg, cstr, log, mem, put_cstr};
use crate::rt::Cpu;
use std::sync::atomic::{AtomicU32, Ordering};

const SOCKET_ERROR: u32 = u32::MAX;
const WSAEAFNOSUPPORT: u32 = 10047;
const WSAENOTSOCK: u32 = 10038;

static LAST: AtomicU32 = AtomicU32::new(0);

fn fail(e: u32) -> u32 {
    LAST.store(e, Ordering::Relaxed);
    SOCKET_ERROR
}

/// WSAStartup(version, WSADATA*): Winsock 2.2.
fn wsa_startup(c: &mut Cpu) -> u32 {
    let p = arg(c, 1);
    let m = mem();
    m.slice(p, 400).fill(0);
    m.wr16(p, 0x0202);
    m.wr16(p + 2, 0x0202);
    put_cstr(p + 4, "WinSock 2.0", 257);
    put_cstr(p + 261, "Running", 129);
    m.wr16(p + 390, 100); // max sockets
    m.wr16(p + 392, 65467); // max datagram
    0
}
fn wsa_cleanup(_: &mut Cpu) -> u32 {
    0
}
fn wsa_get_last_error(_: &mut Cpu) -> u32 {
    LAST.load(Ordering::Relaxed)
}
fn socket(c: &mut Cpu) -> u32 {
    log!("socket({}, {}, {}): no network", arg(c, 0), arg(c, 1), arg(c, 2));
    fail(WSAEAFNOSUPPORT)
}
/// Everything on a socket: there are none.
fn not_a_socket(_: &mut Cpu) -> u32 {
    fail(WSAENOTSOCK)
}
fn htonl(c: &mut Cpu) -> u32 {
    arg(c, 0).swap_bytes()
}
fn htons(c: &mut Cpu) -> u32 {
    (arg(c, 0) as u16).swap_bytes() as u32
}
/// inet_addr("a.b.c.d"): the address in network order, INADDR_NONE when it is none.
fn inet_addr(c: &mut Cpu) -> u32 {
    let s = cstr(arg(c, 0));
    match s.trim().parse::<std::net::Ipv4Addr>() {
        Ok(a) => u32::from_le_bytes(a.octets()),
        Err(_) => u32::MAX,
    }
}
/// inet_ntoa(address): a static buffer
fn inet_ntoa(c: &mut Cpu) -> u32 {
    static BUF: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    let buf = *BUF.get_or_init(|| alloc(16));
    let b = arg(c, 0).to_le_bytes();
    put_cstr(buf, &format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]), 16);
    buf
}
fn gethostname(c: &mut Cpu) -> u32 {
    put_cstr(arg(c, 0), "localhost", arg(c, 1));
    0
}
/// gethostbyname / gethostbyaddr: 127.0.0.1 for every name.
fn gethostbyname(_: &mut Cpu) -> u32 {
    static HOST: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *HOST.get_or_init(|| {
        let m = mem();
        // hostent: name, aliases, type (2), length (4), address list; then the data
        let h = alloc(64);
        let name = h + 32;
        put_cstr(name, "localhost", 16);
        let (aliases, list, addr) = (h + 16, h + 20, h + 28);
        m.wr32(h, name);
        m.wr32(h + 4, aliases);
        m.wr16(h + 8, 2);
        m.wr16(h + 10, 4);
        m.wr32(h + 12, list);
        m.wr32(aliases, 0);
        m.wr32(list, addr);
        m.wr32(list + 4, 0);
        m.wr32(addr, u32::from_le_bytes([127, 0, 0, 1]));
        h
    })
}
/// __WSAFDIsSet(socket, fd_set*)
fn fd_is_set(c: &mut Cpu) -> u32 {
    let (s, set) = (arg(c, 0), arg(c, 1));
    let m = mem();
    (0..m.rd32(set).min(64)).any(|k| m.rd32(set + 4 + 4 * k) == s) as u32
}
/// select: nothing ever becomes ready.
fn select(_: &mut Cpu) -> u32 {
    0
}
fn wsa_event_select(_: &mut Cpu) -> u32 {
    fail(WSAENOTSOCK)
}

pub const IMPORTS: &[Import] = &[
    ("#1", 3, not_a_socket),    // accept
    ("#2", 3, not_a_socket),    // bind
    ("#3", 1, not_a_socket),    // closesocket
    ("#4", 3, not_a_socket),    // connect
    ("#5", 3, not_a_socket),    // getpeername
    ("#6", 3, not_a_socket),    // getsockname
    ("#7", 5, not_a_socket),    // getsockopt
    ("#8", 1, htonl),           // htonl
    ("#9", 1, htons),           // htons
    ("#10", 1, inet_addr),      // inet_addr
    ("#11", 1, inet_ntoa),      // inet_ntoa
    ("#12", 3, not_a_socket),   // ioctlsocket
    ("#13", 2, not_a_socket),   // listen
    ("#14", 1, htonl),          // ntohl
    ("#15", 1, htons),          // ntohs
    ("#16", 4, not_a_socket),   // recv
    ("#17", 6, not_a_socket),   // recvfrom
    ("#18", 5, select),         // select
    ("#19", 4, not_a_socket),   // send
    ("#20", 6, not_a_socket),   // sendto
    ("#21", 5, not_a_socket),   // setsockopt
    ("#22", 2, not_a_socket),   // shutdown
    ("#23", 3, socket),         // socket
    ("#51", 3, gethostbyname),  // gethostbyaddr
    ("#52", 1, gethostbyname),  // gethostbyname
    ("#57", 2, gethostname),    // gethostname
    ("#111", 0, wsa_get_last_error),
    ("#115", 2, wsa_startup),
    ("#116", 0, wsa_cleanup),
    ("#151", 2, fd_is_set),
    ("WSAEventSelect", 3, wsa_event_select),
];

use std::os::raw::c_int;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

static SEEN_INTERRUPT: AtomicUsize = AtomicUsize::new(0);
static CONNECTION: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

#[cfg(unix)]
extern "C" {
    fn signal(sig: c_int, handler: extern "C" fn(c_int)) -> extern "C" fn(c_int);
    fn _exit(code: c_int) -> !;
}

#[cfg(unix)]
const SIGINT: c_int = 2;

#[cfg(windows)]
const CTRL_C_EVENT: u32 = 0;
#[cfg(windows)]
const CTRL_BREAK_EVENT: u32 = 1;

#[cfg(windows)]
extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn ExitProcess(code: u32) -> !;
}

#[cfg(unix)]
extern "C" fn interrupt_handler(_: c_int) {
    let count = SEEN_INTERRUPT.fetch_add(1, Ordering::SeqCst) + 1;
    if count > 2 {
        unsafe { _exit(1) };
    }
    let con = CONNECTION.load(Ordering::SeqCst);
    if !con.is_null() {
        unsafe { duckdb_sys::duckdb_interrupt(con as duckdb_sys::duckdb_connection) };
    }
}

#[cfg(windows)]
extern "system" fn console_ctrl_handler(ctrl_type: u32) -> i32 {
    if ctrl_type != CTRL_C_EVENT && ctrl_type != CTRL_BREAK_EVENT {
        return 0;
    }
    let count = SEEN_INTERRUPT.fetch_add(1, Ordering::SeqCst) + 1;
    if count > 2 {
        unsafe { ExitProcess(1) };
    }
    let con = CONNECTION.load(Ordering::SeqCst);
    if !con.is_null() {
        unsafe { duckdb_sys::duckdb_interrupt(con as duckdb_sys::duckdb_connection) };
    }
    1
}

pub fn install(stdin_is_interactive: bool) {
    let _ = stdin_is_interactive;
    SEEN_INTERRUPT.store(0, Ordering::SeqCst);

    #[cfg(unix)]
    unsafe {
        signal(SIGINT, interrupt_handler);
    }

    #[cfg(windows)]
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(console_ctrl_handler), 1);
    }
}

pub fn set_connection(con: duckdb_sys::duckdb_connection) {
    CONNECTION.store(con as *mut _, Ordering::SeqCst);
}

pub fn clear_connection() {
    CONNECTION.store(std::ptr::null_mut(), Ordering::SeqCst);
}

pub fn has_seen_interrupt() -> bool {
    SEEN_INTERRUPT.load(Ordering::SeqCst) != 0
}

pub fn clear_interrupt() {
    SEEN_INTERRUPT.store(0, Ordering::SeqCst);
}

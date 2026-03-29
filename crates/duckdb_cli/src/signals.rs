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

pub fn install(stdin_is_interactive: bool) {
    let _ = stdin_is_interactive;
    SEEN_INTERRUPT.store(0, Ordering::SeqCst);

    #[cfg(unix)]
    unsafe {
        signal(SIGINT, interrupt_handler);
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

//! Reverse-engineering aid: a hardware write watchpoint on one address of the client, to find which
//! code writes it (used to find the client's bone-matrix writers). Armed by the core when a file
//! `skatecraft_watch.txt` appears next to Wow.exe; results go to the log.

use core::ffi::c_void;
use std::collections::HashMap;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{CloseHandle, EXCEPTION_SINGLE_STEP};
use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, GetThreadContext, RemoveVectoredExceptionHandler, SetThreadContext, CONTEXT, EXCEPTION_POINTERS,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread};

const CONTEXT_I386: u32 = 0x0001_0000;
const CONTEXT_DEBUG_REGISTERS: u32 = CONTEXT_I386 | 0x10;
const THREAD_ALL: u32 = 0x001F_FFFF;

/// eip of the writing instruction -> (hits, a few return addresses found on the stack)
static HITS: Mutex<Option<HashMap<u32, (u32, Vec<u32>)>>> = Mutex::new(None);
static HANDLER: Mutex<usize> = Mutex::new(0);

unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    let rec = &*(*info).ExceptionRecord;
    if rec.ExceptionCode == EXCEPTION_SINGLE_STEP {
        let ctx = &mut *(*info).ContextRecord;
        if ctx.Dr6 & 1 != 0 {
            let mut rets = Vec::new();
            let esp = ctx.Esp as *const u32;
            for i in 0..96 {
                let v = *esp.add(i);
                if (0x0040_1000..0x00C0_0000).contains(&v) && *((v - 5) as *const u8) == 0xE8 {
                    rets.push(v);
                    if rets.len() >= 6 {
                        break;
                    }
                }
            }
            if let Ok(mut g) = HITS.lock() {
                let map = g.get_or_insert_with(HashMap::new);
                let e = map.entry(ctx.Eip).or_insert((0, rets));
                e.0 += 1;
            }
            ctx.Dr6 = 0;
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }
    }
    0 // EXCEPTION_CONTINUE_SEARCH
}

unsafe fn for_each_thread(f: impl Fn(*mut c_void)) {
    let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    let mut te: THREADENTRY32 = core::mem::zeroed();
    te.dwSize = core::mem::size_of::<THREADENTRY32>() as u32;
    let (pid, me) = (GetCurrentProcessId(), GetCurrentThreadId());
    if Thread32First(snap, &mut te) != 0 {
        loop {
            if te.th32OwnerProcessID == pid && te.th32ThreadID != me {
                let h = OpenThread(THREAD_ALL, 0, te.th32ThreadID);
                if !h.is_null() {
                    SuspendThread(h);
                    f(h);
                    ResumeThread(h);
                    CloseHandle(h);
                }
            }
            if Thread32Next(snap, &mut te) == 0 {
                break;
            }
        }
    }
    CloseHandle(snap);
}

/// Watch writes to the dword at `addr` on every thread of the client.
pub unsafe fn arm(addr: usize) {
    arm_mode(addr, false)
}

/// `read_too`: also trap reads (to find the code that consumes a value).
pub unsafe fn arm_mode(addr: usize, read_too: bool) {
    *HITS.lock().unwrap() = Some(HashMap::new());
    let h = AddVectoredExceptionHandler(1, Some(handler));
    *HANDLER.lock().unwrap() = h as usize;
    for_each_thread(|t| {
        let mut ctx: CONTEXT = core::mem::zeroed();
        ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;
        if GetThreadContext(t, &mut ctx) != 0 {
            ctx.Dr0 = addr as u32;
            ctx.Dr7 = if read_too { 0x000F_0001 } else { 0x000D_0001 }; // enable DR0: write (or read/write), 4 bytes
            SetThreadContext(t, &ctx);
        }
    });
}

pub unsafe fn disarm() -> Vec<(u32, u32, Vec<u32>)> {
    for_each_thread(|t| {
        let mut ctx: CONTEXT = core::mem::zeroed();
        ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;
        if GetThreadContext(t, &mut ctx) != 0 {
            ctx.Dr7 = 0;
            ctx.Dr0 = 0;
            SetThreadContext(t, &ctx);
        }
    });
    let h = core::mem::take(&mut *HANDLER.lock().unwrap());
    if h != 0 {
        RemoveVectoredExceptionHandler(h as *mut c_void);
    }
    let mut out: Vec<(u32, u32, Vec<u32>)> = HITS.lock().unwrap().take().unwrap_or_default().into_iter().map(|(e, (n, r))| (e, n, r)).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

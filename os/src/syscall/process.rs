//! Process management syscalls
use crate::{
    mm::{translated_byte_buffer, PageTable, PTEFlags, VirtAddr},
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next, get_syscall_count,
        mmap_current, munmap_current, suspend_current_and_run_next,
    },
    timer::get_time_us,
};
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let tv = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let src = unsafe {
        core::slice::from_raw_parts((&tv as *const TimeVal) as *const u8, size_of::<TimeVal>())
    };
    let mut offset = 0;
    for buffer in translated_byte_buffer(current_user_token(), ts as *const u8, size_of::<TimeVal>())
    {
        let len = buffer.len();
        buffer.copy_from_slice(&src[offset..offset + len]);
        offset += len;
    }
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => translated_byte(id, PTEFlags::R)
            .map(|byte| *byte as isize)
            .unwrap_or(-1),
        1 => {
            if let Some(byte) = translated_byte(id, PTEFlags::W) {
                *byte = data as u8;
                0
            } else {
                -1
            }
        }
        2 => get_syscall_count(id),
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap");
    mmap_current(start, len, port)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    munmap_current(start, len)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

fn translated_byte(addr: usize, flag: PTEFlags) -> Option<&'static mut u8> {
    let page_table = PageTable::from_token(current_user_token());
    let va = VirtAddr::from(addr);
    page_table
        .translate(va.floor())
        .filter(|pte| pte.is_valid() && pte.flags().contains(PTEFlags::U | flag))
        .map(|pte| &mut pte.ppn().get_bytes_array()[va.page_offset()])
}

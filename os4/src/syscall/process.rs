//! Process management syscalls

use core::{mem, slice};

use alloc::task;

use crate::config::MAX_SYSCALL_NUM;
use crate::mm::{PageTable, VirtAddr, PhysAddr};
use crate::task::{
    current_user_token, exit_current_and_run_next, get_slice_buffer, get_task_info, mmap, munmap,
    suspend_current_and_run_next, TaskStatus,
};
use crate::timer::get_time_us;
#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    info!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

// YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let _us = get_time_us();
    let (sec, usec) = (_us / 1_000_000, _us % 1_000_000);
    // info!("[get time], sec{}", _us / 1_000_000);
    let ts = TimeVal {
        sec,
        usec,
    };
    if let Some(buffer) = get_slice_buffer::<TimeVal>(_ts as usize) {
        // println!("[get time] ts: {:?},buffer:", ts);
        *buffer = ts;
        0
    } else {
        -1
    }
}

// CLUE: 从 ch4 开始不再对调度算法进行测试~
pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    mmap(_start, _len, _prot)
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    munmap(_start, _len)
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let task_info = get_task_info();
    if let Some(buffer) = get_slice_buffer::<TaskInfo>(ti as usize) {
        *buffer = task_info;
        0
    } else {
        -1
    }
}

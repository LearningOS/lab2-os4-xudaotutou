//! Process management syscalls

use core::{mem, slice};

use alloc::task;

use crate::config::MAX_SYSCALL_NUM;
use crate::mm::{Addr, PageTable, VirtAddr};
use crate::task::{
    current_user_token, exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, get_task_info,
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
    let ts = TimeVal {
        sec: _us / 1_000_000,
        usec: _us % 1_000_000,
    };
    let satp = current_user_token();
    let pt = PageTable::from_token(satp);
    let va = VirtAddr::from(_ts as usize);
    let vpn = va.floor();
    if pt.translate(vpn).is_none() {
        return -1;
    }
    let pte = pt.translate(vpn).unwrap();
    let ppn = pte.ppn();

    let buffer = ppn.get_bytes_array();
    let offset = va.page_offset();

    let data = unsafe {slice::from_raw_parts(
        (&ts as *const TimeVal) as *const u8,
        mem::size_of::<TimeVal>(),
    )};
    data.iter().enumerate().for_each(|(i,d)| {
        buffer[offset + i] = *d;
    });
    0
}

// CLUE: 从 ch4 开始不再对调度算法进行测试~
pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    0
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    -1
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let task_info = get_task_info();
    let satp = current_user_token();
    let pt = PageTable::from_token(satp);
    let va = VirtAddr::from(ti as usize);
    let vpn = va.floor();
    if pt.translate(vpn).is_none() {
        return -1;
    }
    let pte = pt.translate(vpn).unwrap();
    let ppn = pte.ppn();

    let buffer = ppn.get_bytes_array();
    let offset = va.page_offset();

    let data = unsafe {slice::from_raw_parts(
        (&task_info as *const TaskInfo) as *const u8,
        mem::size_of::<TaskInfo>(),
    )};
    data.iter().enumerate().for_each(|(i,d)| {
        buffer[offset + i] = *d;
    });
    0
}

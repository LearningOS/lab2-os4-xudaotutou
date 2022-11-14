//! Process management syscalls

use core::{mem, slice};

use alloc::task;

use crate::config::MAX_SYSCALL_NUM;
use crate::mm::{Addr, PageTable, VirtAddr};
use crate::task::{
    current_user_token, exit_current_and_run_next, get_slice_buffer, get_task_info, mmap,
    suspend_current_and_run_next, TaskStatus, munmap,
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
    if let Some(buffer) = get_slice_buffer(_ts as usize) {
        let data = unsafe {
            slice::from_raw_parts(
                (&ts as *const TimeVal) as *const u8,
                mem::size_of::<TimeVal>(),
            )
        };
        data.iter().enumerate().for_each(|(i, d)| {
            buffer[i] = *d;
        });
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
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    
    if _len == 0 {return  0;}
    // 0，1，2位有效，其他位必须为0,mask => b 0...0111 =>0x7
    if _port & 0x7 == 0 || _port & !0x7 != 0 || (_start % 4096) != 0 {
        return -1;
    }

    mmap(_start, _len, _port as u8)
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    if _len == 0 {return  0;}
    munmap(_start,_len)
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let task_info = get_task_info();
    if let Some(buffer) = get_slice_buffer(ti as usize) {
        let data = unsafe {
            slice::from_raw_parts(
                (&task_info as *const TaskInfo) as *const u8,
                mem::size_of::<TaskInfo>(),
            )
        };
        data.iter().enumerate().for_each(|(i, &d)| {
            buffer[i] = d;
        });
        0
    } else {
        -1
    }
}

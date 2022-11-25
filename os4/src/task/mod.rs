//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see [`__switch`]. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::config::PAGE_SIZE;
use crate::loader::{get_app_data, get_num_app};
use crate::mm::{ MapPermission, PageTable, VirtAddr, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::syscall::TaskInfo;
use crate::timer::get_time_us;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
pub use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        info!("init TASK_MANAGER");
        let num_app = get_num_app();
        info!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        Self::set_task_time(next_task);
        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    #[allow(clippy::mut_from_ref)]
    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            Self::set_task_time(&mut inner.tasks[next]);
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    pub fn get_task_info(&self) -> TaskInfo {
        let inner = self.inner.exclusive_access();
        let cur = &inner.tasks[inner.current_task];
        TaskInfo {
            status: TaskStatus::Running,
            syscall_times: cur.syscall_times,
            time: (get_time_us() - cur.start_time) / 1000,
        }
    }
    fn set_task_time(cur: &mut TaskControlBlock) {
        if cur.start_time == 0 {
            cur.start_time = get_time_us();
        }
    }

    pub fn get_slice_buffer(&self, start: usize) -> Option<&'static mut [u8]> {
        let satp = self.get_current_token();
        let pt = PageTable::from_token(satp);
        let va = VirtAddr::from(start);
        let vpn = va.floor();
        if pt.translate(vpn).is_none() {
            return None;
        }
        let pte = pt.translate(vpn).unwrap();
        let ppn = pte.ppn();

        let buffer = ppn.get_bytes_array();
        let offset = va.page_offset();
        // 可能会越出当前页?
        Some(&mut buffer[offset..])
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

pub fn get_task_info() -> TaskInfo {
    TASK_MANAGER.get_task_info()
}

pub fn get_slice_buffer(start: usize) -> Option<&'static mut [u8]> {
    TASK_MANAGER.get_slice_buffer(start)
}

// fn mem_in_range(cur_tcb: &TaskControlBlock, start: usize, end: usize) -> bool {
//     let (l, r) = (VirtAddr::from(start), VirtAddr::from(end));
//     let (lvpn, rvpn) = (l.floor(), r.ceil());
//     // let (lmin, _) = cur_tcb.memory_set.get_l_r(); 
//     for area in &cur_tcb.memory_set.areas {
//         if lvpn <= area.get_start() && rvpn > area.get_start() {
//             return false;
//         }
//     }
//     true
// }
fn get_vpn(start: usize, end: usize)->(VirtPageNum, VirtPageNum) {
    let (l, r) = (VirtAddr::from(start), VirtAddr::from(end));
    (l.floor(), r.ceil())
}
pub fn mmap(start: usize, len: usize, port:  usize) -> isize {
    if len == 0 {return  0;}
    // 0，1，2位有效，其他位必须为0,mask => b 0...0111 =>0x7
    if port & 0x7 == 0 || port & !0x7 != 0 || (start % 4096) != 0 {
        return -1;
    }
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let idx = inner.current_task;
    let cur_tcb = &mut inner.tasks[idx];
    let end = start + len;
    print!("mmap!!!");
    let (lvpn, rvpn) = get_vpn(start, end);
    for area in &cur_tcb.memory_set.areas {
        if lvpn <= area.get_start() && rvpn > area.get_start() {
            return -1;
        }
    }
    let mut permission = MapPermission::from_bits((port as u8) << 1).unwrap();
    permission.set(MapPermission::U, true);

    (0..end).step_by(PAGE_SIZE).for_each(|l| {
        
        let r = end.min(l + PAGE_SIZE);
        println!("start: {}, end: {}, real end: {}",l, r, end);
        cur_tcb
            .memory_set
            .insert_framed_area(l.into(), r.into(), permission);
    });
    0
}

pub fn munmap(start: usize, len: usize) -> isize {
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let idx = inner.current_task;
    let cur_tcb = &mut inner.tasks[idx];
    let mut cnt = 0;
    
    let (lvpn, rvpn) = get_vpn( start, start + len);
    print!("unmap!!!");
    for area in &cur_tcb.memory_set.areas {
        if lvpn <= area.get_start() && rvpn >= area.get_start()
        {
            cnt += 1;
        }
    }
    if cnt < rvpn.0 - lvpn.0 {
        return -1;
    }

    cur_tcb.memory_set.remove_framed_area(start, start + len);
    cur_tcb.memory_set.clean_area();
    0
}

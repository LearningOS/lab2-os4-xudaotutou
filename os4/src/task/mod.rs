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
use crate::mm::{MapPermission, PageTable, PhysAddr, VirtAddr, VirtPageNum};
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
        self.set_task_time(next_task);
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
            // info!("next_task: app_{}",next);
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            self.set_task_time(&mut inner.tasks[next]);
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
            status: cur.task_status,
            syscall_times: cur.syscall_times,
            time: (get_time_us() - cur.start_time) / 1000,
        }
    }
    fn set_task_time(&self, cur: &mut TaskControlBlock) {
        if cur.start_time == 0 {
            cur.start_time = get_time_us();
        }
    }
    pub fn update_syscall_time(&self, id: usize) {
        if id < 500 {
            let mut inner = self.inner.exclusive_access();
            let key = inner.current_task;

            let cur = &mut inner.tasks[key];
            cur.syscall_times[id] += 1;
        }
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
pub fn update_syscall_time(id: usize) {
    TASK_MANAGER.update_syscall_time(id);
}
pub fn get_slice_buffer<'a, T>(start: usize) -> Option<&'a mut T> {
    // TASK_MANAGER.get_slice_buffer::<T>(start)
    let satp = TASK_MANAGER.get_current_token();
    let pt = PageTable::from_token(satp);
    let va = VirtAddr::from(start);
    let vpn = va.vpn();
    if let Some(pte) = pt.translate(vpn) {
        let ppn = pte.ppn();
        let pa = PhysAddr::from(PhysAddr::from(ppn).0 | va.page_offset());
        pa.get_mut::<T>()
    } else {
        None
    }
}

pub fn mmap(start: usize, len: usize, prot: usize) -> isize {
    if len == 0 {
        info!("reason1");
        return 0;
    }
    // if len % 4096 != 0 {
    //     return  -1;
    // }
    // 0???1???2??????????????????????????????0,mask => b 0...0111 =>0x7
    if (prot >> 3) != 0 || (prot & 0x7) == 0 || start % 4096 != 0 {
        info!("reason2");
        return -1;
    }
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let idx = inner.current_task;
    let cur_tcb = &mut inner.tasks[idx];
    let end = start + len;
    println!("mmap!!!");
    cur_tcb.memory_set.mmap(start, end,prot)
}

pub fn munmap(start: usize, len: usize) -> isize {
    if len == 0 {
        return 0;
    }
    if start % 4096 != 0 {
        return  -1;
    }
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let idx = inner.current_task;
    let cur_tcb = &mut inner.tasks[idx];
    cur_tcb.memory_set.munmap(start, start + len)
}

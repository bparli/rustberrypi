use crate::{exception, process};
extern crate alloc;
use alloc::collections::vec_deque::VecDeque;
use cortex_a::regs::*;
use process::{Task, TaskState};
use spin::Mutex;

pub struct GlobalScheduler(Mutex<Option<Scheduler>>);

pub static SCHEDULER: GlobalScheduler = GlobalScheduler::uninitialized();

impl GlobalScheduler {
    pub fn init(&self) {
        *self.0.lock() = Some(Scheduler::new());
    }
    /// Returns an uninitialized wrapper around a local scheduler.
    pub const fn uninitialized() -> GlobalScheduler {
        GlobalScheduler(Mutex::new(None))
    }

    /// Adds a process to the scheduler's queue and returns that process's ID.
    /// For more details, see the documentation on `Scheduler::add()`.
    pub fn add_task(&self, task: Task) -> Option<u64> {
        self.0
            .lock()
            .as_mut()
            .expect("scheduler uninitialized")
            .add_task(task)
    }

    /// Performs a context switch using `tf` by setting the state of the current
    /// process to `new_state`, saving `tf` into the current process, and
    /// restoring the next process's trap frame into `tf`. For more details, see
    /// the documentation on `Scheduler::switch()`.
    pub fn switch(&self, ec: &mut exception::ExceptionContext) -> Option<u64> {
        self.0
            .lock()
            .as_mut()
            .expect("scheduler uninitialized")
            .switch(ec)
    }

    pub fn timer_tick(&self, e: &mut exception::ExceptionContext) -> Option<u64> {
        exception::asynchronous::exec_with_irq_masked(|| self.switch(e))
    }
}

struct Scheduler {
    processes: VecDeque<Task>,
    current: Option<u64>,
    last_id: Option<u64>,
}

impl Scheduler {
    /// Returns a new `Scheduler` with an empty queue.
    pub fn new() -> Scheduler {
        Scheduler {
            processes: VecDeque::new(),
            current: None,
            last_id: None,
        }
    }

    /// Adds a process to the scheduler's queue and returns that process's ID if
    /// a new process can be scheduled. The process ID is newly allocated for
    /// the process and saved in its `trap_frame`. If no further processes can
    /// be scheduled, returns `None`.
    ///
    /// If this is the first process added, it is marked as the current process.
    /// It is the caller's responsibility to ensure that the first time `switch`
    /// is called, that process is executing on the CPU.
    fn add_task(&mut self, mut task: Task) -> Option<u64> {
        let id = self.last_id.get_or_insert(1);

        *id += 1;
        task.context.tpidr = *id;
        task.pid = task.context.tpidr;
        self.processes.push_back(task);

        if let None = self.current {
            self.current = Some(*id);
        }

        Some(*id)
    }

    /// Sets the current process's state to `new_state`, finds the next process
    /// to switch to, and performs the context switch on `tf` by saving `tf`
    /// into the current process and restoring the next process's trap frame
    /// into `tf`. If there is no current process, returns `None`. Otherwise,
    /// returns `Some` of the process ID that was context switched into `tf`.
    ///
    /// This method blocks until there is a process to switch to, conserving
    /// energy as much as possible in the interim.
    fn switch(&mut self, ec: &mut exception::ExceptionContext) -> Option<u64> {
        if self.processes.len() < 2 {
            return None;
        }

        // must be the real init proc
        if self.current != Some(ec.tpidr) {
            let mut t = process::Task::new().unwrap();
            t.pid = 1;
            t.state = TaskState::RUNNING;
            *t.context = *ec;
            t.context.sp = t.stack.top().as_u64();
            t.context.tpidr = 1;
            self.current = Some(ec.tpidr);
            self.processes.push_front(t);
            return None;
        }

        if let Some(task) = self.processes.front_mut() {
            task.counter -= 1;
            if task.counter <= 0 {
                let mut task = self.processes.pop_front().unwrap();
                task.counter = 1;
                task.state = TaskState::READY;
                *task.context = *ec;
                self.flush_tlb(&task.stack);
                self.processes.push_back(task);
            } else {
                return None;
            }
        }

        loop {
            let num_tasks = self.processes.len();
            for _ in 0..num_tasks {
                let mut new_task = self.processes.pop_front().unwrap();
                if new_task.state == TaskState::READY {
                    *ec = *new_task.context;
                    new_task.state = TaskState::RUNNING;
                    self.current = Some(ec.tpidr);
                    self.processes.push_front(new_task);
                    return self.current;
                } else {
                    new_task.counter = (new_task.counter >> 1) + new_task.priority;
                    self.processes.push_back(new_task);
                }
            }
            unsafe { asm!("wfi") }
        }
    }

    fn flush_tlb(&self, st: &process::Stack) {
        unsafe {
            if st.bottom().as_u64() != TTBR1_EL1.get() {
                llvm_asm! {"
                    msr	ttbr1_el1, $0
                    tlbi vmalle1is
                    DSB ISH
                    isb
                "
                ::   "r"(st.bottom().as_u64())
                }
            }
        }
    }
}

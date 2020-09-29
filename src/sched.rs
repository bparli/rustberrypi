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

    pub fn exit_task(&self, ec: &mut exception::ExceptionContext) {
        self.0
            .lock()
            .as_mut()
            .expect("scheduler uninitialized")
            .exit_task(ec);
        // now find new trask to run on this core
        loop {
            {
                if self
                    .0
                    .lock()
                    .as_mut()
                    .expect("scheduler uninitialized")
                    .schedule(ec)
                    > 0
                {
                    break;
                }
            }
        }
    }

    /// Performs a context switch using `tf` by setting the state of the current
    /// process to `new_state`, saving `tf` into the current process, and
    /// restoring the next process's trap frame into `tf`. For more details, see
    /// the documentation on `Scheduler::switch()`.
    pub fn switch(&self, update_state: TaskState, ec: &mut exception::ExceptionContext) {
        let mut sched = false;
        {
            sched = self
                .0
                .lock()
                .as_mut()
                .expect("scheduler uninitialized")
                .deschedule(update_state, ec);
        }
        // now find new trask to run on this core
        if sched {
            loop {
                {
                    if self
                        .0
                        .lock()
                        .as_mut()
                        .expect("scheduler uninitialized")
                        .schedule(ec)
                        > 0
                    {
                        break;
                    }
                }
            }
        }
    }

    pub fn timer_tick(&self, e: &mut exception::ExceptionContext) {
        exception::asynchronous::exec_with_irq_masked(|| self.switch(TaskState::READY, e))
    }
}

struct Scheduler {
    processes: VecDeque<Task>,
    last_id: Option<u64>,
}

impl Scheduler {
    /// Returns a new `Scheduler` with an empty queue.
    pub fn new() -> Scheduler {
        Scheduler {
            processes: VecDeque::new(),
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
    fn deschedule(
        &mut self,
        update_state: TaskState,
        ec: &mut exception::ExceptionContext,
    ) -> bool {
        // find the task currently running on this core and
        // decrement the counter on running task once its found

        for (ind, tsk) in self.processes.iter_mut().enumerate() {
            if tsk.pid == ec.tpidr {
                tsk.counter -= 1;
                match update_state {
                    TaskState::READY => {
                        if tsk.counter > 0 {
                            return false;
                        }
                    }
                    _ => {}
                }
                // times up, deschedule running task
                if let Some(mut running) = self.processes.remove(ind) {
                    running.counter = 1;
                    running.state = update_state;
                    *running.context = *ec;
                    flush_tlb(&running.stack);
                    self.processes.push_back(running);
                }
                break;
            }
        }
        return true;
    }

    fn schedule(&mut self, ec: &mut exception::ExceptionContext) -> u64 {
        let num_tasks = self.processes.len();
        for _ in 0..num_tasks {
            let mut new_task = self.processes.pop_front().unwrap();
            if new_task.is_ready() {
                let pid = ec.tpidr;
                *ec = *new_task.context;
                new_task.state = TaskState::RUNNING;
                self.processes.push_front(new_task);
                return pid;
            } else if new_task.is_waiting() {
                new_task.counter = (new_task.counter >> 1) + new_task.priority;
            }
            self.processes.push_back(new_task);
        }
        return 0;
    }

    fn exit_task(&mut self, ec: &mut exception::ExceptionContext) {
        for task in self.processes.iter_mut() {
            if task.pid == ec.tpidr {
                // clean up task, dealloc stack
                task.exit();
                break;
            }
        }
        self.deschedule(TaskState::ZOMBIE, ec);
    }
}

fn flush_tlb(st: &process::Stack) {
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

use crate::bsp::generic_timer;
use crate::exception::{self, ExceptionContext};
use crate::process::{Task, TaskState};
use crate::sched::SCHEDULER;
use alloc::boxed::Box;
use core::time::Duration;

fn sleep_task(ms: u64, ec: &mut ExceptionContext) {
    let timer = generic_timer();
    let begin = timer.current_time();
    let target_time = begin + Duration::from_millis(ms as u64);
    let polling_fn = Box::new(move |task: &mut Task| {
        let current = timer.current_time();
        if current > target_time {
            task.context.gpr[7] = 0; // x7 = 0; succeed
            task.context.gpr[0] = (current - begin).as_millis() as u64; // x0 = elapsed time in ms
            true
        } else {
            false
        }
    });

    exception::asynchronous::exec_with_irq_masked(|| {
        SCHEDULER.switch(TaskState::WAITING(polling_fn), ec)
    })
}

fn exit_task(ec: &mut ExceptionContext) {
    exception::asynchronous::exec_with_irq_masked(|| SCHEDULER.exit_task(ec))
}

pub fn handle(ec: &mut ExceptionContext) -> Result<(), &str> {
    match ec.gpr[8] {
        1 => {
            // Sleep syscall
            exception::asynchronous::exec_with_irq_masked(|| sleep_task(ec.gpr[0], ec));
            Ok(())
        }
        2 => {
            // Exit syscall
            exit_task(ec);
            Ok(())
        }
        _ => Err("does not exist"),
    }
}

pub fn sleep(time: u64) {
    unsafe {
        llvm_asm! {"
                mov w8, 1
                mov x0, $0
                svc #0
                ret
            "
        ::   "r"(time)
        }
    }
}

pub fn exit() {
    unsafe {
        llvm_asm! {"
                mov w8, 2
                svc #0
                ret
            "
        }
    }
}

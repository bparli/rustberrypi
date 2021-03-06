use crate::exception::ExceptionContext;
use crate::memory::ALLOCATOR;
use crate::sched::SCHEDULER;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use core::fmt;
use core::mem::replace;
use core::ptr::{NonNull, Unique};

#[repr(C)]
pub struct Task {
    pub context: Box<ExceptionContext>,
    pub state: TaskState,
    pub counter: i8,
    pub priority: i8,
    pub pid: u64,
    pub stack: Stack,
}

/// Type of a function used to determine if a task is ready to be scheduled
/// again. The scheduler calls this function when it is the task's turn to
/// execute. If the function returns `true`, the task is scheduled. If it
/// returns `false`, the task is not scheduled, and this function will be
/// called on the next time slice.
pub type EventPollFn = Box<dyn FnMut(&mut Task) -> bool + Send>;

#[repr(C)]
pub enum TaskState {
    RUNNING,
    WAITING(EventPollFn),
    READY,
    ZOMBIE,
}

impl Task {
    pub fn new() -> Option<Task> {
        match Stack::new() {
            Some(stack) => Some(Task {
                context: Box::new(ExceptionContext::default()),
                state: TaskState::READY,
                counter: 0,
                priority: 1,
                pid: 0,
                stack: stack,
            }),
            None => None,
        }
    }

    pub fn is_ready(&mut self) -> bool {
        match self.state {
            TaskState::READY => true,
            TaskState::RUNNING => false,
            TaskState::ZOMBIE => false,
            TaskState::WAITING(_) => {
                let mut current_state = replace(&mut self.state, TaskState::READY);
                let current_ready = match current_state {
                    TaskState::WAITING(ref mut event_pol_fn) => event_pol_fn(self),
                    TaskState::READY => true,
                    _ => false,
                };
                if !current_ready {
                    self.state = current_state;
                }
                current_ready
            }
            _ => false,
        }
    }

    pub fn is_waiting(&mut self) -> bool {
        match self.state {
            TaskState::WAITING(_) => true,
            _ => false,
        }
    }

    pub fn is_running(&mut self) -> bool {
        match self.state {
            TaskState::RUNNING => true,
            _ => false,
        }
    }

    pub fn exit(&mut self) {
        self.state = TaskState::ZOMBIE;
        self.counter = 0;
        self.priority = 0;
        unsafe {
            (&ALLOCATOR).lock().deallocate(
                NonNull::new(self.stack.as_mut_ptr()).expect("non-null"),
                Stack::layout(),
            );
        }
    }
}

/// A task stack. The default size is 4kb with an alignment of 16 bytes.
pub struct Stack {
    ptr: Unique<[u8; Stack::SIZE]>,
}

impl Stack {
    /// The default stack size is 64kb.
    pub const SIZE: usize = 1 << 12;

    /// The default stack alignment is 16 bytes.
    pub const ALIGN: usize = 16;

    /// The default layout for a stack.
    pub fn layout() -> Layout {
        Layout::from_size_align(Self::SIZE, Self::ALIGN).unwrap()
    }

    /// Returns a newly allocated process stack, zeroed out, if one could be
    /// successfully allocated. If there is no memory, or memory allocation
    /// fails for some other reason, returns `None`.
    pub fn new() -> Option<Stack> {
        let raw_ptr = unsafe {
            let raw_ptr: *mut u8 = (&ALLOCATOR)
                .lock()
                .allocate_first_fit(Stack::layout())
                .expect("Out of Memory I guess")
                .as_ptr();
            raw_ptr.write_bytes(0, Self::SIZE);
            raw_ptr
        };

        let ptr = Unique::new(raw_ptr as *mut _).expect("non-null");
        Some(Stack { ptr })
    }

    /// Internal method to cast to a `*mut u8`.
    unsafe fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr() as _
    }

    /// Returns the physical address of top of the stack.
    pub fn top(&self) -> PhysicalAddr {
        unsafe { self.as_mut_ptr().add(Self::SIZE).into() }
    }

    /// Returns the physical address of bottom of the stack.
    pub fn bottom(&self) -> PhysicalAddr {
        unsafe { self.as_mut_ptr().into() }
    }
}

impl fmt::Debug for Stack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Stack")
            .field("top", &self.top())
            .field("bottom", &self.bottom())
            .field("size", &Self::SIZE)
            .finish()
    }
}

/// A physical address.
#[derive(Debug)]
pub struct PhysicalAddr(usize);

macro_rules! impl_for {
    ($T:tt) => {
        impl<T: Sized> From<*mut T> for $T {
            fn from(raw_ptr: *mut T) -> $T {
                $T(raw_ptr as usize)
            }
        }

        impl $T {
            /// Returns the inner address of `self`.
            pub fn as_ptr(&self) -> *const u8 {
                self.0 as *const u8
            }

            /// Returns the inner address of `self`.
            ///
            /// # Safety
            ///
            /// This method is marked `unsafe` because it can be used to create
            /// multiple mutable aliases to the address represented by `self`. The
            /// caller must ensure that they do not alias.
            pub fn as_mut_ptr(&mut self) -> *mut u8 {
                self.0 as *mut u8
            }

            /// Returns the inner address of `self` as a `usize`.
            pub fn as_usize(&self) -> usize {
                self.0
            }

            /// Returns the inner address of `self` as a `u64`.
            #[cfg(target_pointer_width = "64")]
            pub fn as_u64(&self) -> u64 {
                self.0 as u64
            }
        }
    };
}

impl_for!(PhysicalAddr);

pub fn add_user_process(entry: fn()) {
    add_process(entry, 0b0100); // EL0
}

pub fn add_kernel_process(entry: fn()) {
    add_process(entry, 0b0101); // EL1
}

fn add_process(entry: fn(), spsr: u64) {
    let mut task = Task::new().unwrap();
    task.context.sp = task.stack.bottom().as_u64();
    task.context.elr = entry as *mut u8 as u64;
    task.context.spsr = spsr;
    SCHEDULER.add_task(task).unwrap();
}

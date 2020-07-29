use crate::exception::ExceptionContext;
//use crate::info;
use crate::memory::ALLOCATOR;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use core::fmt;
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

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub enum TaskState {
    RUNNING,
    READY,
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
}

/// A task stack. The default size is 4kb with an alignment of 16 bytes.
pub struct Stack {
    ptr: Unique<[u8; Stack::SIZE]>,
}

impl Stack {
    /// The default stack size is 4kb.
    pub const SIZE: usize = 1 << 12;

    /// The default stack alignment is 16 bytes.
    pub const ALIGN: usize = 16;

    /// The default layout for a stack.
    fn layout() -> Layout {
        unsafe { Layout::from_size_align_unchecked(Self::SIZE, Self::ALIGN) }
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

impl Drop for Stack {
    fn drop(&mut self) {
        unsafe {
            (&ALLOCATOR).lock().deallocate(
                NonNull::new(self.as_mut_ptr()).expect("non-null"),
                Self::layout(),
            );
        }
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

// pub fn copy_process(entry: fn(arg: &str), arg: &str) -> i8 {
//     unsafe {
//         let layout = Layout::from_size_align_unchecked(4096, 16);
//         let task = alloc_zeroed(layout);

//         *(task as *mut Task) = init_task();

//         preempt_disable();

//         NUM_TASKS += 1;
//         let mut t = *(task as *mut Task);
//         t.priority = CURRENT.priority;
//         t.state = TaskState::RUNNING;
//         t.counter = t.priority;
//         t.ptr = task;
//         t.preempt_count = 1;

//         t.context.x19 = entry as *mut u8;
//         t.context.x20 = arg.as_ptr() as *mut u8;
//         t.context.pc = return_from_fork as *mut u8;
//         t.context.sp = t.ptr.offset(4096);
//         t.pid = NUM_TASKS - 1;
//         TASKS[NUM_TASKS - 1] = t;
//         preempt_enable();

//         info!(
//             "forked proc {:?}, {:?}, {:?}, {:?}",
//             t.pid, t.context.sp, t.ptr, task
//         );
//         return 0;
//     }
// }

// fn return_from_fork() {
//     info!("TEST ret from fork");
//     unsafe {
//         preempt_enable();
//         info!("TEST ret from fork preempt enabled");
//         asm! {"
//             mov    x0, x20
//             blr    x19
//         "};
//     }
//     info!("TEST ret from fork DONE");
// }

#![allow(incomplete_features)]
#![feature(asm)]
#![feature(const_fn)]
#![feature(const_generics)]
#![feature(const_panic)]
#![feature(core_intrinsics)]
#![feature(format_args_nl)]
#![feature(global_asm)]
#![feature(linkage)]
#![feature(naked_functions)]
#![feature(panic_info_message)]
#![feature(slice_ptr_range)]
#![feature(trait_alias)]
#![no_std]
#![feature(ptr_internals)]
#![feature(llvm_asm)]
#![feature(negative_impls)]
// Testing
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::test_runner)]
#![feature(alloc_error_handler)]

// `mod cpu` provides the `_start()` function, the first function to run. `_start()` then calls
// `runtime_init()`, which jumps to `kernel_init()` (defined in `main.rs`).

mod panic_wait;
mod runtime_init;

pub mod bsp;
pub mod console;
pub mod cpu;
pub mod driver;
pub mod exception;
pub mod memory;
pub mod net;
pub mod print;
pub mod process;
pub mod sched;
pub mod syscall;

extern crate alloc;

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

/// The default runner for unit tests.
pub fn test_runner(tests: &[&test_types::UnitTest]) {
    println!("Running {} tests", tests.len());
    println!("-------------------------------------------------------------------\n");
    for (i, test) in tests.iter().enumerate() {
        print!("{:>3}. {:.<58}", i + 1, test.name);

        // Run the actual test.
        (test.test_func)();

        // Failed tests call panic!(). Execution reaches here only if the test has passed.
        println!("[ok]")
    }
}

/// The `kernel_init()` for unit tests. Called from `runtime_init()`.
#[cfg(test)]
#[no_mangle]
unsafe fn kernel_init() -> ! {
    use memory::{heap_map, ALLOCATOR};
    extern crate alloc;
    let (heap_start, heap_end) = heap_map().expect("failed to derive heap map");
    ALLOCATOR.lock().init(heap_start, heap_end - heap_start);
    bsp::qemu_bring_up_console();

    test_main();

    cpu::qemu_exit_success()
}

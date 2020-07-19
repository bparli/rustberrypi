//! The `kernel` binary.

#![feature(format_args_nl)]
#![no_main]
#![no_std]

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use libkernel::{bsp, cpu, driver, exception, info, memory, state, time, warn};
use linked_list_allocator::LockedHeap;
extern crate alloc;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Early init code.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order:
///     - Virtual memory must be activated before the device drivers.
///       - Without it, any atomic operations, e.g. the yet-to-be-introduced spinlocks in the device
///         drivers (which currently employ IRQSafeNullLocks instead of spinlocks), will fail to
///         work on the RPi SoCs.
#[no_mangle]
unsafe fn kernel_init() -> ! {
    use driver::interface::DriverManager;
    //use memory::mmu::interface::MMU;

    exception::handling_init();

    if let Err(string) = memory::mmu::init() {
        panic!("MMU: {}", string);
    }

    for i in bsp::driver::driver_manager().all_device_drivers().iter() {
        if i.init().is_err() {
            panic!("Error loading driver: {}", i.compatible())
        }
    }
    bsp::driver::driver_manager().post_device_driver_init();
    // println! is usable from here on.

    // Let device drivers register and enable their handlers with the interrupt controller.
    for i in bsp::driver::driver_manager().all_device_drivers() {
        if let Err(msg) = i.register_and_enable_irq_handler() {
            warn!("Error registering IRQ handler: {}", msg);
        }
    }

    // Unmask interrupts on the boot CPU core.
    exception::asynchronous::local_irq_unmask();

    // Announce conclusion of the kernel_init() phase.
    state::state_manager().transition_to_single_core_main();

    ALLOCATOR
        .lock()
        .init(memory::map::virt::HEAP_START, memory::heap_size());

    // Transition from unsafe to safe.
    kernel_main()
}

/// The main function running after the early init.
fn kernel_main() -> ! {
    use driver::interface::DriverManager;
    use exception::asynchronous::interface::IRQManager;

    info!("Booting on: {}", bsp::board_name());

    info!("MMU online. Special regions:");
    memory::print_layout();

    let (_, privilege_level) = exception::current_privilege_level();
    info!("Current privilege level: {}", privilege_level);

    info!("Exception handling state:");
    exception::asynchronous::print_state();

    info!(
        "Architectural timer resolution: {} ns",
        time::time_manager().resolution().as_nanos()
    );

    info!("Drivers loaded:");
    for (i, driver) in bsp::driver::driver_manager()
        .all_device_drivers()
        .iter()
        .enumerate()
    {
        info!("      {}. {}", i + 1, driver.compatible());
    }

    info!("Registered IRQ handlers:");
    bsp::exception::asynchronous::irq_manager().print_handler();

    // allocate a number on the heap
    let heap_value = Box::new(41);
    info!("heap_value at {:p}", heap_value);

    // create a dynamically sized vector
    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    info!("vec at {:p}", vec.as_slice());

    // create a reference counted vector -> will be freed when count reaches 0
    let reference_counted = Rc::new(vec![1, 2, 3]);
    let cloned_reference = reference_counted.clone();
    info!(
        "current reference count is {}",
        Rc::strong_count(&cloned_reference),
    );
    core::mem::drop(reference_counted);
    info!(
        "reference count is {} now",
        Rc::strong_count(&cloned_reference)
    );

    info!("Echoing input now");
    cpu::wait_forever();
}

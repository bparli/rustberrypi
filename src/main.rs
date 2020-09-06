//! The `kernel` binary.

#![feature(format_args_nl)]
#![no_main]
#![no_std]

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use cortex_a::asm;
use libkernel::{bsp, cpu, driver, exception, info, memory, process, sched, syscall, time, warn};
extern crate alloc;
use core::time::Duration;
use memory::ALLOCATOR;
use sched::SCHEDULER;

// Early init code.
#[no_mangle]
unsafe fn kernel_init() -> ! {
    use driver::interface::DriverManager;

    exception::asynchronous::local_fiq_mask();
    exception::asynchronous::local_irq_mask();

    if let Err(string) = memory::mmu::init() {
        panic!("MMU: {}", string);
    }

    // still not working yet for some reason.  can't transition secondary cores to EL1
    //cpu::wake_up_secondary_cores();

    // enable the core's mmu
    memory::mmu::core_setup();
    
    for i in bsp::driver::driver_manager().all_device_drivers().iter() {
        if i.init().is_err() {
            panic!("Error loading driver: {}", i.compatible())
        }
    }

    bsp::driver::driver_manager().post_device_driver_init();
    //println! is usable from here on.

    // Let device drivers register and enable their handlers with the interrupt controller.
    for i in bsp::driver::driver_manager().all_device_drivers() {
        if let Err(msg) = i.register_and_enable_irq_handler() {
            warn!("Error registering IRQ handler: {}", msg);
        }
    }

    ALLOCATOR
        .lock()
        .init(memory::map::virt::HEAP_START, memory::heap_size());

    // Unmask interrupts on the boot CPU core.
    exception::asynchronous::local_irq_unmask();
    exception::asynchronous::local_fiq_unmask();

    SCHEDULER.init();

    asm::sev();

    kernel_main()
}

// The main function running after the early init.
fn kernel_main() -> ! {
    use driver::interface::DriverManager;
    use exception::asynchronous::interface::IRQManager;

    //time::time_manager().spin_for(Duration::from_secs(2));
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

    process::add_user_process(process1);
    process::add_user_process(process2);
    process::add_user_process(process4);
    process::add_kernel_process(process3);
    loop {}
}

fn process1() {
    loop {
        //info!("forked proc numero uno");
        syscall::sleep(2000);
    }
}

fn process4() {
    loop {
        //info!("forked proc numero uno");
        time::time_manager().spin_for(Duration::from_secs(2));
    }
}

fn process2() {
    for _num in 0..3 {
        //info!("forked proc dos");
        syscall::sleep(2000);
    }

    info!("forked proc dos is exiting");
    syscall::exit();
}

fn process3() {
    loop {
        info!("forked kernel proc {}", cpu::core_id::<usize>());
        asm::sev();
        time::time_manager().spin_for(Duration::from_secs(2));
    }
}

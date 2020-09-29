#![feature(format_args_nl)]
#![no_main]
#![no_std]

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use libkernel::{bsp, cpu, driver, exception, info, memory, process, sched, syscall, warn};
extern crate alloc;
use memory::ALLOCATOR;
use sched::SCHEDULER;

static CORE0_TIMER: bsp::device_driver::LocalTimer = unsafe {
    bsp::device_driver::LocalTimer::new(bsp::exception::asynchronous::irq_map::LOCAL_TIMER)
};

// Early init code.
#[no_mangle]
unsafe fn kernel_init() -> ! {
    use driver::interface::DriverManager;
    use memory::mmu::interface::MMU;

    if let Err(string) = memory::mmu::mmu().init() {
        panic!("MMU: {}", string);
    }

    // finally working
    cpu::wake_up_secondary_cores();

    // enable the core's mmu
    memory::mmu::core_setup();

    for i in bsp::driver::driver_manager().all_device_drivers().iter() {
        if i.init().is_err() {
            panic!("Error loading driver: {}", i.compatible())
        }
    }

    bsp::driver::driver_manager().post_device_driver_init();
    //println! is usable from here on.

    //Let device drivers register and enable their handlers with the interrupt controller.
    for i in bsp::driver::driver_manager().all_device_drivers() {
        if let Err(msg) = i.register_and_enable_irq_handler() {
            warn!("Error registering IRQ handler: {}", msg);
        }
    }

    let (_, privilege_level) = exception::current_privilege_level();
    info!("Current privilege level: {}", privilege_level);

    if let Err(mssg) = CORE0_TIMER.register_and_enable_irq_handler() {
        warn!("Error registering IRQ handler: {}", mssg);
    }

    let (heap_start, heap_end) = memory::heap_map().expect("failed to derive heap map");
    ALLOCATOR.lock().init(heap_start, heap_end - heap_start);

    // Unmask interrupts on the boot CPU core.
    exception::asynchronous::local_irq_unmask();

    SCHEDULER.init();

    kernel_main()
}

// The main function running after the early init.
fn kernel_main() -> ! {
    use driver::interface::DriverManager;
    use exception::asynchronous::interface::IRQManager;

    info!("Booting on: {}", bsp::board_name());

    info!("MMU online. Special regions:");
    memory::virt_mem_layout().print_layout();

    let (_, privilege_level) = exception::current_privilege_level();
    info!("Current privilege level: {}", privilege_level);

    info!("Exception handling state:");
    exception::asynchronous::print_state();

    info!(
        "Architectural timer resolution: {} ns",
        //time::time_manager().resolution().as_nanos()
        CORE0_TIMER.resolution().as_nanos()
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

    for _ in 0..=5 {
        process::add_user_process(process1);
    }
    process::add_user_process(process4);
    process::add_user_process(process2);
    process::add_kernel_process(process3);
    loop {}
}

static mut PROC_NUM: i32 = 1;
fn process1() {
    unsafe {
        let my_proc = PROC_NUM;
        PROC_NUM += 1;
        loop {
            info!(
                "forked proc numero uno {} {}",
                cpu::core_id::<usize>(),
                my_proc
            );
            syscall::sleep(3500);
        }
    }
}

fn process4() {
    loop {
        info!("forked proc numero quatro {} ", cpu::core_id::<usize>());
        syscall::sleep(2000);
    }
}

fn process2() {
    for _ in 0..=5 {
        info!("forked proc dos {} ", cpu::core_id::<usize>());
        syscall::sleep(2000);
    }

    info!("forked proc dos is exiting");
    syscall::exit();
}

fn process3() {
    loop {
        info!("forked kernel proc {}", cpu::core_id::<usize>());
        cpu::spin_for_cycles(2000000000)
    }
}

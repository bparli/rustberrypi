//use crate::info;
use crate::runtime_init;
use core::ptr::{read_volatile, write_volatile};
use cortex_a::{asm, regs::*};

/// Used by `arch` code to find the early boot core.
pub const BOOT_CORE_ID: usize = 0;

/// The early boot core's stack address.
pub const BOOT_CORE_STACK_START: u64 = 0x80_000;

/// The number of processor cores.
pub const NUM_CORES: usize = 4;

/// The base of physical addresses that each core is spinning on
pub const SPINNING_BASE: *mut usize = 0xd8 as *mut usize;

global_asm!(include_str!("exception.S"));

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Return the executing core's id.
#[inline(always)]
pub fn core_id<T>() -> T
where
    T: From<u8>,
{
    const CORE_MASK: u64 = 0b11;

    T::from((MPIDR_EL1.get() & CORE_MASK) as u8)
}

//--------------------------------------------------------------------------------------------------
// Boot Code
//--------------------------------------------------------------------------------------------------

/// The entry of the `kernel` binary.
///
/// The function must be named `_start`, because the linker is looking for this exact name.
///
/// # Safety
///
/// - Linker script must ensure to place this function at `0x80_000`.
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
    if BOOT_CORE_ID == core_id() {
        SP.set(BOOT_CORE_STACK_START);
        kinit()
    }
    start2()
}

#[no_mangle]
unsafe fn kinit() -> ! {
    extern "Rust" {
        fn kernel_init() -> !;
    }
    runtime_init::zero_bss();
    el3_to_el2();
    el2_to_el1();
    kernel_init()
}

// Transition from EL2 to EL1.
#[no_mangle]
unsafe fn el2_to_el1() {
    extern "C" {
        static mut __exception_vector_start: u64;
    }

    if CurrentEL.get() == CurrentEL::EL::EL2.value {
        // set the stack-pointer for EL1
        SP_EL1.set(SP.get() as u64);

        // Enable timer counter registers for EL1.
        CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

        // No offset for reading the counters.
        CNTVOFF_EL2.set(0);

        // Set EL1 execution state to AArch64.
        HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64);

        // Set SCTLR to known state
        runtime_init::SCTLR_EL1.set(runtime_init::SCTLR_EL1::RES1);

        VBAR_EL1.set(&__exception_vector_start as *const _ as u64);

        // Set up a simulated exception return.
        //
        // First, fake a saved program status where all interrupts were masked and SP_EL1 was used as a
        // stack pointer.
        SPSR_EL2.write(
            SPSR_EL2::D::Masked
                + SPSR_EL2::A::Masked
                + SPSR_EL2::I::Masked
                + SPSR_EL2::F::Masked
                + SPSR_EL2::M::EL1h,
        );

        // eret to itself, expecting current_el() == 1 this time.
        ELR_EL2.set(el2_to_el1 as *const () as u64);
        asm::eret();
    }
}

#[no_mangle]
unsafe fn el3_to_el2() {
    if CurrentEL.get() == CurrentEL::EL::EL3.value {
        // set up Secure Configuration Register (D13.2.10)
        runtime_init::SCR_EL3.set(
            runtime_init::SCR_EL3::NS
                | runtime_init::SCR_EL3::SMD
                | runtime_init::SCR_EL3::HCE
                | runtime_init::SCR_EL3::RW
                | runtime_init::SCR_EL3::RES1,
        );

        // set up Saved Program Status Regiser (C5.2.19)
        runtime_init::SPSR_EL3.set(
            (runtime_init::SPSR_EL3::M & 0b1001)
                | runtime_init::SPSR_EL3::F
                | runtime_init::SPSR_EL3::I
                | runtime_init::SPSR_EL3::A
                | runtime_init::SPSR_EL3::D,
        );

        // eret to itself,EL == 2 this time.
        runtime_init::ELR_EL3.set(el3_to_el2 as *const () as u64);
        asm::eret();
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

pub use asm::nop;

/// Spin for `n` cycles.
#[inline(always)]
pub fn spin_for_cycles(n: usize) {
    for _ in 0..n {
        asm::nop();
    }
}

pub unsafe fn wake_up_secondary_cores() {
    for core_index in 1..=3 {
        let core_spin_ptr = SPINNING_BASE.add(core_index);
        write_volatile(core_spin_ptr, start2 as *const () as usize);
    }
    asm::sev();
    for core_index in 1..=3 {
        let core_spin_ptr = SPINNING_BASE.add(core_index);
        while read_volatile(core_spin_ptr as *const usize) != 0 {
            //spin
        }
    }
}

/// Pause execution on the core.
pub fn wait_forever() -> ! {
    loop {
        asm::wfe();
    }
}

#[no_mangle]
#[naked]
unsafe extern "C" fn start2() -> ! {
    SP.set(BOOT_CORE_STACK_START - (4096 * core_id::<usize>() as u64));
    el3_to_el2();
    el2_to_el1();
    kmain2()
}

unsafe fn kmain2() -> ! {
    use crate::memory;
    write_volatile(SPINNING_BASE.add(core_id::<usize>()), 0);
    memory::mmu::core_setup();
    loop {
        asm::wfe();
    }
}

//------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

/// Make the host QEMU binary execute `exit(1)`.
pub fn qemu_exit_failure() -> ! {
    qemu_exit::aarch64::exit_failure()
}

/// Make the host QEMU binary execute `exit(0)`.
pub fn qemu_exit_success() -> ! {
    qemu_exit::aarch64::exit_success()
}

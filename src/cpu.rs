use cortex_a::{asm, regs::*};

/// Used by `arch` code to find the early boot core.
pub const BOOT_CORE_ID: usize = 0;

/// The early boot core's stack address.
pub const BOOT_CORE_STACK_START: u64 = 0x80_000;

/// The number of processor cores.
pub const NUM_CORES: usize = 4;

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
#[naked]
#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // Expect the boot core to start in EL2.
    if (BOOT_CORE_ID == core_id()) && (CurrentEL.get() == CurrentEL::EL::EL2.value) {
        el2_to_el1_transition()
    } else {
        // If not core0, infinitely wait for events.
        wait_forever()
    }
}

/// Transition from EL2 to EL1.
///
/// # Safety
///
/// - The HW state of EL1 must be prepared in a sound way.
/// - Exception return from EL2 must must continue execution in EL1 with
///   `runtime_init::runtime_init()`.
#[inline(always)]
unsafe fn el2_to_el1_transition() -> ! {
    use crate::runtime_init;

    // Enable timer counter registers for EL1.
    CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

    // No offset for reading the counters.
    CNTVOFF_EL2.set(0);

    // Set EL1 execution state to AArch64.
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64);

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

    // Second, let the link register point to runtime_init().
    ELR_EL2.set(runtime_init::runtime_init as *const () as u64);

    // Set up SP_EL1 (stack pointer), which will be used by EL1 once we "return" to it.
    SP_EL1.set(BOOT_CORE_STACK_START);

    // Use `eret` to "return" to EL1. This results in execution of runtime_init() in EL1.
    asm::eret()
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

/// Pause execution on the core.
#[inline(always)]
pub fn wait_forever() -> ! {
    loop {
        asm::wfe()
    }
}

//--------------------------------------------------------------------------------------------------
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

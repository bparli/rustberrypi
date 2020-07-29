pub mod asynchronous;

/// Kernel privilege levels.
#[allow(missing_docs)]
#[derive(PartialEq)]
pub enum PrivilegeLevel {
    User,
    Kernel,
    Hypervisor,
    Unknown,
}

use crate::{bsp, exception};
use core::fmt;
use cortex_a::{barrier, regs::*};

// Assembly counterpart to this file.
global_asm!(include_str!("exception.S"));

/// The exception context as it is stored on the stack on exception entry.
#[derive(Default, Copy, Clone)]
#[repr(C)]
pub struct ExceptionContext {
    /// General Purpose Registers.
    pub gpr: [u64; 30],

    pub tpidr: u64,
    pub sp: u64,

    /// The link register, aka x30.
    pub lr: u64,

    /// Exception link register. The program counter at the time the exception happened.
    pub elr: u64,

    /// Saved program status.
    pub spsr: u64,
}

/// Wrapper struct for pretty printing ESR_EL1.
struct EsrEL1;

/// Print verbose information about the exception and the panic.
fn default_exception_handler(e: &ExceptionContext) {
    panic!(
        "\n\nCPU Exception!\n\
         FAR_EL1: {:#018x}\n\
         {}\n\
         {}",
        FAR_EL1.get(),
        EsrEL1 {},
        e
    );
}

//------------------------------------------------------------------------------
// Current, EL0
//------------------------------------------------------------------------------

#[no_mangle]
unsafe extern "C" fn current_el0_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
unsafe extern "C" fn current_el0_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
unsafe extern "C" fn current_el0_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Current, ELx
//------------------------------------------------------------------------------

#[no_mangle]
unsafe extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
unsafe extern "C" fn current_elx_irq(e: &mut ExceptionContext) {
    use exception::asynchronous::interface::IRQManager;

    let token = &exception::asynchronous::IRQContext::new();
    bsp::exception::asynchronous::irq_manager().handle_pending_irqs(token, e);
}

#[no_mangle]
unsafe extern "C" fn current_elx_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch64
//------------------------------------------------------------------------------

#[no_mangle]
unsafe extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
unsafe extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
unsafe extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch32
//------------------------------------------------------------------------------

#[no_mangle]
unsafe extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
unsafe extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
unsafe extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Pretty printing
//------------------------------------------------------------------------------

/// Human readable ESR_EL1.
#[rustfmt::skip]
impl fmt::Display for EsrEL1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let esr_el1 = ESR_EL1.extract();

        // Raw print of whole register.
        writeln!(f, "ESR_EL1: {:#010x}", esr_el1.get())?;

        // Raw print of exception class.
        write!(f, "      Exception Class         (EC) : {:#x}", esr_el1.read(ESR_EL1::EC))?;

        // Exception class, translation.
        let ec_translation = match esr_el1.read_as_enum(ESR_EL1::EC) {
            Some(ESR_EL1::EC::Value::DataAbortCurrentEL) => "Data Abort, current EL",
            _ => "N/A",
        };
        writeln!(f, " - {}", ec_translation)?;

        // Raw print of instruction specific syndrome.
        write!(f, "      Instr Specific Syndrome (ISS): {:#x}", esr_el1.read(ESR_EL1::ISS))?;

        Ok(())
    }
}

/// Human readable print of the exception context.
impl fmt::Display for ExceptionContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "ELR: {:#018x}", self.elr)?;
        writeln!(f, "{}", self.spsr)?;
        writeln!(f)?;
        writeln!(f, "General purpose register:")?;

        #[rustfmt::skip]
        let alternating = |x| -> _ {
            if x % 2 == 0 { "   " } else { "\n" }
        };

        // Print two registers per line.
        for (i, reg) in self.gpr.iter().enumerate() {
            write!(f, "      x{: <2}: {: >#018x}{}", i, reg, alternating(i))?;
        }
        write!(f, "      lr : {:#018x}", self.lr)?;

        Ok(())
    }
}

/// The processing element's current privilege level.
pub fn current_privilege_level() -> (PrivilegeLevel, &'static str) {
    let el = CurrentEL.read_as_enum(CurrentEL::EL);
    match el {
        Some(CurrentEL::EL::Value::EL2) => (PrivilegeLevel::Hypervisor, "EL2"),
        Some(CurrentEL::EL::Value::EL1) => (PrivilegeLevel::Kernel, "EL1"),
        Some(CurrentEL::EL::Value::EL0) => (PrivilegeLevel::User, "EL0"),
        _ => (PrivilegeLevel::Unknown, "Unknown"),
    }
}

/// Init exception handling by setting the exception vector base address register.
///
/// # Safety
///
/// - Changes the HW state of the executing core.
/// - The vector table and the symbol `__exception_vector_table_start` from the linker script must
///   adhere to the alignment and size constraints demanded by the ARMv8-A Architecture Reference
///   Manual.
pub unsafe fn handling_init() {
    // Provided by exception.S.
    extern "C" {
        static mut __exception_vector_start: u64;
    }
    let addr: u64 = &__exception_vector_start as *const _ as u64;

    VBAR_EL1.set(addr);

    // Force VBAR update to complete before next instruction.
    barrier::isb(barrier::SY);
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use test_macros::kernel_test;

    /// Libkernel unit tests must execute in kernel mode.
    #[kernel_test]
    fn test_runner_executes_in_kernel_mode() {
        let (level, _) = current_privilege_level();

        assert!(level == PrivilegeLevel::Kernel)
    }
}

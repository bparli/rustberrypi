use crate::memory;
use core::ops::Range;

/// Return the range spanning the .bss section.
///
/// # Safety
///
/// - The symbol-provided addresses must be valid.
/// - The symbol-provided addresses must be usize aligned.
unsafe fn bss_range() -> Range<*mut usize> {
    extern "C" {
        // Boundaries of the .bss section, provided by linker script symbols.
        static mut __bss_start: usize;
        static mut __bss_end: usize;
    }

    Range {
        start: &mut __bss_start,
        end: &mut __bss_end,
    }
}

/// Zero out the .bss section.
///
/// # Safety
///
/// - Must only be called pre `kernel_init()`.
#[inline(always)]
pub unsafe fn zero_bss() {
    memory::zero_volatile(bss_range());
}

#[macro_export]
macro_rules! define_mask {
    ($end:expr, $beg:expr) => {
        ((1 << $end) - (1 << $beg) + (1 << $end))
    };
}

#[macro_export]
macro_rules! define_bitfield {
    ($field:ident, [$($end:tt - $beg:tt)|*]) => {
        #[allow(non_upper_case_globals)]
        pub const $field: u64 = $( define_mask!($end, $beg) )|*;
    };
}

#[macro_export]
macro_rules! defreg {
    ($regname:ident) => { defreg!($regname, []); };
    ($regname:ident, [$($field:ident $bits:tt,)*]) => {
        #[allow(non_snake_case)]
        pub mod $regname {
            pub struct Register;
            impl Register {
                #[inline(always)]
                pub unsafe fn set(&self, val: u64) {
                    llvm_asm!(concat!("msr ", stringify!($regname), ", $0") :: "r"(val) :: "volatile");
                }
                #[inline(always)]
                pub unsafe fn get(&self) -> u64 {
                    let rtn;
                    llvm_asm!(concat!("mrs $0, ", stringify!($regname))
                         : "=r"(rtn) ::: "volatile");
                    rtn
                }
            }

            $( define_bitfield!($field, $bits); )*
        }

        #[allow(non_upper_case_globals)]
        pub static $regname: $regname::Register = $regname::Register {};
    }
}

// (ref: D7.2.87: Secure Configuration Register)
defreg!(
    SCR_EL3,
    [
        TERR[15 - 15], // Trap Error record accesses
        TLOR[14 - 14], // Trap LOR registers
        TWE[13 - 13],  // Traps EL2, EL1, and EL0 execution of WFE to EL3
        TWI[12 - 12],  // Traps EL2, EL1, and EL0 execution of WFI to EL3
        ST[11 - 11], // Traps Secure EL1 accesses to the Counter-timer Physical Secure timer registers to EL3
        RW[10 - 10], // Execution state control for lower Exception levels
        SIF[09 - 09], // Secure instruction fetch
        HCE[08 - 08], // Hypervisor Call instruction enable
        SMD[07 - 07], // Secure Monitor Call disable
        EA[03 - 03], // External Abort and SError interrupt routing
        FIQ[02 - 02], // Physical FIQ Routing
        IRQ[01 - 01], // Physical IRQ Routing
        NS[00 - 00], // Non-secure bit
        RES0[63 - 16 | 06 - 06],
        RES1[05 - 04],
    ]
);

// (ref: C5.2.20: Saved Program Status Register)
defreg!(
    SPSR_EL3,
    [
        N[31 - 31],     // Negative Condition flag
        Z[30 - 30],     // Zero Condition flag
        C[29 - 29],     // Carry Condition flag
        V[28 - 28],     // Overflow Condition flag
        TCO[25 - 25],   // Tag Check Override
        DIT[24 - 24],   // Data Independent Timing
        UAO[23 - 23],   // User Access Override
        PAN[22 - 22],   // Privileged Access Never
        SS[21 - 21],    // Software Step
        IL[20 - 20],    // Illegal Execution state
        SSBS[12 - 12],  // Speculative Store Bypass
        BTYPE[11 - 10], // Branch Type Indicator
        D[09 - 09],     // Debug exception mask
        A[08 - 08],     // SError interrupt mask
        I[07 - 07],     // IRQ interrupt mask
        F[06 - 06],     // FIQ interrupt mask
        M4[04 - 04],    // Execution state
        M[03 - 00],     // AArch64 Exception level and selected Stack Pointer
        RES0[63 - 32 | 27 - 26 | 19 - 13 | 05 - 05],
    ]
);

// (ref: C5.2.7 Exception Link Register EL3)
defreg!(ELR_EL3);

// (ref: D7.2.88 System Control Register)
defreg!(
    SCTLR_EL1,
    [
        UCI[26 - 26],  // Traps EL0 execution of cache maintenance instructions to EL1
        EE[25 - 25],   // Endianness of data accesses at EL1
        EOE[24 - 24],  // Endianness of data accesses at EL0
        WXN[19 - 19],  // Write permission implies XN (Execute-never)
        nTWE[18 - 18], // Traps EL0 execution of WFE instructions to EL1
        nTWI[16 - 16], // Traps EL0 execution of WFI instructions to EL1
        UCT[15 - 15],  // Traps EL0 accesses to the CTR_EL0 to EL1
        DZE[14 - 14],  // Traps EL0 execution of DC ZVA instructions to EL1
        I[12 - 12],    // Instruction access Cacheability control
        UMA[09 - 09],  // User Mask Access
        SED[08 - 08],  // SETEND instruction disable
        ITD[07 - 07],  // IT Disable
        CP15[05 - 05], // System instruction memory barrier enable
        SA0[04 - 04],  // SP Alignment check enable for EL0
        SA[03 - 03],   // SP Alignment check enable.
        C[02 - 02],    // Cacheability control
        A[01 - 01],    // Alignment check enable
        M[00 - 00],    // MMU enable for EL1 and EL0 stage 1 address translation
        RES1[29 - 28 | 23 - 22 | 20 - 20 | 11 - 11],
    ]
);

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use test_macros::kernel_test;

    /// Check `bss` section layout.
    #[kernel_test]
    fn bss_section_is_sane() {
        use core::mem;

        let start = unsafe { bss_range().start } as *const _ as usize;
        let end = unsafe { bss_range().end } as *const _ as usize;

        assert_eq!(start % mem::size_of::<usize>(), 0);
        assert_eq!(end % mem::size_of::<usize>(), 0);
        assert!(end >= start);
    }
}

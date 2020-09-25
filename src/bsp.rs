pub mod device_driver;
mod raspberrypi;
pub use raspberrypi::*;

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::memory;
    use test_macros::kernel_test;

    /// Ensure the kernel's virtual memory layout is free of overlaps.
    #[kernel_test]
    fn virt_mem_layout_has_no_overlaps() {
        let layout = memory::virt_mem_layout().inner();

        for (i, first) in layout.iter().enumerate() {
            for second in layout.iter().skip(i + 1) {
                let first_range = first.virtual_range;
                let second_range = second.virtual_range;

                assert!(!first_range().contains(second_range().start()));
                assert!(!first_range().contains(second_range().end()));
                assert!(!second_range().contains(first_range().start()));
                assert!(!second_range().contains(first_range().end()));
            }
        }
    }
}

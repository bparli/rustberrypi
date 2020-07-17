#[path = "arch/time.rs"]
mod arch_time;
pub use arch_time::*;

/// Timekeeping interfaces.
pub mod interface {
    use core::time::Duration;

    /// Time management functions.
    ///
    /// The `BSP` is supposed to supply one global instance.
    pub trait TimeManager {
        /// The timer's resolution.
        fn resolution(&self) -> Duration;

        /// The uptime since power-on of the device.
        ///
        /// This includes time consumed by firmware and bootloaders.
        fn uptime(&self) -> Duration;

        /// Spin for a given duration.
        fn spin_for(&self, duration: Duration);
    }
}

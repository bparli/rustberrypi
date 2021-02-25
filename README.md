# Rustberrypi
Hobby OS to hack on.  Built in Rust for raspberry pi 3

# Setup
Thanks to the [Rust raspberry pi tutorials repo](https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials), tooling is conveniently Docker based.

```
rustup toolchain add nightly-2020-06-30
rustup default nightly-2020-06-30
rustup component add llvm-tools-preview
rustup target add aarch64-unknown-none-softfloat
cargo install cargo-binutils
```

# Build

Copy contents of `ext` to the boot partition of the sd card and load into the raspberry pi

Run `make` then copy `kernel8.img` to sd boot partition 

For more convenient development, copy `/ext/kernel8.img` to sd boot partition and use `make chainboot` to load the kernel over `UART`

# Features
* Virtual memory
* Global Heap allocation
* Interrupt handling
* Process scheduler and context switching
* User level kernel level processes/tasks
* Syscalls suport (only exit and sleep for now)
* Multi-core
* Ethernet

## Acknowledgements

* [https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials](https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials)
* [https://github.com/s-matyukevich/raspberry-pi-os](https://github.com/s-matyukevich/raspberry-pi-os)


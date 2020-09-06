// use std::env;

// fn main() {
//     //let linker_file = env::var("LINKER_FILE").unwrap();

//     println!("cargo:rerun-if-changed={}", linker_file);
// }

pub fn main() {
    println!("cargo:rerun-if-changed=./src/link.ld");
    println!("cargo:rerun-if-env-changed=LOG_LEVEL");
}
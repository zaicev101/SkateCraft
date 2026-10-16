fn main() {
    // x86 stdcall exports need explicit undecorated names: map them with a .def file.
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap().replace('\\', "/");
    println!("cargo:rustc-link-arg=/DEF:{dir}/dinput8.def");
    println!("cargo:rerun-if-changed=dinput8.def");
}

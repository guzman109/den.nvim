//! Neovim loads this library into its own process, where the Lua C API
//! already exists. On macOS the linker must be told to leave those symbols
//! unresolved until load time; only the library gets that flag.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-cdylib-link-arg=-undefined");
        println!("cargo:rustc-cdylib-link-arg=dynamic_lookup");
    }
}

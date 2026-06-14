//! Embed the default config.toml into the binary at compile time.

fn main() {
    println!("cargo:rerun-if-changed=defaults/config.toml");
}

fn main() {
    // Debug builds carry the compiled element tree so the accessibility tests
    // can walk it; release builds leave it out.
    let debug_info = std::env::var("PROFILE").as_deref() != Ok("release");
    let config = slint_build::CompilerConfiguration::new().with_debug_info(debug_info);
    slint_build::compile_with_config("ui/app.slint", config)
        .expect("failed to compile the Slint UI");
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=../../assets/branding");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LUMBRIDGE_TUI_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=LUMBRIDGE_TUI_BUILD_ID");
    println!("cargo:rerun-if-env-changed=LUMBRIDGE_TUI_BUILD_COMMIT");
}

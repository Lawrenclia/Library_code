//! Print the actual compiled registry for tooling and isolated UI verification.
fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&library_core::framework::manifest()).unwrap()
    );
}

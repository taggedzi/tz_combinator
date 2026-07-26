//! Guardrails for the core/interface boundary.

#[test]
fn core_has_no_codec_or_interface_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("csv ="));
    assert!(!manifest.contains("serde_json ="));
    assert!(!std::path::Path::new("src/input.rs").exists());
    assert!(!std::path::Path::new("src/output.rs").exists());
    assert!(!std::path::Path::new("src/template.rs").exists());
    assert!(!std::path::Path::new("src/estimate.rs").exists());
    assert!(!std::path::Path::new("src/execute.rs").exists());
}

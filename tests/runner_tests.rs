use nabla_runner::{FirmwareBuildRunner, BuildRunner};
use nabla_core::{BuildSystem, BuildResult};
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;
use std::path::Path;
use async_trait::async_trait;

struct MockBuildRunner;

#[async_trait]
impl BuildRunner for MockBuildRunner {
    async fn detect(&self, path: &Path) -> Option<BuildSystem> {
        if path.join("Cargo.toml").exists() {
            Some(BuildSystem::Cargo)
        } else if path.join("Makefile").exists() {
            Some(BuildSystem::Makefile)
        } else if path.join("CMakeLists.txt").exists() {
            Some(BuildSystem::CMake)
        } else {
            None
        }
    }

    async fn build(&self, _path: &Path, system: BuildSystem) -> anyhow::Result<BuildResult> {
        Ok(BuildResult {
            success: true,
            output_path: Some("/tmp/firmware.bin".to_string()),
            target_format: Some("ELF".to_string()),
            error_output: None,
            build_system: system,
            duration_ms: 1234,
        })
    }
}


#[tokio::test]
async fn test_detect_rust_project() {
    let dir = tempdir().unwrap();
    let path = dir.path();

    let cargo_toml_path = path.join("Cargo.toml");
    let mut file = File::create(cargo_toml_path).unwrap();
    file.write_all(b"[package]\nname = \"test-crate\"\nversion = \"0.1.0\"
").unwrap();

    let runner = FirmwareBuildRunner::new();
    let detected_system = runner.detect(path).await;

    assert_eq!(detected_system, Some(BuildSystem::Cargo));
}

#[tokio::test]
async fn test_detect_makefile_project() {
    let dir = tempdir().unwrap();
    let path = dir.path();

    let makefile_path = path.join("Makefile");
    File::create(makefile_path).unwrap();

    let runner = FirmwareBuildRunner::new();
    let detected_system = runner.detect(path).await;

    assert_eq!(detected_system, Some(BuildSystem::Makefile));
}

#[tokio::test]
async fn test_detect_cmake_project() {
    let dir = tempdir().unwrap();
    let path = dir.path();

    let cmakelists_path = path.join("CMakeLists.txt");
    File::create(cmakelists_path).unwrap();

    let runner = FirmwareBuildRunner::new();
    let detected_system = runner.detect(path).await;

    assert_eq!(detected_system, Some(BuildSystem::CMake));
}

#[tokio::test]
async fn test_build_function() {
    let dir = tempdir().unwrap();
    let path = dir.path();
    let runner = MockBuildRunner{};

    let build_result = runner.build(path, BuildSystem::Cargo).await.unwrap();

    assert!(build_result.success);
    assert_eq!(build_result.build_system, BuildSystem::Cargo);
    assert_eq!(build_result.output_path, Some("/tmp/firmware.bin".to_string()));
}
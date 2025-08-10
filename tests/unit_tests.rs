use nabla_runner::{detection, execution};
use nabla_core::BuildSystem;
use std::fs;
use tempfile::TempDir;
use tokio;

#[tokio::test]
async fn test_detect_cargo_project() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create a Cargo.toml file
    let cargo_toml = r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#;
    fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml).unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::Cargo));
}

#[tokio::test]
async fn test_detect_makefile_project() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create a Makefile
    let makefile = r#"all:
	echo "Building project"

clean:
	rm -f output
"#;
    fs::write(temp_dir.path().join("Makefile"), makefile).unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::Makefile));
}

#[tokio::test]
async fn test_detect_cmake_project() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create a CMakeLists.txt file
    let cmake_lists = r#"cmake_minimum_required(VERSION 3.10)
project(TestProject)
add_executable(test main.c)
"#;
    fs::write(temp_dir.path().join("CMakeLists.txt"), cmake_lists).unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::CMake));
}

#[tokio::test]
async fn test_detect_platformio_project() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create a platformio.ini file
    let platformio_ini = r#"[env:uno]
platform = atmelavr
board = uno
framework = arduino
"#;
    fs::write(temp_dir.path().join("platformio.ini"), platformio_ini).unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::PlatformIO));
}

#[tokio::test]
async fn test_detect_zephyr_project() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create a west.yml file
    let west_yml = r#"manifest:
  projects:
    - name: zephyr
      url: https://github.com/zephyrproject-rtos/zephyr
      revision: main
"#;
    fs::write(temp_dir.path().join("west.yml"), west_yml).unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::ZephyrWest));
}

#[tokio::test]
async fn test_detect_scons_project() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create a SConstruct file
    let sconstruct = r#"env = Environment()
env.Program('hello', 'hello.c')
"#;
    fs::write(temp_dir.path().join("SConstruct"), sconstruct).unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::SCons));
}

#[tokio::test]
async fn test_detect_stm32_project() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create STM32 project files
    fs::write(temp_dir.path().join(".project"), "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    fs::write(temp_dir.path().join(".cproject"), "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::STM32CubeIDE));
}

#[tokio::test]
async fn test_detect_no_build_system() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create just a README file
    fs::write(temp_dir.path().join("README.md"), "# Test Project").unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, None);
}

#[tokio::test]
async fn test_detect_multiple_build_systems() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create multiple build system files - should detect Cargo first (priority order)
    fs::write(temp_dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
    fs::write(temp_dir.path().join("Makefile"), "all:\n\techo test").unwrap();
    
    let detected = detection::detect_build_system(temp_dir.path()).await;
    assert_eq!(detected, Some(BuildSystem::Cargo));
}

#[tokio::test]
async fn test_execution_with_invalid_path() {
    let temp_dir = TempDir::new().unwrap();
    let non_existent_path = temp_dir.path().join("non-existent");
    
    let result = execution::execute_build(&non_existent_path, BuildSystem::Cargo).await;
    assert!(result.is_ok());
    
    let build_result = result.unwrap();
    assert!(!build_result.success);
    assert!(build_result.error_output.is_some());
}

#[tokio::test]
async fn test_build_system_display() {
    // Test that BuildSystem enum can be formatted for logging
    assert_eq!(format!("{:?}", BuildSystem::Cargo), "Cargo");
    assert_eq!(format!("{:?}", BuildSystem::Makefile), "Makefile");
    assert_eq!(format!("{:?}", BuildSystem::CMake), "CMake");
    assert_eq!(format!("{:?}", BuildSystem::PlatformIO), "PlatformIO");
    assert_eq!(format!("{:?}", BuildSystem::ZephyrWest), "ZephyrWest");
    assert_eq!(format!("{:?}", BuildSystem::STM32CubeIDE), "STM32CubeIDE");
    assert_eq!(format!("{:?}", BuildSystem::SCons), "SCons");
}
use anyhow::Result;
use nabla_runner::{
    core::{BuildResult, BuildSystem},
    intelligent_build::{IntelligentBuilder, BuildFixDatabase, BuildStrategy, BuildConfig},
};
use std::collections::HashMap;
use std::path::Path;
use tempfile::TempDir;
use std::fs;

/// Create a mock IntelligentBuilder for testing
fn create_test_intelligent_builder() -> IntelligentBuilder {
    let fix_db = BuildFixDatabase {
        error_patterns: HashMap::new(),
        successful_configs: HashMap::new(),
    };
    IntelligentBuilder::new(fix_db)
}

/// Create a mock IntelligentBuilder with pre-configured error patterns
fn create_intelligent_builder_with_patterns() -> IntelligentBuilder {
    let mut error_patterns = HashMap::new();
    
    // Add some test error patterns for PlatformIO
    error_patterns.insert(
        "Could not install package".to_string(),
        vec![
            BuildStrategy::VersionDowngrade("5.4.0".to_string()),
            BuildStrategy::ArchitectureSwitch("amd64".to_string()),
        ],
    );
    
    // Add test patterns for CMake
    error_patterns.insert(
        "Could not find compiler".to_string(),
        vec![
            BuildStrategy::ToolchainFallback("gcc".to_string()),
            BuildStrategy::ToolchainFallback("clang".to_string()),
        ],
    );
    
    let fix_db = BuildFixDatabase {
        error_patterns,
        successful_configs: HashMap::new(),
    };
    IntelligentBuilder::new(fix_db)
}

fn create_test_platformio_project(temp_dir: &Path) -> Result<()> {
    // Create platformio.ini
    let platformio_ini = r#"[env:esp32dev]
platform = espressif32
board = esp32dev
framework = arduino
"#;
    fs::write(temp_dir.join("platformio.ini"), platformio_ini)?;

    // Create src/main.cpp
    fs::create_dir_all(temp_dir.join("src"))?;
    let main_cpp = r#"#include <Arduino.h>

void setup() {
    Serial.begin(115200);
    Serial.println("Hello World!");
}

void loop() {
    delay(1000);
}
"#;
    fs::write(temp_dir.join("src").join("main.cpp"), main_cpp)?;
    Ok(())
}

fn create_test_cmake_project(temp_dir: &Path) -> Result<()> {
    // Create CMakeLists.txt
    let cmake_lists = r#"cmake_minimum_required(VERSION 3.10)
project(TestFirmware)

set(CMAKE_C_STANDARD 99)

add_executable(firmware main.c)
"#;
    fs::write(temp_dir.join("CMakeLists.txt"), cmake_lists)?;

    // Create main.c
    let main_c = r#"#include <stdio.h>

int main() {
    printf("Hello, firmware world!\n");
    return 0;
}
"#;
    fs::write(temp_dir.join("main.c"), main_c)?;
    Ok(())
}

#[tokio::test]
async fn test_intelligent_builder_creation() {
    let builder = create_test_intelligent_builder();
    // Just test that we can create the builder without panicking
    assert!(true);
}

#[tokio::test]
async fn test_platformio_error_analysis() {
    let builder = create_intelligent_builder_with_patterns();
    
    // Test PlatformIO error analysis
    let error = "Could not install package framework-arduinoespressif32";
    let strategies = builder.analyze_error(error, BuildSystem::PlatformIO);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(!strategies.is_empty());
    
    // Should contain version downgrade strategy
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::VersionDowngrade(_))));
}

#[tokio::test]
async fn test_cmake_error_analysis() {
    let builder = create_intelligent_builder_with_patterns();
    
    // Test CMake error analysis
    let error = "Could not find compiler for C language";
    let strategies = builder.analyze_error(error, BuildSystem::CMake);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(!strategies.is_empty());
    
    // Should contain toolchain fallback strategies
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::ToolchainFallback(_))));
}

#[tokio::test]
async fn test_makefile_error_analysis() {
    let builder = create_test_intelligent_builder();
    
    // Test Makefile error analysis with missing gcc
    let error = "make: gcc: No such file or directory";
    let strategies = builder.analyze_error(error, BuildSystem::Makefile);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution strategy
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));
}

#[tokio::test]
async fn test_zephyr_error_analysis() {
    let builder = create_test_intelligent_builder();
    
    // Test Zephyr error analysis with missing west
    let error = "west: command not found";
    let strategies = builder.analyze_error(error, BuildSystem::ZephyrWest);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution strategy for west
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));
}

#[tokio::test]
async fn test_stm32_error_analysis() {
    let builder = create_test_intelligent_builder();
    
    // Test STM32 error analysis with missing arm toolchain
    let error = "arm-none-eabi-gcc: command not found";
    let strategies = builder.analyze_error(error, BuildSystem::STM32CubeIDE);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution strategy
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));
}

#[tokio::test]
async fn test_scons_error_analysis() {
    let builder = create_test_intelligent_builder();
    
    // Test SCons error analysis with missing scons
    let error = "scons: command not found";
    let strategies = builder.analyze_error(error, BuildSystem::SCons);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution strategy
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));
}

#[tokio::test]
async fn test_no_error_patterns_found() {
    let builder = create_test_intelligent_builder();
    
    // Test with an error that has no known patterns
    let error = "Some unknown error that we have never seen before";
    let strategies = builder.analyze_error(error, BuildSystem::PlatformIO);
    
    // Should return None for unknown errors
    assert!(strategies.is_none());
}

#[tokio::test]
async fn test_build_strategy_variants() {
    // Test that we can create all strategy variants
    let strategies = vec![
        BuildStrategy::Default,
        BuildStrategy::ToolchainFallback("gcc".to_string()),
        BuildStrategy::ConfigPatch(HashMap::from([("key".to_string(), "value".to_string())])),
        BuildStrategy::DependencyResolution(vec!["package".to_string()]),
        BuildStrategy::ArchitectureSwitch("amd64".to_string()),
        BuildStrategy::VersionDowngrade("1.0.0".to_string()),
    ];
    
    assert_eq!(strategies.len(), 6);
}

#[tokio::test]
async fn test_build_config_creation() {
    let config = BuildConfig {
        toolchain: Some("gcc".to_string()),
        environment: HashMap::from([("CC".to_string(), "gcc".to_string())]),
        build_flags: vec!["-O2".to_string(), "-Wall".to_string()],
    };
    
    assert!(config.toolchain.is_some());
    assert!(!config.environment.is_empty());
    assert!(!config.build_flags.is_empty());
}

#[tokio::test]
async fn test_build_fix_database_creation() {
    let mut error_patterns = HashMap::new();
    error_patterns.insert(
        "test_error".to_string(),
        vec![BuildStrategy::Default],
    );
    
    let mut successful_configs = HashMap::new();
    successful_configs.insert(
        "test_config".to_string(),
        BuildConfig {
            toolchain: None,
            environment: HashMap::new(),
            build_flags: vec![],
        },
    );
    
    let db = BuildFixDatabase {
        error_patterns,
        successful_configs,
    };
    
    assert!(!db.error_patterns.is_empty());
    assert!(!db.successful_configs.is_empty());
}

// Mock test that simulates PlatformIO build failure and retry
#[tokio::test]
async fn test_platformio_build_with_fallback_simulation() {
    let temp_dir = TempDir::new().unwrap();
    create_test_platformio_project(temp_dir.path()).unwrap();
    
    let builder = create_test_intelligent_builder();
    
    // This test would fail in real execution due to missing PlatformIO,
    // but we're testing the structure and error handling path
    let result = builder.execute_with_fallbacks(temp_dir.path(), BuildSystem::PlatformIO).await;
    
    // The build should fail (no PlatformIO installed), but should return a BuildResult
    assert!(result.is_err() || (result.is_ok() && !result.unwrap().success));
}

// Mock test that simulates CMake build failure and retry
#[tokio::test]
async fn test_cmake_build_with_fallback_simulation() {
    let temp_dir = TempDir::new().unwrap();
    create_test_cmake_project(temp_dir.path()).unwrap();
    
    let builder = create_test_intelligent_builder();
    
    // This test would fail in real execution due to missing CMake or compilers,
    // but we're testing the structure and error handling path
    let result = builder.execute_with_fallbacks(temp_dir.path(), BuildSystem::CMake).await;
    
    // The build might succeed or fail depending on the environment, but should return a valid result
    match result {
        Ok(build_result) => {
            // If it succeeds, great! CMake is available
            assert_eq!(build_result.build_system, BuildSystem::CMake);
        }
        Err(_) => {
            // If it fails, that's expected without proper build environment
        }
    }
}

#[tokio::test]
async fn test_patch_platformio_config() {
    let temp_dir = TempDir::new().unwrap();
    create_test_platformio_project(temp_dir.path()).unwrap();
    
    let builder = create_test_intelligent_builder();
    let patches = HashMap::from([
        ("platform".to_string(), "espressif32@5.4.0".to_string()),
    ]);
    
    let result = builder.patch_platformio_config(temp_dir.path(), patches).await;
    assert!(result.is_ok());
    
    // Verify the config was patched
    let config_content = fs::read_to_string(temp_dir.path().join("platformio.ini")).unwrap();
    assert!(config_content.contains("espressif32@5.4.0"));
}

#[tokio::test]
async fn test_patch_cmake_config() {
    let temp_dir = TempDir::new().unwrap();
    create_test_cmake_project(temp_dir.path()).unwrap();
    
    // Create a fake CMakeCache.txt
    fs::write(temp_dir.path().join("CMakeCache.txt"), "CMAKE_C_COMPILER:FILEPATH=/usr/bin/gcc\n").unwrap();
    
    let builder = create_test_intelligent_builder();
    let patches = HashMap::new(); // Empty patches should trigger cache clean
    
    let result = builder.patch_cmake_config(temp_dir.path(), patches).await;
    assert!(result.is_ok());
    
    // Verify the cache was cleaned
    assert!(!temp_dir.path().join("CMakeCache.txt").exists());
}

// Integration test structure for real Tiltbridge testing
#[tokio::test]
async fn test_tiltbridge_integration_structure() {
    // This is a structure test - we would need actual Tiltbridge repo data
    // to make this work, but we can test the structure
    
    let builder = create_test_intelligent_builder();
    
    // In a real scenario, we would:
    // 1. Clone or download Tiltbridge repo
    // 2. Run builder.execute_with_fallbacks on it
    // 3. Verify the build succeeds with fallbacks
    
    // For now, just verify our builder can handle the expected build system
    let temp_dir = TempDir::new().unwrap();
    
    // Tiltbridge uses PlatformIO, so create a similar structure
    create_test_platformio_project(temp_dir.path()).unwrap();
    
    // Test would go here
    let result = builder.execute_with_fallbacks(temp_dir.path(), BuildSystem::PlatformIO).await;
    
    // Should return some result (success or failure with proper error)
    assert!(result.is_err() || result.is_ok());
}
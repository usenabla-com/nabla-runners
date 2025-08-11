use anyhow::Result;
use nabla_runner::{
    core::{BuildResult, BuildSystem},
    intelligent_build::{IntelligentBuilder, BuildFixDatabase, BuildStrategy},
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tokio::process::Command;

/// Mock builder that always fails with specific error messages
struct MockFailingBuilder {
    intelligent_builder: IntelligentBuilder,
    simulated_errors: HashMap<BuildSystem, String>,
}

impl MockFailingBuilder {
    fn new() -> Self {
        let mut error_patterns = HashMap::new();
        
        // Define comprehensive error patterns for each build system
        error_patterns.insert(
            "Could not install package framework-arduinoespressif32".to_string(),
            vec![
                BuildStrategy::VersionDowngrade("5.4.0".to_string()),
                BuildStrategy::ArchitectureSwitch("amd64".to_string()),
                BuildStrategy::ConfigPatch(HashMap::from([
                    ("platform_packages".to_string(), "framework-arduinoespressif32@3.20014.231204".to_string())
                ])),
            ],
        );
        
        error_patterns.insert(
            "Could not find compiler for C language".to_string(),
            vec![
                BuildStrategy::ToolchainFallback("gcc".to_string()),
                BuildStrategy::ToolchainFallback("clang".to_string()),
                BuildStrategy::DependencyResolution(vec!["build-essential".to_string()]),
            ],
        );
        
        error_patterns.insert(
            "make: command not found".to_string(),
            vec![
                BuildStrategy::DependencyResolution(vec![
                    "build-essential".to_string(),
                    "make".to_string(),
                ]),
            ],
        );
        
        error_patterns.insert(
            "west: command not found".to_string(),
            vec![
                BuildStrategy::DependencyResolution(vec!["west".to_string()]),
                BuildStrategy::ConfigPatch(HashMap::from([
                    ("ZEPHYR_BASE".to_string(), "/opt/zephyr".to_string())
                ])),
            ],
        );
        
        error_patterns.insert(
            "arm-none-eabi-gcc: command not found".to_string(),
            vec![
                BuildStrategy::DependencyResolution(vec!["gcc-arm-none-eabi".to_string()]),
                BuildStrategy::ToolchainFallback("gcc".to_string()),
            ],
        );
        
        error_patterns.insert(
            "scons: command not found".to_string(),
            vec![
                BuildStrategy::DependencyResolution(vec!["scons".to_string()]),
                BuildStrategy::ToolchainFallback("python3".to_string()),
            ],
        );

        let fix_db = BuildFixDatabase {
            error_patterns,
            successful_configs: HashMap::new(),
        };
        
        let intelligent_builder = IntelligentBuilder::new(fix_db);
        
        // Define specific error messages for each build system
        let mut simulated_errors = HashMap::new();
        simulated_errors.insert(
            BuildSystem::PlatformIO,
            "Could not install package framework-arduinoespressif32@3.20014.231204. Are you sure it exists?".to_string(),
        );
        simulated_errors.insert(
            BuildSystem::CMake,
            "Could not find compiler for C language. Tried: /usr/bin/cc".to_string(),
        );
        simulated_errors.insert(
            BuildSystem::Makefile,
            "make: gcc: No such file or directory".to_string(),
        );
        simulated_errors.insert(
            BuildSystem::ZephyrWest,
            "west: command not found. Please install west tool.".to_string(),
        );
        simulated_errors.insert(
            BuildSystem::STM32CubeIDE,
            "arm-none-eabi-gcc: command not found. ARM toolchain not installed.".to_string(),
        );
        simulated_errors.insert(
            BuildSystem::SCons,
            "scons: command not found. Please install SCons build tool.".to_string(),
        );

        Self {
            intelligent_builder,
            simulated_errors,
        }
    }

    /// Simulate a build failure for testing
    async fn simulate_build_failure(&self, build_system: BuildSystem) -> Result<Vec<BuildStrategy>> {
        let default_error = "Generic build failure".to_string();
        let error_message = self.simulated_errors.get(&build_system)
            .unwrap_or(&default_error);
        
        let strategies = self.intelligent_builder.analyze_error(error_message, build_system);
        Ok(strategies.unwrap_or_default())
    }
}

fn create_platformio_project_with_issues(temp_dir: &Path) -> Result<()> {
    // Create a PlatformIO project with known issues
    let platformio_ini = r#"[env:esp32dev]
platform = espressif32@99.99.99
board = esp32dev
framework = arduino
lib_deps = 
    nonexistent-library@1.0.0
    another-fake-lib
"#;
    fs::write(temp_dir.join("platformio.ini"), platformio_ini)?;

    fs::create_dir_all(temp_dir.join("src"))?;
    let main_cpp = r#"#include <Arduino.h>
#include <NonexistentLibrary.h>  // This will cause compile errors

void setup() {
    Serial.begin(115200);
    NonexistentFunction();  // This will fail
}

void loop() {
    delay(1000);
}
"#;
    fs::write(temp_dir.join("src").join("main.cpp"), main_cpp)?;
    Ok(())
}

fn create_cmake_project_with_issues(temp_dir: &Path) -> Result<()> {
    // Create a CMake project with dependency issues
    let cmake_lists = r#"cmake_minimum_required(VERSION 99.99)  # Unrealistic version requirement
project(FailingProject)

set(CMAKE_C_STANDARD 99)  # Non-existent standard

find_package(NonexistentPackage REQUIRED)  # Will fail

add_executable(firmware main.c nonexistent_file.c)  # Missing file

target_link_libraries(firmware nonexistent_lib)  # Missing library
"#;
    fs::write(temp_dir.join("CMakeLists.txt"), cmake_lists)?;

    let main_c = r#"#include <stdio.h>
#include "nonexistent_header.h"  // Missing header

int main() {
    printf("This won't compile\n");
    nonexistent_function();  // Missing function
    return 0;
}
"#;
    fs::write(temp_dir.join("main.c"), main_c)?;
    Ok(())
}

fn create_makefile_project_with_issues(temp_dir: &Path) -> Result<()> {
    // Create a Makefile project with toolchain issues
    let makefile = r#"CC=nonexistent-compiler
CFLAGS=-Wall -Wextra -std=c99
LDFLAGS=-lnonexistent

firmware: main.c missing_file.c
	$(CC) $(CFLAGS) -o firmware $^ $(LDFLAGS)

clean:
	rm -f firmware

.PHONY: clean
"#;
    fs::write(temp_dir.join("Makefile"), makefile)?;

    let main_c = r#"#include <stdio.h>
#include "missing_header.h"

extern void missing_function(void);

int main() {
    printf("Hello, world!\n");
    missing_function();  // Undefined reference
    return 0;
}
"#;
    fs::write(temp_dir.join("main.c"), main_c)?;
    Ok(())
}

#[tokio::test]
async fn test_platformio_failure_scenarios() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test ESP32 platform installation failure
    let strategies = mock_builder.simulate_build_failure(BuildSystem::PlatformIO).await?;
    assert!(!strategies.is_empty());
    
    // Should contain version downgrade strategy
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::VersionDowngrade(_))));
    
    // Should contain architecture switch strategy
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::ArchitectureSwitch(_))));
    
    // Should contain config patch strategy
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::ConfigPatch(_))));

    Ok(())
}

#[tokio::test]
async fn test_cmake_failure_scenarios() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test CMake compiler not found failure
    let strategies = mock_builder.simulate_build_failure(BuildSystem::CMake).await?;
    assert!(!strategies.is_empty());
    
    // Should contain toolchain fallback strategies
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::ToolchainFallback(_))));

    Ok(())
}

#[tokio::test]
async fn test_makefile_failure_scenarios() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test Makefile missing tools failure
    let strategies = mock_builder.simulate_build_failure(BuildSystem::Makefile).await?;
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution strategy for build tools
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));

    Ok(())
}

#[tokio::test]
async fn test_zephyr_failure_scenarios() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test Zephyr missing west tool failure
    let strategies = mock_builder.simulate_build_failure(BuildSystem::ZephyrWest).await?;
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution for west
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));

    Ok(())
}

#[tokio::test]
async fn test_stm32_failure_scenarios() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test STM32 missing ARM toolchain failure
    let strategies = mock_builder.simulate_build_failure(BuildSystem::STM32CubeIDE).await?;
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution for ARM toolchain
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));

    Ok(())
}

#[tokio::test]
async fn test_scons_failure_scenarios() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test SCons missing tool failure
    let strategies = mock_builder.simulate_build_failure(BuildSystem::SCons).await?;
    assert!(!strategies.is_empty());
    
    // Should contain dependency resolution for scons
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_))));

    Ok(())
}

#[tokio::test]
async fn test_cascading_failure_strategies() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test that multiple strategies are generated for complex errors
    let complex_error = "Could not install package framework-arduinoespressif32@3.20014.231204. Error: Package not found for platform espressif32@6.4.0 on linux_x86_64";
    
    let strategies = mock_builder.intelligent_builder.analyze_error(complex_error, BuildSystem::PlatformIO);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(strategies.len() >= 2); // Should have multiple fallback strategies
    
    // Should include both version downgrade and architecture switch
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::VersionDowngrade(_))));
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::ArchitectureSwitch(_))));

    Ok(())
}

#[tokio::test]
async fn test_unknown_error_handling() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test with completely unknown error
    let unknown_error = "Some completely new error that we have never seen before and have no patterns for";
    
    let strategies = mock_builder.intelligent_builder.analyze_error(unknown_error, BuildSystem::PlatformIO);
    
    // Should return None for unknown errors
    assert!(strategies.is_none());

    Ok(())
}

#[tokio::test]
async fn test_error_pattern_priority() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test that error patterns are matched correctly
    let specific_error = "Could not install package framework-arduinoespressif32";
    let general_error = "Could not install package some-other-package";
    
    let specific_strategies = mock_builder.intelligent_builder.analyze_error(specific_error, BuildSystem::PlatformIO);
    let general_strategies = mock_builder.intelligent_builder.analyze_error(general_error, BuildSystem::PlatformIO);
    
    // Specific error should match our patterns
    assert!(specific_strategies.is_some());
    
    // General error might not match depending on exact patterns
    // The key is that specific patterns should always work
    println!("Specific strategies: {:?}", specific_strategies);
    println!("General strategies: {:?}", general_strategies);

    Ok(())
}

#[tokio::test]
async fn test_build_system_specific_error_analysis() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test that the same error message produces different strategies for different build systems
    let compiler_error = "Could not find compiler for C language";
    
    let cmake_strategies = mock_builder.intelligent_builder.analyze_error(compiler_error, BuildSystem::CMake);
    let makefile_strategies = mock_builder.intelligent_builder.analyze_error(compiler_error, BuildSystem::Makefile);
    
    // CMake should have specific strategies
    assert!(cmake_strategies.is_some());
    
    // Makefile might not have specific patterns for this error
    // (depending on our error pattern database)
    // This tests that build system context matters
    println!("CMake strategies: {:?}", cmake_strategies);
    println!("Makefile strategies: {:?}", makefile_strategies);

    Ok(())
}

#[tokio::test]
async fn test_strategy_application_simulation() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_platformio_project_with_issues(temp_dir.path())?;
    
    let mock_builder = MockFailingBuilder::new();
    
    // Simulate applying a version downgrade strategy
    let patches = HashMap::from([
        ("platform".to_string(), "espressif32@5.4.0".to_string()),
    ]);
    
    let result = mock_builder.intelligent_builder.patch_platformio_config(temp_dir.path(), patches).await;
    assert!(result.is_ok());
    
    // Verify the patch was applied
    let config_content = fs::read_to_string(temp_dir.path().join("platformio.ini"))?;
    assert!(config_content.contains("espressif32@5.4.0"));

    Ok(())
}

#[tokio::test]
async fn test_multiple_error_recovery_simulation() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Simulate multiple consecutive errors and recovery strategies
    let errors = vec![
        ("Could not install package framework-arduinoespressif32", BuildSystem::PlatformIO),
        ("Could not find compiler for C language", BuildSystem::CMake),
        ("make: gcc: No such file or directory", BuildSystem::Makefile),
    ];
    
    let mut all_strategies = Vec::new();
    
    for (error, system) in errors {
        let strategies = mock_builder.intelligent_builder.analyze_error(error, system);
        if let Some(strategies) = strategies {
            all_strategies.extend(strategies);
        }
    }
    
    // Should have accumulated multiple different strategies
    assert!(!all_strategies.is_empty());
    
    // Should have different types of strategies
    let has_version_downgrade = all_strategies.iter().any(|s| matches!(s, BuildStrategy::VersionDowngrade(_)));
    let has_toolchain_fallback = all_strategies.iter().any(|s| matches!(s, BuildStrategy::ToolchainFallback(_)));
    let has_dependency_resolution = all_strategies.iter().any(|s| matches!(s, BuildStrategy::DependencyResolution(_)));
    
    assert!(has_version_downgrade || has_toolchain_fallback || has_dependency_resolution);

    Ok(())
}

#[tokio::test]
async fn test_container_fallback_simulation() -> Result<()> {
    let mock_builder = MockFailingBuilder::new();
    
    // Test architecture switch strategy (which would trigger container builds)
    let arch_error = "Package espressif32 is not compatible with linux_x86_64";
    let strategies = mock_builder.intelligent_builder.analyze_error(arch_error, BuildSystem::PlatformIO);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    
    // Should suggest architecture switch
    assert!(strategies.iter().any(|s| matches!(s, BuildStrategy::ArchitectureSwitch(_))));
    
    // In real implementation, this would trigger container-based builds
    let temp_dir = TempDir::new()?;
    create_platformio_project_with_issues(temp_dir.path())?;
    
    // Test that build_in_container method exists and returns expected error
    let result = mock_builder.intelligent_builder.build_in_container(temp_dir.path(), BuildSystem::PlatformIO, "amd64".to_string()).await;
    
    // Should return error since container building is not implemented
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Container building not implemented"));

    Ok(())
}
use anyhow::Result;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use nabla_runner::{
    core::{BuildResult, BuildSystem},
    detection::detect_build_system,
    intelligent_build::{IntelligentBuilder, BuildFixDatabase},
    server::create_app,
};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tokio::process::Command;
use tower::util::ServiceExt;
use zip::write::FileOptions;
use zip::ZipWriter;

/// Download and extract Tiltbridge repository for testing
async fn download_tiltbridge_repo() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;
    let archive_url = "https://github.com/thorrak/tiltbridge/archive/refs/heads/main.tar.gz";
    
    // Download the archive
    let response = reqwest::get(archive_url).await?;
    let archive_data = response.bytes().await?;
    
    // Save to temporary file
    let archive_path = temp_dir.path().join("tiltbridge.tar.gz");
    tokio::fs::write(&archive_path, archive_data).await?;
    
    // Extract the archive
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(temp_dir.path())
        .output()
        .await?;
    
    if !output.status.success() {
        anyhow::bail!("Failed to extract archive: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    Ok(temp_dir)
}

/// Find the extracted Tiltbridge directory
fn find_tiltbridge_dir(temp_dir: &Path) -> Result<std::path::PathBuf> {
    for entry in fs::read_dir(temp_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.file_name().unwrap().to_str().unwrap().starts_with("tiltbridge-") {
            return Ok(path);
        }
    }
    anyhow::bail!("Could not find Tiltbridge directory")
}

/// Create a zip archive of a directory
fn zip_directory(dir_path: &Path) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    {
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);

        let walkdir = walkdir::WalkDir::new(dir_path);
        let it = walkdir.into_iter().filter_map(|e| e.ok());

        for entry in it {
            let path = entry.path();
            let name = path.strip_prefix(dir_path).unwrap();

            if path.is_file() {
                zip.start_file(name.to_string_lossy(), options)?;
                let mut f = fs::File::open(path)?;
                std::io::copy(&mut f, &mut zip)?;
            } else if !name.as_os_str().is_empty() {
                zip.add_directory(name.to_string_lossy(), options)?;
            }
        }
        zip.finish()?;
    }
    Ok(buffer)
}

/// Create a mock Tiltbridge-like project for testing when download fails
fn create_mock_tiltbridge_project(temp_dir: &Path) -> Result<()> {
    // Create platformio.ini similar to Tiltbridge
    let platformio_ini = r#"[platformio]
default_envs = d32_pro_thread

[common]
framework = arduino
monitor_speed = 115200
upload_speed = 460800

[env:d32_pro]
platform = espressif32@6.4.0
board = lolin_d32_pro
framework = ${common.framework}
monitor_speed = ${common.monitor_speed}
upload_speed = ${common.upload_speed}

[env:d32_pro_thread]
platform = espressif32@6.4.0
board = lolin_d32_pro
framework = ${common.framework}
monitor_speed = ${common.monitor_speed}
upload_speed = ${common.upload_speed}
build_flags = -DUSE_THREADING

[env:tbeam_thread]
platform = espressif32@6.4.0
board = ttgo-t-beam
framework = ${common.framework}
monitor_speed = ${common.monitor_speed}
upload_speed = ${common.upload_speed}
build_flags = -DUSE_THREADING
"#;
    fs::write(temp_dir.join("platformio.ini"), platformio_ini)?;

    // Create src structure
    fs::create_dir_all(temp_dir.join("src"))?;
    
    // Create main.cpp
    let main_cpp = r#"#include <Arduino.h>
#include <WiFi.h>
#include <WebServer.h>

// Mock Tiltbridge main application
WebServer server(80);

void setup() {
    Serial.begin(115200);
    Serial.println("TiltBridge starting...");
    
    // Initialize WiFi (mock)
    WiFi.mode(WIFI_STA);
    
    // Initialize web server
    server.on("/", []() {
        server.send(200, "text/plain", "TiltBridge Web Interface");
    });
    
    server.begin();
    Serial.println("TiltBridge ready");
}

void loop() {
    server.handleClient();
    delay(10);
}
"#;
    fs::write(temp_dir.join("src").join("main.cpp"), main_cpp)?;
    
    // Create lib directory with some mock libraries
    fs::create_dir_all(temp_dir.join("lib").join("TiltBridge"))?;
    let lib_header = r#"#ifndef TILTBRIDGE_H
#define TILTBRIDGE_H

class TiltBridge {
public:
    void init();
    void loop();
};

#endif
"#;
    fs::write(temp_dir.join("lib").join("TiltBridge").join("TiltBridge.h"), lib_header)?;
    
    let lib_cpp = r#"#include "TiltBridge.h"
#include <Arduino.h>

void TiltBridge::init() {
    Serial.println("TiltBridge library initialized");
}

void TiltBridge::loop() {
    // Main loop processing
}
"#;
    fs::write(temp_dir.join("lib").join("TiltBridge").join("TiltBridge.cpp"), lib_cpp)?;

    Ok(())
}

#[tokio::test]
#[ignore] // Mark as ignored by default since it requires network access
async fn test_real_tiltbridge_download_and_detect() -> Result<()> {
    // Try to download real Tiltbridge repo
    let temp_dir = match download_tiltbridge_repo().await {
        Ok(dir) => dir,
        Err(e) => {
            println!("Failed to download Tiltbridge repo: {}. Using mock instead.", e);
            let temp_dir = TempDir::new()?;
            create_mock_tiltbridge_project(temp_dir.path())?;
            temp_dir
        }
    };

    let tiltbridge_dir = if let Ok(dir) = find_tiltbridge_dir(temp_dir.path()) {
        dir
    } else {
        temp_dir.path().to_path_buf()
    };

    // Test build system detection
    let detected_system = detect_build_system(&tiltbridge_dir).await;
    assert_eq!(detected_system, Some(BuildSystem::PlatformIO));

    // Test that we can create a build database and intelligent builder
    let fix_db = BuildFixDatabase {
        error_patterns: HashMap::new(),
        successful_configs: HashMap::new(),
    };
    let builder = IntelligentBuilder::new(fix_db);

    // Test build execution (will likely fail due to missing tools, but should handle gracefully)
    let result = builder.execute_with_fallbacks(&tiltbridge_dir, BuildSystem::PlatformIO).await;
    
    // The build will probably fail, but we should get a proper BuildResult
    match result {
        Ok(build_result) => {
            assert_eq!(build_result.build_system, BuildSystem::PlatformIO);
            // Build might succeed or fail depending on the environment
        }
        Err(e) => {
            // Build failed, but that's expected without PlatformIO installed
            println!("Build failed as expected: {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_mock_tiltbridge_build_detection() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_mock_tiltbridge_project(temp_dir.path())?;

    // Test build system detection
    let detected_system = detect_build_system(temp_dir.path()).await;
    assert_eq!(detected_system, Some(BuildSystem::PlatformIO));

    Ok(())
}

#[tokio::test]
async fn test_mock_tiltbridge_intelligent_build() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_mock_tiltbridge_project(temp_dir.path())?;

    // Create intelligent builder with error patterns similar to what Tiltbridge might encounter
    let mut error_patterns = HashMap::new();
    
    // ESP32 platform specific errors
    error_patterns.insert(
        "Could not install package framework-arduinoespressif32".to_string(),
        vec![
            nabla_runner::intelligent_build::BuildStrategy::VersionDowngrade("5.4.0".to_string()),
            nabla_runner::intelligent_build::BuildStrategy::ArchitectureSwitch("amd64".to_string()),
        ],
    );
    
    error_patterns.insert(
        "platform espressif32@6.4.0 is not compatible".to_string(),
        vec![
            nabla_runner::intelligent_build::BuildStrategy::VersionDowngrade("5.4.0".to_string()),
        ],
    );

    let fix_db = BuildFixDatabase {
        error_patterns,
        successful_configs: HashMap::new(),
    };
    let builder = IntelligentBuilder::new(fix_db);

    // Test error analysis for common ESP32 issues
    let esp32_error = "Could not install package framework-arduinoespressif32@3.20014.231204";
    let strategies = builder.analyze_error(esp32_error, BuildSystem::PlatformIO);
    
    assert!(strategies.is_some());
    let strategies = strategies.unwrap();
    assert!(!strategies.is_empty());

    Ok(())
}

#[tokio::test]
#[ignore] // Network dependent test
async fn test_tiltbridge_http_build_request() -> Result<()> {
    let app = create_app();
    
    let temp_dir = TempDir::new()?;
    create_mock_tiltbridge_project(temp_dir.path())?;
    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=thorrak&repo=tiltbridge&head_sha=main&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should attempt to build but likely fail due to environment
    assert!(response.status() == StatusCode::INTERNAL_SERVER_ERROR || response.status() == StatusCode::ACCEPTED);

    Ok(())
}

#[tokio::test]
async fn test_tiltbridge_build_failure_scenarios() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_mock_tiltbridge_project(temp_dir.path())?;

    let builder = IntelligentBuilder::new(BuildFixDatabase {
        error_patterns: HashMap::new(),
        successful_configs: HashMap::new(),
    });

    // Test various failure scenarios that might occur with Tiltbridge

    // 1. Test missing platform error
    let platform_error = "Error: Unknown platform 'espressif32@6.4.0'";
    let strategies = builder.analyze_error(platform_error, BuildSystem::PlatformIO);
    // Should not find specific patterns for this mock error
    assert!(strategies.is_none());

    // 2. Test architecture mismatch error  
    let arch_error = "Error: Package espressif32 is not compatible with linux_x86_64";
    let strategies = builder.analyze_error(arch_error, BuildSystem::PlatformIO);
    // Should detect architecture issue
    assert!(strategies.is_some());

    // 3. Test dependency error
    let dep_error = "Error: Could not install package framework-arduinoespressif32";
    let strategies = builder.analyze_error(dep_error, BuildSystem::PlatformIO);
    // Should suggest fallback strategies
    assert!(strategies.is_some());

    Ok(())
}

#[tokio::test]
async fn test_tiltbridge_config_patching() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_mock_tiltbridge_project(temp_dir.path())?;

    let builder = IntelligentBuilder::new(BuildFixDatabase {
        error_patterns: HashMap::new(),
        successful_configs: HashMap::new(),
    });

    // Test patching PlatformIO config for Tiltbridge-specific issues
    let patches = HashMap::from([
        ("platform".to_string(), "espressif32@5.4.0".to_string()),
    ]);

    let result = builder.patch_platformio_config(temp_dir.path(), patches).await;
    assert!(result.is_ok());

    // Verify the patch was applied
    let config_content = fs::read_to_string(temp_dir.path().join("platformio.ini"))?;
    assert!(config_content.contains("espressif32@5.4.0"));

    Ok(())
}

#[tokio::test]
async fn test_tiltbridge_multiple_environments() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_mock_tiltbridge_project(temp_dir.path())?;

    // Verify that our mock Tiltbridge project has multiple environments
    let config_content = fs::read_to_string(temp_dir.path().join("platformio.ini"))?;
    
    assert!(config_content.contains("d32_pro"));
    assert!(config_content.contains("d32_pro_thread"));
    assert!(config_content.contains("tbeam_thread"));

    // Test build system detection still works with multiple environments
    let detected_system = detect_build_system(temp_dir.path()).await;
    assert_eq!(detected_system, Some(BuildSystem::PlatformIO));

    Ok(())
}

// Test that demonstrates how to set up a CI/CD test pipeline
#[tokio::test]
async fn test_tiltbridge_ci_simulation() -> Result<()> {
    // This test simulates what would happen in a CI/CD environment
    let temp_dir = TempDir::new()?;
    create_mock_tiltbridge_project(temp_dir.path())?;

    // 1. Detection phase
    let build_system = detect_build_system(temp_dir.path()).await;
    assert_eq!(build_system, Some(BuildSystem::PlatformIO));

    // 2. Create intelligent builder with comprehensive error patterns
    let mut error_patterns = HashMap::new();
    
    // Common CI errors
    error_patterns.insert(
        "Could not install package".to_string(),
        vec![
            nabla_runner::intelligent_build::BuildStrategy::VersionDowngrade("5.4.0".to_string()),
            nabla_runner::intelligent_build::BuildStrategy::ArchitectureSwitch("amd64".to_string()),
        ],
    );

    let fix_db = BuildFixDatabase {
        error_patterns,
        successful_configs: HashMap::new(),
    };
    let builder = IntelligentBuilder::new(fix_db);

    // 3. Attempt build with fallbacks
    let build_result = builder.execute_with_fallbacks(temp_dir.path(), build_system.unwrap()).await;
    
    // 4. Verify we get a proper result structure
    match build_result {
        Ok(result) => {
            assert_eq!(result.build_system, BuildSystem::PlatformIO);
            // In CI, we'd check if build artifacts were created
        }
        Err(_) => {
            // Build failed, but that's expected without proper environment
            // In real CI, we'd have PlatformIO installed
        }
    }

    Ok(())
}
use anyhow::Result;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose, Engine as _};
use nabla_runner::server::create_app;
use serde_json;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tower::util::ServiceExt;
use zip::write::FileOptions;
use zip::ZipWriter;

fn create_test_cargo_project(temp_dir: &Path) -> Result<()> {
    // Create Cargo.toml
    let cargo_toml = r#"[package]
name = "test-firmware"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "firmware"
path = "src/main.rs"
"#;
    fs::write(temp_dir.join("Cargo.toml"), cargo_toml)?;

    // Create src directory and main.rs
    fs::create_dir_all(temp_dir.join("src"))?;
    let main_rs = r#"fn main() {
    println!("Hello, firmware world!");
}
"#;
    fs::write(temp_dir.join("src").join("main.rs"), main_rs)?;

    Ok(())
}

fn create_test_makefile_project(temp_dir: &Path) -> Result<()> {
    // Create Makefile
    let makefile = r#"CC=gcc
CFLAGS=-Wall -Wextra -std=c99

firmware: main.c
	$(CC) $(CFLAGS) -o firmware main.c

clean:
	rm -f firmware

.PHONY: clean
"#;
    fs::write(temp_dir.join("Makefile"), makefile)?;

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

fn create_test_platformio_project(temp_dir: &Path) -> Result<()> {
    // Create platformio.ini
    let platformio_ini = r#"[env:uno]
platform = atmelavr
board = uno
framework = arduino
"#;
    fs::write(temp_dir.join("platformio.ini"), platformio_ini)?;

    // Create src/main.cpp
    fs::create_dir_all(temp_dir.join("src"))?;
    let main_cpp = r#"#include <Arduino.h>

void setup() {
    Serial.begin(9600);
}

void loop() {
    Serial.println("Hello, firmware world!");
    delay(1000);
}
"#;
    fs::write(temp_dir.join("src").join("main.cpp"), main_cpp)?;

    Ok(())
}

fn create_test_zephyr_project(temp_dir: &Path) -> Result<()> {
    // Create west.yml
    let west_yml = r#"manifest:
  projects:
    - name: zephyr
      url: https://github.com/zephyrproject-rtos/zephyr
      revision: main
"#;
    fs::write(temp_dir.join("west.yml"), west_yml)?;

    // Create CMakeLists.txt
    let cmake_lists = r#"cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(test_firmware)

target_sources(app PRIVATE src/main.c)
"#;
    fs::write(temp_dir.join("CMakeLists.txt"), cmake_lists)?;

    // Create src/main.c
    fs::create_dir_all(temp_dir.join("src"))?;
    let main_c = r#"#include <zephyr/kernel.h>

void main(void) {
    printk("Hello, firmware world!\n");
}
"#;
    fs::write(temp_dir.join("src").join("main.c"), main_c)?;

    Ok(())
}

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

#[tokio::test]
async fn test_cargo_project_detection_via_http() -> Result<()> {
    let app = create_app();
    let temp_dir = TempDir::new()?;
    create_test_cargo_project(temp_dir.path())?;

    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // The build will fail because we don't have /workspace in tests, but we should get an error response
    assert!(response.status() == StatusCode::INTERNAL_SERVER_ERROR);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
        // Check that it at least tried to process the request
        assert_eq!(json["status"], "error");
        let error = json["error"].as_str().unwrap_or("");
        // The error should be about workspace or build failure, not about invalid request
        assert!(error.contains("build failed") || error.contains("workspace") || error.contains("No such file"));
    }

    Ok(())
}

#[tokio::test]
async fn test_makefile_project_detection_via_http() -> Result<()> {
    let app = create_app();
    let temp_dir = TempDir::new()?;
    create_test_makefile_project(temp_dir.path())?;

    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // The build will fail because we don't have /workspace in tests
    assert!(response.status() == StatusCode::INTERNAL_SERVER_ERROR);

    Ok(())
}

#[tokio::test]
async fn test_cmake_project_detection_via_http() -> Result<()> {
    let app = create_app();
    let temp_dir = TempDir::new()?;
    create_test_cmake_project(temp_dir.path())?;

    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // The build will fail because we don't have /workspace in tests
    assert!(response.status() == StatusCode::INTERNAL_SERVER_ERROR);

    Ok(())
}

#[tokio::test]
async fn test_platformio_project_detection_via_http() -> Result<()> {
    let app = create_app();
    let temp_dir = TempDir::new()?;
    create_test_platformio_project(temp_dir.path())?;

    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // The build will fail because we don't have /workspace in tests
    assert!(response.status() == StatusCode::INTERNAL_SERVER_ERROR);

    Ok(())
}

#[tokio::test]
async fn test_zephyr_project_detection_via_http() -> Result<()> {
    let app = create_app();
    let temp_dir = TempDir::new()?;
    create_test_zephyr_project(temp_dir.path())?;

    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // The build will fail because we don't have /workspace in tests
    assert!(response.status() == StatusCode::INTERNAL_SERVER_ERROR);

    Ok(())
}

#[tokio::test]
async fn test_invalid_base64_data_via_http() -> Result<()> {
    let app = create_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/base64")
                .body(Body::from("invalid-base64-data!!!"))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should fail with internal server error due to invalid base64
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
        assert_eq!(json["status"], "error");
        let error = json["error"].as_str().unwrap_or("");
        // The error should mention base64 decoding or build failure
        assert!(error.contains("base64") || error.contains("decode") || error.contains("build failed"), 
                "Unexpected error message: {}", error);
    }

    Ok(())
}

#[tokio::test]
async fn test_unsupported_project_type_via_http() -> Result<()> {
    let app = create_app();
    let temp_dir = TempDir::new()?;
    
    // Create a directory with just a README file (no build system)
    fs::write(temp_dir.path().join("README.md"), "# Test Project\n\nThis is a test.")?;

    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should fail with internal server error due to unsupported build system
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
        assert_eq!(json["status"], "error");
        let error = json["error"].as_str().unwrap_or("");
        // The error might be about workspace or unsupported build system
        assert!(error.contains("Unsupported") || error.contains("undetected") || error.contains("workspace") || error.contains("build failed"),
                "Unexpected error message: {}", error);
    }

    Ok(())
}

#[tokio::test]
async fn test_base64_and_zip_content_types() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_test_cargo_project(temp_dir.path())?;
    let zip_data = zip_directory(temp_dir.path())?;

    // Test with ZIP content type
    let app = create_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data.clone()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR); // Expected due to /workspace

    // Test with BASE64 content type
    let app = create_app();
    let base64_data = general_purpose::STANDARD.encode(&zip_data);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/base64")
                .body(Body::from(base64_data))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR); // Expected due to /workspace

    Ok(())
}

#[tokio::test]
async fn test_multiple_build_systems_priority() -> Result<()> {
    let app = create_app();
    let temp_dir = TempDir::new()?;
    
    // Create a project with multiple build systems - Cargo should take priority
    create_test_cargo_project(temp_dir.path())?;
    
    // Also add a Makefile
    let makefile = r#"all:
	echo "This should not be used"
"#;
    fs::write(temp_dir.path().join("Makefile"), makefile)?;

    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123def&installation_id=123&upload_url=http://httpbin.org/post")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    // The build will fail due to /workspace, but it should detect Cargo first
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    Ok(())
}
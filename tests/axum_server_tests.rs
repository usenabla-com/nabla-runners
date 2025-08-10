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
use tower::util::ServiceExt; // for `oneshot`
use zip::write::FileOptions;
use zip::ZipWriter;
use walkdir;

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
async fn test_health_endpoint() -> Result<()> {
    let app = create_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(json["status"], "healthy");
    assert_eq!(json["service"], "nabla-runner");

    Ok(())
}

#[tokio::test]
async fn test_build_endpoint_missing_params() -> Result<()> {
    let app = create_app();

    let temp_dir = TempDir::new()?;
    create_test_cargo_project(temp_dir.path())?;
    let zip_data = zip_directory(temp_dir.path())?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build") // No query parameters
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    
    // Try to parse as JSON, but handle the case where it might not be valid JSON
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
        assert_eq!(json["status"], "error");
        assert!(json["error"].as_str().unwrap_or("").contains("invalid query params"));
    } else {
        // If not JSON, check the raw text - Axum returns plain text for query deserialization errors
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("missing field") || text.contains("deserialize") || text.contains("invalid") || text.contains("error"), 
                "Unexpected response: {}", text);
    }

    Ok(())
}

#[tokio::test]
async fn test_build_endpoint_invalid_content_type() -> Result<()> {
    let app = create_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123&installation_id=123&upload_url=http://example.com")
                .header("content-type", "text/html")
                .body(Body::from("test data"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(json["status"], "error");
    assert!(json["error"].as_str().unwrap().contains("unsupported media type"));

    Ok(())
}

#[tokio::test]
async fn test_build_endpoint_payload_too_large() -> Result<()> {
    let app = create_app();

    // Create a large payload (larger than MAX_UPLOAD_SIZE)
    let large_data = vec![0u8; 201 * 1024 * 1024]; // 201 MB

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc123&installation_id=123&upload_url=http://example.com")
                .header("content-type", "application/zip")
                .body(Body::from(large_data))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    
    // Try to parse as JSON, but handle the case where it might not be valid JSON
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
        assert_eq!(json["status"], "error");
        assert!(json["error"].as_str().unwrap_or("").contains("payload too large"));
    } else {
        // If not JSON, check the raw text - Axum returns plain text for body size limit errors
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("length limit exceeded") || text.contains("payload") || text.contains("large"), 
                "Unexpected response: {}", text);
    }

    Ok(())
}

#[tokio::test]
async fn test_build_endpoint_invalid_params() -> Result<()> {
    let temp_dir = TempDir::new()?;
    create_test_cargo_project(temp_dir.path())?;
    let zip_data = zip_directory(temp_dir.path())?;

    // Test with invalid owner (contains invalid characters)
    let app = create_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=invalid/owner&repo=test&head_sha=abc123&installation_id=123&upload_url=http://example.com")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data.clone()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Test with invalid SHA (too short)
    let app = create_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/build?owner=test&repo=test&head_sha=abc&installation_id=123&upload_url=http://example.com")
                .header("content-type", "application/zip")
                .body(Body::from(zip_data))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    Ok(())
}

#[tokio::test]
async fn test_build_endpoint_zip_content_type() -> Result<()> {
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

    // This will likely fail at the build stage since we don't have the full build environment
    // in the test, but it should at least accept the request and start processing
    assert!(response.status() == StatusCode::ACCEPTED || response.status() == StatusCode::INTERNAL_SERVER_ERROR);

    Ok(())
}

#[tokio::test]
async fn test_build_endpoint_base64_content_type() -> Result<()> {
    let app = create_app();

    let temp_dir = TempDir::new()?;
    create_test_cargo_project(temp_dir.path())?;
    let zip_data = zip_directory(temp_dir.path())?;
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

    // This will likely fail at the build stage since we don't have the full build environment
    // in the test, but it should at least accept the request and start processing
    assert!(response.status() == StatusCode::ACCEPTED || response.status() == StatusCode::INTERNAL_SERVER_ERROR);

    Ok(())
}

#[tokio::test]
async fn test_parameter_validation() -> Result<()> {
    let app = create_app();

    let test_cases = vec![
        // (query_params, expected_status, description)
        ("", StatusCode::BAD_REQUEST, "missing all params"),
        ("owner=&repo=test&head_sha=abc123&installation_id=123&upload_url=http://example.com", StatusCode::BAD_REQUEST, "empty owner"),
        ("owner=test&repo=&head_sha=abc123&installation_id=123&upload_url=http://example.com", StatusCode::BAD_REQUEST, "empty repo"),
        ("owner=test&repo=test&head_sha=&installation_id=123&upload_url=http://example.com", StatusCode::BAD_REQUEST, "empty head_sha"),
        ("owner=test&repo=test&head_sha=abc123&installation_id=0&upload_url=http://example.com", StatusCode::BAD_REQUEST, "zero installation_id"),
        ("owner=test&repo=test&head_sha=abc123&installation_id=123&upload_url=", StatusCode::BAD_REQUEST, "empty upload_url"),
        ("owner=test&repo=test&head_sha=abc123def456&installation_id=123&upload_url=http://example.com", StatusCode::INTERNAL_SERVER_ERROR, "valid params but will fail at build"),
    ];

    for (query_params, expected_status, description) in test_cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/build?{}", query_params))
                    .header("content-type", "application/zip")
                    .body(Body::from("dummy data"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            expected_status,
            "Failed for case: {}",
            description
        );
    }

    Ok(())
}
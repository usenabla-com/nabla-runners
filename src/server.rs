use anyhow::{anyhow, Result};
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::post,
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use crate::{detection, execution};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::process::Command;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

const MAX_UPLOAD_SIZE: usize = 200 * 1024 * 1024; // 200 MB

#[derive(Debug, Deserialize)]
struct BuildParams {
    owner: String,
    repo: String,
    head_sha: String,
    installation_id: String,
    upload_url: String,
}

#[derive(Debug, Serialize)]
struct BuildResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Clone)]
struct AppState {
    // Add any shared state here if needed
}

impl Default for AppState {
    fn default() -> Self {
        Self {}
    }
}

fn validate_owner_repo(s: &str) -> bool {
    !s.is_empty() 
        && s.len() <= 100 
        && s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn validate_head_sha(s: &str) -> bool {
    s.len() >= 7 
        && s.len() <= 40 
        && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_params(params: &BuildParams) -> Result<()> {
    if !validate_owner_repo(&params.owner) {
        return Err(anyhow!("Invalid owner"));
    }
    
    if !validate_owner_repo(&params.repo) {
        return Err(anyhow!("Invalid repo"));
    }
    
    if !validate_head_sha(&params.head_sha) {
        return Err(anyhow!("Invalid head_sha"));
    }
    
    let installation_id: u64 = params.installation_id.parse()
        .map_err(|_| anyhow!("Invalid installation_id"))?;
    
    if installation_id == 0 {
        return Err(anyhow!("Installation ID must be positive"));
    }
    
    if params.upload_url.is_empty() {
        return Err(anyhow!("Upload URL is required"));
    }
    
    Ok(())
}

async fn setup_workspace() -> Result<std::path::PathBuf> {
    let workspace = std::path::PathBuf::from("/workspace");
    
    // Clean and create workspace directories
    let _ = fs::remove_dir_all(&workspace).await; // Ignore errors if doesn't exist
    fs::create_dir_all(&workspace).await?;
    fs::create_dir_all(workspace.join("build")).await?;
    fs::create_dir_all(workspace.join("out")).await?;

    Ok(workspace)
}

async fn extract_repository_from_base64(base64_data: &str, workspace: &Path) -> Result<std::path::PathBuf> {
    // Decode base64 to get ZIP bytes
    let zip_bytes = general_purpose::STANDARD.decode(base64_data)
        .map_err(|e| anyhow!("Failed to decode base64 data: {}", e))?;

    // Write ZIP bytes to temporary file
    let temp_zip = workspace.join("temp_repo.zip");
    fs::write(&temp_zip, zip_bytes).await?;

    let repo_dir = workspace.join("repo");
    fs::create_dir_all(&repo_dir).await?;

    // Extract ZIP using unzip command
    let output = Command::new("unzip")
        .arg("-q")
        .arg(&temp_zip)
        .arg("-d")
        .arg(&repo_dir)
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!(
            "Failed to extract ZIP: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Clean up temporary ZIP file
    let _ = fs::remove_file(&temp_zip).await;

    // Handle nested directory structure (common with GitHub archives)
    let final_repo_dir = find_actual_repo_dir(&repo_dir).await?;
    
    Ok(final_repo_dir)
}

async fn find_actual_repo_dir(repo_dir: &Path) -> Result<std::path::PathBuf> {
    let mut entries = fs::read_dir(repo_dir).await?;
    let mut dirs = Vec::new();
    
    while let Some(entry) = entries.next_entry().await? {
        if entry.path().is_dir() {
            dirs.push(entry.path());
        }
    }

    // If there's exactly one directory, it's likely the actual repo content
    if dirs.len() == 1 {
        Ok(dirs[0].clone())
    } else {
        // Multiple directories or files at root level, use the extraction directory
        Ok(repo_dir.to_path_buf())
    }
}

async fn package_artifact(artifact_path: &str, workspace: &Path) -> Result<std::path::PathBuf> {
    let artifact_path = Path::new(artifact_path);
    let out_dir = workspace.join("out");
    let artifact_name = artifact_path.file_name()
        .ok_or_else(|| anyhow!("Invalid artifact path"))?;

    // Copy artifact to output directory
    let dest_path = out_dir.join(artifact_name);
    fs::copy(artifact_path, &dest_path).await?;

    // Create ZIP archive
    let zip_path = workspace.join("artifact.zip");
    let output = Command::new("zip")
        .arg("-q")
        .arg(&zip_path)
        .arg(artifact_name)
        .current_dir(&out_dir)
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!(
            "Failed to create ZIP: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(zip_path)
}

async fn upload_artifact(zip_path: &Path, params: &BuildParams) -> Result<()> {
    // URL encode parameters
    let owner = urlencoding::encode(&params.owner);
    let repo = urlencoding::encode(&params.repo);
    let head_sha = urlencoding::encode(&params.head_sha);
    let installation_id = urlencoding::encode(&params.installation_id);

    let upload_url = format!(
        "{}?owner={}&repo={}&head_sha={}&installation_id={}",
        params.upload_url, owner, repo, head_sha, installation_id
    );

    // Read the ZIP file
    let zip_data = fs::read(zip_path).await?;

    // Create HTTP client
    let client = reqwest::Client::new();
    
    // Send request
    let response = client
        .post(&upload_url)
        .header("Content-Type", "application/zip")
        .body(zip_data)
        .send()
        .await?;

    let status = response.status();

    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Upload failed with status {}: {}",
            status,
            error_body
        ));
    }

    Ok(())
}

async fn build_handler(
    State(_state): State<Arc<AppState>>,
    Query(params): Query<BuildParams>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<BuildResponse>, (StatusCode, Json<BuildResponse>)> {
    // Validate content type
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    
    let is_base64 = content_type.starts_with("application/base64") || content_type.starts_with("text/plain");
    let is_zip = content_type.starts_with("application/zip") || content_type.starts_with("application/octet-stream");
    
    if !is_base64 && !is_zip {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(BuildResponse {
                status: "error".to_string(),
                output: None,
                error: Some("unsupported media type - use application/zip or application/base64".to_string()),
            }),
        ));
    }

    // Validate size
    if body.len() > MAX_UPLOAD_SIZE {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(BuildResponse {
                status: "error".to_string(),
                output: None,
                error: Some("payload too large".to_string()),
            }),
        ));
    }

    // Validate parameters
    if let Err(e) = validate_params(&params) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(BuildResponse {
                status: "error".to_string(),
                output: None,
                error: Some(format!("invalid query params: {}", e)),
            }),
        ));
    }

    info!("Build request: {}/{} @ {}", params.owner, params.repo, params.head_sha);

    // Process repository data
    let repo_data_base64 = if is_base64 {
        // Data is already BASE64 encoded
        String::from_utf8(body.to_vec()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(BuildResponse {
                    status: "error".to_string(),
                    output: None,
                    error: Some(format!("invalid UTF-8 in base64 data: {}", e)),
                }),
            )
        })?
    } else {
        // ZIP bytes - need to encode to BASE64
        general_purpose::STANDARD.encode(&body)
    };

    // Execute build
    match execute_build_pipeline(&repo_data_base64, &params).await {
        Ok(output) => Ok(Json(BuildResponse {
            status: "accepted".to_string(),
            output: Some(output),
            error: None,
        })),
        Err(e) => {
            error!("Build failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(BuildResponse {
                    status: "error".to_string(),
                    output: None,
                    error: Some(format!("build failed: {}", e)),
                }),
            ))
        }
    }
}

async fn execute_build_pipeline(repo_data_base64: &str, params: &BuildParams) -> Result<String> {
    let mut output_log = Vec::new();
    
    // Setup workspace
    let workspace = setup_workspace().await?;
    output_log.push(format!("Workspace ready: {}", workspace.display()));

    // Extract repository from base64 data
    let repo_dir = extract_repository_from_base64(repo_data_base64, &workspace).await?;
    output_log.push(format!("Repository extracted to: {}", repo_dir.display()));

    // Detect build system
    let build_system = detection::detect_build_system(&repo_dir).await
        .ok_or_else(|| anyhow!("Unsupported or undetected build system"))?;
    output_log.push(format!("Detected build system: {:?}", build_system));

    // Execute build
    output_log.push("Starting build...".to_string());
    let build_result = execution::execute_build(&repo_dir, build_system).await?;

    if !build_result.success {
        let error_msg = build_result.error_output.unwrap_or_else(|| "Unknown build error".to_string());
        output_log.push(format!("Build failed: {}", error_msg));
        return Err(anyhow!("Build failed: {}", error_msg));
    }

    let artifact_path = build_result.output_path
        .ok_or_else(|| anyhow!("Build succeeded but no artifact path returned"))?;
    output_log.push(format!("Build completed successfully. Artifact: {}", artifact_path));

    // Package artifact
    let packaged_artifact = package_artifact(&artifact_path, &workspace).await?;
    output_log.push(format!("Artifact packaged: {}", packaged_artifact.display()));

    // Upload artifact
    upload_artifact(&packaged_artifact, params).await?;
    output_log.push("Upload successful".to_string());

    // Return last 4000 chars of logs to keep response small
    let full_output = output_log.join("\n");
    let tail = if full_output.len() > 4000 {
        full_output.chars().skip(full_output.len() - 4000).collect()
    } else {
        full_output
    };

    Ok(tail)
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "nabla-runner",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

pub fn create_app() -> Router {
    let state = Arc::new(AppState::default());

    Router::new()
        .route("/build", post(build_handler))
        .route("/health", axum::routing::get(health_handler))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive())
                .into_inner(),
        )
        .with_state(state)
}

pub async fn run_server(port: u16) -> Result<()> {
    let app = create_app();
    
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Server running on http://0.0.0.0:{}", port);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
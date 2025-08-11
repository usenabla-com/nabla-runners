use anyhow::{anyhow, Result};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use crate::{detection, execution};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::process::Command;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{error, info};


#[derive(Debug, Deserialize)]
struct BuildParams {
    archive_url: String,
    owner: String,
    repo: String,
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

fn validate_archive_url(url: &str) -> bool {
    url.starts_with("https://") && url.len() > 8 && url.len() <= 500
}

fn validate_params(params: &BuildParams) -> Result<()> {
    if !validate_archive_url(&params.archive_url) {
        return Err(anyhow!("Invalid archive_url - must be a valid HTTPS URL"));
    }
    
    if params.owner.is_empty() || params.owner.len() > 100 {
        return Err(anyhow!("Invalid owner - must be 1-100 characters"));
    }
    
    if params.repo.is_empty() || params.repo.len() > 100 {
        return Err(anyhow!("Invalid repo - must be 1-100 characters"));
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
    // Use /workspace in production (Docker/Linux), temp dir for local development
    let workspace = if std::path::Path::new("/workspace").exists() {
        std::path::PathBuf::from("/workspace")
    } else {
        // For local development, use a temp directory
        let temp_base = std::env::temp_dir().join("nabla-workspace");
        fs::create_dir_all(&temp_base).await?;
        temp_base
    };
    
    // Clean and create workspace directories
    let _ = fs::remove_dir_all(&workspace).await; // Ignore errors if doesn't exist
    fs::create_dir_all(&workspace).await?;
    fs::create_dir_all(workspace.join("build")).await?;
    fs::create_dir_all(workspace.join("out")).await?;

    Ok(workspace)
}

async fn fetch_and_extract_repository(archive_url: &str, workspace: &Path) -> Result<std::path::PathBuf> {
    info!("Fetching repository archive from: {}", archive_url);
    
    // Fetch the archive
    let client = reqwest::Client::new();
    let response = client
        .get(archive_url)
        .header("User-Agent", "nabla-runner/0.1.0")
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to fetch repository archive: HTTP {}",
            response.status()
        ));
    }
    
    let archive_bytes = response.bytes().await?;
    
    // Write archive to temporary file
    let temp_archive = workspace.join("temp_repo.tar.gz");
    fs::write(&temp_archive, archive_bytes).await?;
    
    let repo_dir = workspace.join("repo");
    fs::create_dir_all(&repo_dir).await?;
    
    // Extract tarball using tar command
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(&temp_archive)
        .arg("-C")
        .arg(&repo_dir)
        .arg("--strip-components=1")  // Remove the top-level directory from archive
        .output()
        .await?;
    
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to extract tar.gz: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    // Clean up temporary archive file
    let _ = fs::remove_file(&temp_archive).await;
    
    Ok(repo_dir)
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
    let owner_encoded = urlencoding::encode(&params.owner);
    let repo_encoded = urlencoding::encode(&params.repo);
    let installation_id = urlencoding::encode(&params.installation_id);

    let upload_url = format!(
        "{}?owner={}&repo={}&installation_id={}",
        params.upload_url, owner_encoded, repo_encoded, installation_id
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
) -> Result<Json<BuildResponse>, (StatusCode, Json<BuildResponse>)> {
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

    info!("Build request: {}/{} from {}", params.owner, params.repo, params.archive_url);

    // Execute build
    match execute_build_pipeline(&params).await {
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

async fn execute_build_pipeline(params: &BuildParams) -> Result<String> {
    let mut output_log = Vec::new();
    
    // Setup workspace
    let workspace = setup_workspace().await?;
    output_log.push(format!("Workspace ready: {}", workspace.display()));

    // Fetch and extract repository from archive URL
    let repo_dir = fetch_and_extract_repository(&params.archive_url, &workspace).await?;
    output_log.push(format!("Repository fetched and extracted to: {}", repo_dir.display()));

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
use anyhow::{anyhow, Result};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use crate::{detection, execution, jobs::{BuildJob, JobManager, JobStatus}};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::process::Command;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use uuid::Uuid;


#[derive(Debug, Deserialize, Clone)]
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
    job_id: Uuid,
    message: String,
}

#[derive(Debug, Serialize)]
struct JobStatusResponse {
    job_id: Uuid,
    status: JobStatus,
    created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct JobListResponse {
    jobs: Vec<JobStatusResponse>,
}

#[derive(Clone)]
struct AppState {
    job_manager: JobManager,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            job_manager: JobManager::new(),
        }
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
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildParams>,
) -> Result<Json<BuildResponse>, (StatusCode, Json<BuildResponse>)> {
    // Validate parameters
    if let Err(e) = validate_params(&params) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(BuildResponse {
                status: "error".to_string(),
                job_id: Uuid::nil(),
                message: format!("invalid query params: {}", e),
            }),
        ));
    }

    info!("Build request: {}/{} from {}", params.owner, params.repo, params.archive_url);

    // Create new job
    let job = BuildJob::new(
        params.archive_url.clone(),
        params.owner.clone(),
        params.repo.clone(),
        params.installation_id.clone(),
        params.upload_url.clone(),
    );

    let job_id = job.id;
    let job_manager = state.job_manager.clone();

    // Submit job to queue
    job_manager.submit_job(job);

    // Start async build task
    let build_params = params.clone();
    let task_job_manager = job_manager.clone();
    let handle = tokio::spawn(async move {
        execute_build_task(task_job_manager, job_id, build_params).await;
    });

    // Store the task handle
    if let Err(e) = job_manager.start_job(&job_id, handle) {
        error!("Failed to start job: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(BuildResponse {
                status: "error".to_string(),
                job_id: Uuid::nil(),
                message: "Failed to start build job".to_string(),
            }),
        ));
    }

    Ok(Json(BuildResponse {
        status: "accepted".to_string(),
        job_id,
        message: "Build job submitted successfully".to_string(),
    }))
}

async fn job_status_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(job_id): AxumPath<Uuid>,
) -> Result<Json<JobStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    match state.job_manager.get_job(&job_id) {
        Some(job) => Ok(Json(JobStatusResponse {
            job_id: job.id,
            status: job.status,
            created_at: job.created_at,
            started_at: job.started_at,
            completed_at: job.completed_at,
            output: job.output,
            error: job.error,
            artifact_path: job.artifact_path,
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Job not found",
                "job_id": job_id
            })),
        )),
    }
}

async fn job_list_handler(
    State(state): State<Arc<AppState>>,
) -> Json<JobListResponse> {
    let jobs = state.job_manager.list_jobs();
    let job_responses: Vec<JobStatusResponse> = jobs
        .into_iter()
        .map(|job| JobStatusResponse {
            job_id: job.id,
            status: job.status,
            created_at: job.created_at,
            started_at: job.started_at,
            completed_at: job.completed_at,
            output: job.output,
            error: job.error,
            artifact_path: job.artifact_path,
        })
        .collect();

    Json(JobListResponse {
        jobs: job_responses,
    })
}

async fn job_cancel_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(job_id): AxumPath<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.job_manager.cancel_job(&job_id) {
        Ok(()) => Ok(Json(serde_json::json!({
            "message": "Job cancelled successfully",
            "job_id": job_id
        }))),
        Err(e) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("Failed to cancel job: {}", e),
                "job_id": job_id
            })),
        )),
    }
}

async fn execute_build_task(job_manager: JobManager, job_id: Uuid, params: BuildParams) {
    match execute_build_pipeline(&params).await {
        Ok((output, artifact_path)) => {
            // Build succeeded
            let _ = job_manager.update_job(&job_id, |job| {
                job.complete(output, Some(artifact_path));
            });
        }
        Err(e) => {
            // Build failed
            let error_msg = e.to_string();
            error!("Build job {} failed: {}", job_id, error_msg);
            let _ = job_manager.update_job(&job_id, |job| {
                job.fail(error_msg);
            });
        }
    }
}

async fn execute_build_pipeline(params: &BuildParams) -> Result<(String, String)> {
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

    Ok((tail, artifact_path))
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
        .route("/jobs/:job_id", get(job_status_handler))
        .route("/jobs/:job_id/cancel", post(job_cancel_handler))
        .route("/jobs", get(job_list_handler))
        .route("/health", get(health_handler))
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
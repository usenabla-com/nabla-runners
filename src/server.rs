use anyhow::{anyhow, Result};
use axum::{
    extract::{Json as JsonExtract, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use crate::{detection, execution, jobs::{BuildJob, SingleJobManager}};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::process::Command;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use uuid::Uuid;
use std::env;
use std::collections::HashSet;
use base64::Engine;


#[derive(Debug, Deserialize, Clone)]
struct BuildParams {
    job_id: String,
    archive_url: String,
    owner: String,
    repo: String,
    installation_id: String,
    build_config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct BuildResponse {
    status: String,
    job_id: Uuid,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_data: Option<String>, // Base64 encoded binary
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_output: Option<String>,
}


#[derive(Debug, Clone)]
struct CustomerConfig {
    customer_id: String,
    allowed_installation_ids: HashSet<String>,
}

impl CustomerConfig {
    fn from_env() -> Self {
        let customer_id = env::var("CUSTOMER_ID").unwrap_or_else(|_| "default".to_string());
        
        let installation_ids = env::var("ALLOWED_INSTALLATION_IDS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .collect::<HashSet<_>>();

        info!("Customer config initialized: customer_id={}, allowed_installations={:?}", 
              customer_id, installation_ids);

        Self {
            customer_id,
            allowed_installation_ids: installation_ids,
        }
    }

    fn validate_installation_id(&self, installation_id: &str) -> bool {
        // If no specific installations configured, allow all (backward compatibility)
        if self.allowed_installation_ids.is_empty() {
            warn!("No ALLOWED_INSTALLATION_IDS configured - allowing all installation IDs");
            return true;
        }
        
        let is_allowed = self.allowed_installation_ids.contains(installation_id);
        
        if !is_allowed {
            warn!("Installation ID {} not allowed for customer {}", installation_id, self.customer_id);
        }
        
        is_allowed
    }
}

#[derive(Clone)]
struct AppState {
    job_manager: Arc<std::sync::RwLock<SingleJobManager>>,
    customer_config: CustomerConfig,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            job_manager: Arc::new(std::sync::RwLock::new(SingleJobManager::new())),
            customer_config: CustomerConfig::from_env(),
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
    
    
    Ok(())
}

async fn setup_workspace() -> Result<std::path::PathBuf> {
    // Create unique workspace per job to avoid conflicts
    let job_id = Uuid::new_v4();
    
    let workspace = if std::path::Path::new("/workspace").exists() {
        std::path::PathBuf::from("/workspace").join(format!("job-{}", job_id))
    } else {
        // For local development, use a temp directory
        let temp_base = std::env::temp_dir().join("nabla-workspace");
        temp_base.join(format!("job-{}", job_id))
    };
    
    // Create workspace directories
    fs::create_dir_all(&workspace).await?;
    fs::create_dir_all(workspace.join("build")).await?;
    fs::create_dir_all(workspace.join("out")).await?;

    info!("Created workspace: {}", workspace.display());
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



async fn build_handler(
    State(state): State<Arc<AppState>>,
    JsonExtract(params): JsonExtract<BuildParams>,
) -> Result<Json<BuildResponse>, (StatusCode, Json<BuildResponse>)> {
    // Validate parameters
    if let Err(e) = validate_params(&params) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(BuildResponse {
                status: "error".to_string(),
                job_id: Uuid::nil(),
                message: format!("invalid request: {}", e),
                artifact_data: None,
                artifact_filename: None,
                build_output: None,
            }),
        ));
    }

    // Validate installation ID for this customer
    if !state.customer_config.validate_installation_id(&params.installation_id) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(BuildResponse {
                status: "error".to_string(),
                job_id: Uuid::nil(),
                message: format!("Installation ID {} not allowed for this customer", params.installation_id),
                artifact_data: None,
                artifact_filename: None,
                build_output: None,
            }),
        ));
    }

    info!("Build request: {}/{} from {} (installation: {}, customer: {})", 
          params.owner, params.repo, params.archive_url, 
          params.installation_id, state.customer_config.customer_id);

    // Create new job
    let job = BuildJob::new(
        params.archive_url.clone(),
        params.owner.clone(),
        params.repo.clone(),
        params.installation_id.clone(),
        String::new(), // No upload_url needed anymore
        Some(state.customer_config.customer_id.clone()),
    );

    let job_id = job.id;
    
    // Set the single job
    state.job_manager.write().unwrap().set_job(job);

    // Execute build task synchronously and return result
    info!("Starting build job {}", job_id);
    
    // Update job status to running
    state.job_manager.write().unwrap().update_job(|job| job.start());
    
    match execute_build_pipeline(&params).await {
        Ok((output, artifact_base64, artifact_filename, _workspace)) => {
            // Build succeeded
            info!("Build job {} completed successfully", job_id);
            state.job_manager.write().unwrap().update_job(|job| {
                job.complete(output.clone(), Some(artifact_filename.clone()));
            });
            
            Ok(Json(BuildResponse {
                status: "completed".to_string(),
                job_id,
                message: "Build completed successfully".to_string(),
                artifact_data: Some(artifact_base64),
                artifact_filename: Some(artifact_filename),
                build_output: Some(output),
            }))
        }
        Err(e) => {
            // Build failed
            let error_msg = e.to_string();
            error!("Build job {} failed: {}", job_id, error_msg);
            
            state.job_manager.write().unwrap().update_job(|job| {
                job.fail(error_msg.clone());
            });
            
            Ok(Json(BuildResponse {
                status: "failed".to_string(),
                job_id,
                message: format!("Build failed: {}", error_msg),
                artifact_data: None,
                artifact_filename: None,
                build_output: Some(error_msg),
            }))
        }
    }
}



async fn execute_build_pipeline(params: &BuildParams) -> Result<(String, String, String, std::path::PathBuf)> {
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

    // Read artifact and encode as base64
    let artifact_bytes = fs::read(&artifact_path).await?;
    let artifact_base64 = base64::engine::general_purpose::STANDARD.encode(&artifact_bytes);
    output_log.push(format!("Artifact encoded to base64 ({} bytes)", artifact_bytes.len()));

    // Extract filename from path
    let artifact_filename = Path::new(&artifact_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact.bin")
        .to_string();

    // Return last 4000 chars of logs to keep response manageable
    let full_output = output_log.join("\n");
    let tail = if full_output.len() > 4000 {
        full_output.chars().skip(full_output.len() - 4000).collect()
    } else {
        full_output
    };

    Ok((tail, artifact_base64, artifact_filename, workspace))
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
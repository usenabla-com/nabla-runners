use anyhow::Result;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildJob {
    pub id: Uuid,
    pub status: JobStatus,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub archive_url: String,
    pub owner: String,
    pub repo: String,
    pub installation_id: String,
    pub customer_name: Option<String>,
    pub upload_url: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub artifact_path: Option<String>,
}

impl BuildJob {
    pub fn new(
        archive_url: String,
        owner: String,
        repo: String,
        installation_id: String,
        upload_url: String,
        customer_name: Option<String>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id: Uuid::new_v4(),
            status: JobStatus::Queued,
            created_at: now,
            started_at: None,
            completed_at: None,
            archive_url,
            owner,
            repo,
            installation_id,
            customer_name,
            upload_url,
            output: None,
            error: None,
            artifact_path: None,
        }
    }

    pub fn start(&mut self) {
        self.status = JobStatus::Running;
        self.started_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }

    pub fn complete(&mut self, output: String, artifact_path: Option<String>) {
        self.status = JobStatus::Completed;
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        self.output = Some(output);
        self.artifact_path = artifact_path;
    }

    pub fn fail(&mut self, error: String) {
        self.status = JobStatus::Failed;
        self.completed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        self.error = Some(error);
    }
}

pub struct JobManager {
    jobs: Arc<RwLock<HashMap<Uuid, BuildJob>>>,
    handles: Arc<RwLock<HashMap<Uuid, JoinHandle<()>>>>,
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn submit_job(&self, job: BuildJob) -> Uuid {
        let job_id = job.id;
        self.jobs.write().insert(job_id, job);
        job_id
    }

    pub fn get_job(&self, job_id: &Uuid) -> Option<BuildJob> {
        self.jobs.read().get(job_id).cloned()
    }

    pub fn update_job<F>(&self, job_id: &Uuid, update_fn: F) -> Result<()>
    where
        F: FnOnce(&mut BuildJob),
    {
        let mut jobs = self.jobs.write();
        if let Some(job) = jobs.get_mut(job_id) {
            update_fn(job);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Job not found: {}", job_id))
        }
    }

    pub fn start_job(&self, job_id: &Uuid, handle: JoinHandle<()>) -> Result<()> {
        // Update job status to running
        self.update_job(job_id, |job| job.start())?;
        
        // Store the task handle
        self.handles.write().insert(*job_id, handle);
        
        Ok(())
    }

    pub fn list_jobs(&self) -> Vec<BuildJob> {
        self.jobs.read().values().cloned().collect()
    }

    pub fn cleanup_completed_jobs(&self, max_age_seconds: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut jobs_to_remove = Vec::new();
        
        {
            let jobs = self.jobs.read();
            for (id, job) in jobs.iter() {
                if let Some(completed_at) = job.completed_at {
                    if now - completed_at > max_age_seconds {
                        jobs_to_remove.push(*id);
                    }
                }
            }
        }

        let mut jobs = self.jobs.write();
        let mut handles = self.handles.write();
        
        for job_id in jobs_to_remove {
            jobs.remove(&job_id);
            if let Some(handle) = handles.remove(&job_id) {
                handle.abort();
            }
        }
    }

    pub fn cancel_job(&self, job_id: &Uuid) -> Result<()> {
        // Cancel the task
        if let Some(handle) = self.handles.write().remove(job_id) {
            handle.abort();
        }

        // Update job status
        self.update_job(job_id, |job| {
            job.fail("Job cancelled".to_string());
        })?;

        Ok(())
    }
}

impl Clone for JobManager {
    fn clone(&self) -> Self {
        Self {
            jobs: self.jobs.clone(),
            handles: self.handles.clone(),
        }
    }
}
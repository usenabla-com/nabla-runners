use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
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

pub struct SingleJobManager {
    current_job: Option<BuildJob>,
}

impl Default for SingleJobManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SingleJobManager {
    pub fn new() -> Self {
        Self {
            current_job: None,
        }
    }

    pub fn set_job(&mut self, job: BuildJob) {
        self.current_job = Some(job);
    }

    pub fn get_job(&self) -> Option<&BuildJob> {
        self.current_job.as_ref()
    }

    pub fn update_job<F>(&mut self, update_fn: F)
    where
        F: FnOnce(&mut BuildJob),
    {
        if let Some(job) = &mut self.current_job {
            update_fn(job);
        }
    }
}

impl Clone for SingleJobManager {
    fn clone(&self) -> Self {
        Self {
            current_job: self.current_job.clone(),
        }
    }
}
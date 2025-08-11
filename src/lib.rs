pub mod core;
pub mod detection;
pub mod execution;
pub mod intelligent_build;
pub mod server;

use async_trait::async_trait;
use anyhow::Result;
use crate::core::{BuildResult, BuildSystem};
use std::path::Path;

#[async_trait]
pub trait BuildRunner {
    async fn detect(&self, path: &Path) -> Option<BuildSystem>;
    async fn build(&self, path: &Path, system: BuildSystem) -> Result<BuildResult>;
}

pub struct FirmwareBuildRunner;

impl Default for FirmwareBuildRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl FirmwareBuildRunner {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl BuildRunner for FirmwareBuildRunner {
    async fn detect(&self, path: &Path) -> Option<BuildSystem> {
        detection::detect_build_system(path).await
    }

    async fn build(&self, path: &Path, system: BuildSystem) -> Result<BuildResult> {
        execution::execute_build(path, system).await
    }
}
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildSystem {
    Makefile,
    CMake,
    PlatformIO,
    ZephyrWest,
    STM32CubeIDE,
    SCons,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub success: bool,
    pub output_path: Option<String>,
    pub target_format: Option<String>,
    pub error_output: Option<String>,
    pub build_system: BuildSystem,
    pub duration_ms: u64,
}
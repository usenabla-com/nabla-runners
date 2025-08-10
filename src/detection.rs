use nabla_core::BuildSystem;
use std::path::Path;
use tokio::fs;

pub async fn detect_build_system(path: &Path) -> Option<BuildSystem> {
    if path.join("Cargo.toml").exists() {
        return Some(BuildSystem::Cargo);
    }

    if path.join("Makefile").exists() || path.join("makefile").exists() {
        return Some(BuildSystem::Makefile);
    }

    if path.join("CMakeLists.txt").exists() {
        return Some(BuildSystem::CMake);
    }

    if path.join("platformio.ini").exists() {
        return Some(BuildSystem::PlatformIO);
    }

    if path.join("west.yml").exists() || path.join(".west").is_dir() {
        return Some(BuildSystem::ZephyrWest);
    }

    if has_stm32_project_files(path).await {
        return Some(BuildSystem::STM32CubeIDE);
    }

    if path.join("SConstruct").exists() || path.join("SConscript").exists() {
        return Some(BuildSystem::SCons);
    }

    None
}

async fn has_stm32_project_files(path: &Path) -> bool {
    let extensions = [".project", ".cproject"];
    
    for ext in &extensions {
        if let Ok(mut entries) = fs::read_dir(path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Some(file_name) = entry.file_name().to_str() {
                    if file_name.ends_with(ext) {
                        return true;
                    }
                }
            }
        }
    }
    
    false
}
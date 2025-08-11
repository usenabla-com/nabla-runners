use crate::core::{BuildResult, BuildSystem};
use crate::intelligent_build::{IntelligentBuilder, BuildFixDatabase};
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use std::time::Instant;
use tokio::fs;
use std::os::unix::fs::PermissionsExt;
use std::collections::HashMap;

pub async fn execute_build(path: &Path, system: BuildSystem) -> Result<BuildResult> {
    let intelligent_builder = create_intelligent_builder().await;
    intelligent_builder.execute_with_fallbacks(path, system).await
}

async fn create_intelligent_builder() -> IntelligentBuilder {
    let fix_db = BuildFixDatabase {
        error_patterns: HashMap::new(),
        successful_configs: HashMap::new(),
    };
    IntelligentBuilder::new(fix_db)
}

fn create_build_result(output_path: String, target_format: String, build_system: BuildSystem, start_time: Instant) -> BuildResult {
    BuildResult {
        success: true,
        output_path: Some(output_path),
        target_format: Some(target_format),
        error_output: None,
        build_system,
        duration_ms: start_time.elapsed().as_millis() as u64,
    }
}

/// Helper function to find executable files in a directory
async fn find_executable_in_dir(dir: &Path) -> Result<PathBuf> {
    tracing::debug!("Searching for executable in directory: {:?}", dir);
    
    if !dir.exists() {
        return Err(anyhow!("Directory does not exist: {:?}", dir));
    }
    
    let mut entries = fs::read_dir(dir).await?;
    let mut candidates = Vec::new();
    
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_file() {
            let metadata = fs::metadata(&path).await?;
            let permissions = metadata.permissions();
            
            // Check if file is executable (Unix-specific)
            if permissions.mode() & 0o111 != 0 {
                // Additional check: ensure it's not a script or text file
                if !path.extension().map_or(false, |ext| 
                    ext == "sh" || ext == "py" || ext == "txt" || ext == "md" || ext == "yml" || ext == "yaml" || ext == "json"
                ) {
                    tracing::debug!("Found executable candidate: {:?}", path);
                    candidates.push(path.clone());
                    return Ok(path);
                }
            }
        }
    }
    
    if !candidates.is_empty() {
        tracing::debug!("Found {} executable candidates, returning first: {:?}", candidates.len(), candidates[0]);
        return Ok(candidates[0].clone());
    }
    
    Err(anyhow!("No executable binary found in directory: {:?}", dir))
}

/// Helper function to find binary files by common patterns
async fn find_binary_by_patterns(dir: &Path, patterns: &[&str]) -> Result<PathBuf> {
    tracing::debug!("Searching for binary in {:?} with patterns: {:?}", dir, patterns);
    
    if !dir.exists() {
        tracing::warn!("Directory does not exist: {:?}", dir);
        return Err(anyhow!("Directory does not exist: {:?}", dir));
    }
    
    // First, try exact pattern matches
    for pattern in patterns {
        let path = dir.join(pattern);
        tracing::trace!("Checking exact path: {:?}", path);
        if path.exists() && path.is_file() {
            tracing::info!("Found binary at exact path: {:?}", path);
            return Ok(path);
        }
        
        // Also check with common extensions
        for ext in &[".elf", ".bin", ".hex", ".out", ""] {
            let path_with_ext = if ext.is_empty() {
                dir.join(pattern)
            } else {
                dir.join(format!("{}{}", pattern, ext))
            };
            tracing::trace!("Checking path with extension: {:?}", path_with_ext);
            if path_with_ext.exists() && path_with_ext.is_file() {
                tracing::info!("Found binary with extension: {:?}", path_with_ext);
                return Ok(path_with_ext);
            }
        }
    }
    
    // Log directory contents for debugging
    tracing::debug!("No pattern match found. Listing directory contents:");
    if let Ok(mut entries) = fs::read_dir(dir).await {
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                tracing::debug!("  File: {:?}", path.file_name());
            }
        }
    }
    
    // Fallback to finding any executable
    tracing::debug!("Falling back to finding any executable in directory");
    find_executable_in_dir(dir).await
}

pub async fn build_makefile_original(path: &Path) -> Result<BuildResult> {
    let start_time = Instant::now();
    // First, try to get the output name from make (for future enhancement)
    let _dry_run = Command::new("make")
        .arg("-n")
        .arg("--print-data-base")
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;
    
    // Run the actual build
    let output = Command::new("make")
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!("Make build failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // Common output locations and names for firmware projects
    let common_patterns = [
        "firmware", "main", "app", "output", "build/firmware",
        "bin/firmware", "out/firmware", "dist/firmware"
    ];
    
    // Try to find the binary
    let binary_path = find_binary_by_patterns(path, &common_patterns)
        .await
        .map_err(|_| anyhow!("Could not find built binary after make"))?;
    
    Ok(create_build_result(binary_path.to_string_lossy().to_string(), "bin".to_string(), BuildSystem::Makefile, start_time))
}

pub async fn build_cmake_original(path: &Path) -> Result<BuildResult> {
    let start_time = Instant::now();
    let build_dir = path.join("build");
    tokio::fs::create_dir_all(&build_dir).await?;

    let configure = Command::new("cmake")
        .arg("..")
        .current_dir(&build_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !configure.status.success() {
        return Err(anyhow!("CMake configure failed: {}", String::from_utf8_lossy(&configure.stderr)));
    }

    let build = Command::new("cmake")
        .arg("--build")
        .arg(".")
        .current_dir(&build_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !build.status.success() {
        return Err(anyhow!("CMake build failed: {}", String::from_utf8_lossy(&build.stderr)));
    }

    // CMake typically puts executables directly in build/ or in subdirectories
    let common_patterns = [
        "firmware", "main", "app", 
        "bin/firmware", "bin/main",
        "src/firmware", "src/main"
    ];
    
    let binary_path = find_binary_by_patterns(&build_dir, &common_patterns)
        .await
        .map_err(|_| anyhow!("Could not find built binary in CMake build directory"))?;
    
    Ok(create_build_result(binary_path.to_string_lossy().to_string(), "elf".to_string(), BuildSystem::CMake, start_time))
}

pub async fn build_platformio_original(path: &Path) -> Result<BuildResult> {
    let start_time = Instant::now();
    let output = Command::new("pio")
        .arg("run")
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!("PlatformIO build failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // PlatformIO creates builds per environment
    let build_base = path.join(".pio/build");
    
    // Find the first environment directory
    let mut entries = fs::read_dir(&build_base).await?;
    while let Some(entry) = entries.next_entry().await? {
        let env_path = entry.path();
        if env_path.is_dir() {
            // Look for firmware files in this environment
            let patterns = ["firmware", "program"];
            for pattern in &patterns {
                for ext in &[".hex", ".bin", ".elf"] {
                    let firmware_path = env_path.join(format!("{}{}", pattern, ext));
                    if firmware_path.exists() && firmware_path.is_file() {
                        let format = ext.trim_start_matches('.').to_string();
                        return Ok(create_build_result(firmware_path.to_string_lossy().to_string(), format, BuildSystem::PlatformIO, start_time));
                    }
                }
            }
        }
    }
    
    Err(anyhow!("Could not find PlatformIO build output"))
}

pub async fn build_zephyr_original(path: &Path) -> Result<BuildResult> {
    let start_time = Instant::now();
    let output = Command::new("west")
        .arg("build")
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!("Zephyr build failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // Zephyr puts the binary in build/zephyr/zephyr.elf
    let zephyr_elf = path.join("build/zephyr/zephyr.elf");
    if zephyr_elf.exists() && zephyr_elf.is_file() {
        return Ok(create_build_result(zephyr_elf.to_string_lossy().to_string(), "elf".to_string(), BuildSystem::ZephyrWest, start_time));
    }
    
    // Alternative locations
    let alt_patterns = [
        "build/zephyr/zephyr.bin",
        "build/zephyr/zephyr.hex",
        "build/app.elf"
    ];
    
    for pattern in &alt_patterns {
        let alt_path = path.join(pattern);
        if alt_path.exists() && alt_path.is_file() {
            let format = alt_path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin")
                .to_string();
            return Ok(create_build_result(alt_path.to_string_lossy().to_string(), format, BuildSystem::ZephyrWest, start_time));
        }
    }
    
    Err(anyhow!("Could not find Zephyr build output"))
}

pub async fn build_stm32_original(_path: &Path) -> Result<BuildResult> {
    let start_time = Instant::now();
    // STM32CubeIDE typically requires IDE integration
    // However, if using STM32CubeMX with Makefile generation:
    
    let output = Command::new("make")
        .arg("-f")
        .arg("STM32Make.make") // Common STM32 makefile name
        .current_dir(_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;
    
    if let Ok(output) = output {
        if output.status.success() {
            // STM32 builds typically create .elf, .bin, and .hex files
            let build_dir = _path.join("build");
            let patterns = [
                "*.elf",
                "Debug/*.elf",
                "Release/*.elf"
            ];
            
            for pattern in &patterns {
                let search_path = if pattern.contains('/') {
                    _path.join(pattern.replace("*.elf", ""))
                } else {
                    build_dir.clone()
                };
                
                if let Ok(binary) = find_executable_in_dir(&search_path).await {
                    return Ok(create_build_result(binary.to_string_lossy().to_string(), "elf".to_string(), BuildSystem::STM32CubeIDE, start_time));
                }
            }
        }
    }
    
    Err(anyhow!("STM32CubeIDE build not implemented - requires IDE integration or STM32CubeMX Makefile"))
}

pub async fn build_scons_original(path: &Path) -> Result<BuildResult> {
    let start_time = Instant::now();
    let output = Command::new("scons")
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!("SCons build failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // SCons output location varies by SConstruct configuration
    // Common patterns:
    let patterns = [
        "build/firmware",
        "build/main", 
        "firmware",
        "main",
        "output/firmware",
        "bin/firmware"
    ];
    
    let binary_path = find_binary_by_patterns(path, &patterns)
        .await
        .map_err(|_| anyhow!("Could not find SCons build output"))?;
    
    Ok(create_build_result(binary_path.to_string_lossy().to_string(), "bin".to_string(), BuildSystem::SCons, start_time))
}
use crate::core::{BuildResult, BuildSystem};
use anyhow::{Result, anyhow};
use std::path::Path;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use std::process::Stdio;
use tokio::fs;

  #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
  pub enum BuildStrategy {
      Default,
      ToolchainFallback(String),
      ConfigPatch(HashMap<String, String>),
      DependencyResolution(Vec<String>),
      ArchitectureSwitch(String),
      VersionDowngrade(String),
  }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub toolchain: Option<String>,
    pub environment: HashMap<String, String>,
    pub build_flags: Vec<String>,
}

  #[derive(Debug, Serialize, Deserialize)]
  pub struct BuildFixDatabase {
      pub error_patterns: HashMap<String, Vec<BuildStrategy>>,
      pub successful_configs: HashMap<String, BuildConfig>,
  }

  pub struct IntelligentBuilder {
      fix_db: BuildFixDatabase,
  }

  impl IntelligentBuilder {
      pub fn new(fix_db: BuildFixDatabase) -> Self {
          Self { fix_db }
      }
  }

  impl IntelligentBuilder {
      pub async fn execute_with_fallbacks(&self, path: &Path, system:
  BuildSystem) -> Result<BuildResult> {
          let mut attempts = vec![BuildStrategy::Default];
          let mut last_error = None;
          let mut attempt_index = 0;

          while attempt_index < attempts.len() {
              let strategy = attempts[attempt_index].clone();
              match self.execute_with_strategy(path, system, strategy.clone()).await {
                  Ok(result) if result.success => {
                      self.record_success(path, system, strategy).await;
                      return Ok(result);
                  },
                  Err(e) => {
                      last_error = Some(anyhow::anyhow!(e.to_string()));
                      if let Some(additional_strategies) =
  self.analyze_error(&e.to_string(), system) {
                          attempts.extend(additional_strategies);
                      }
                  },
                  Ok(failed_result) => {
                      if let Some(error_output) =
  &failed_result.error_output {
                          if let Some(additional_strategies) =
  self.analyze_error(error_output, system) {
                              attempts.extend(additional_strategies);
                          }
                      }
                      last_error = Some(anyhow!("Build failed: {}",
  failed_result.error_output.unwrap_or_default()));
                  }
              }
              attempt_index += 1;
          }

          Err(last_error.unwrap_or_else(|| anyhow!("All build strategies failed")))
      }

      async fn execute_with_strategy(&self, path: &Path, system:
  BuildSystem, strategy: BuildStrategy) -> Result<BuildResult> {
          match system {
              BuildSystem::PlatformIO =>
  self.build_platformio_with_strategy(path, strategy).await,
              BuildSystem::CMake => self.build_cmake_with_strategy(path,
  strategy).await,
              BuildSystem::Makefile =>
  self.build_makefile_with_strategy(path, strategy).await,
              BuildSystem::ZephyrWest =>
  self.build_zephyr_with_strategy(path, strategy).await,
              BuildSystem::STM32CubeIDE =>
  self.build_stm32_with_strategy(path, strategy).await,
              BuildSystem::SCons => self.build_scons_with_strategy(path,
  strategy).await,
          }
      }

      // PlatformIO intelligent strategies
      async fn build_platformio_with_strategy(&self, path: &Path, strategy:
   BuildStrategy) -> Result<BuildResult> {
          match strategy {
              BuildStrategy::Default =>
  self.build_platformio_default(path).await,
              BuildStrategy::ConfigPatch(patches) => {
                  self.patch_platformio_config(path, patches).await?;
                  self.build_platformio_default(path).await
              },
              BuildStrategy::VersionDowngrade(version) => {
                  let patches = HashMap::from([
                      ("platform".to_string(), format!("espressif32@{}",
  version))
                  ]);
                  self.patch_platformio_config(path, patches).await?;
                  self.build_platformio_default(path).await
              },
              BuildStrategy::ArchitectureSwitch(arch) => {
                  self.build_in_container(path, BuildSystem::PlatformIO,
  arch).await
              },
              _ => self.build_platformio_default(path).await,
          }
      }

      // CMake intelligent strategies  
      async fn build_cmake_with_strategy(&self, path: &Path, strategy:
  BuildStrategy) -> Result<BuildResult> {
          match strategy {
              BuildStrategy::Default =>
  self.build_cmake_default(path).await,
              BuildStrategy::ToolchainFallback(toolchain) => {
                  self.build_cmake_with_toolchain(path, &toolchain).await
              },
              BuildStrategy::ConfigPatch(patches) => {
                  self.patch_cmake_config(path, patches).await?;
                  self.build_cmake_default(path).await
              },
              BuildStrategy::DependencyResolution(deps) => {
                  self.install_cmake_dependencies(path, deps).await?;
                  self.build_cmake_default(path).await
              },
              _ => self.build_cmake_default(path).await,
          }
      }

      // Universal error analysis
      pub fn analyze_error(&self, error: &str, system: BuildSystem) ->
  Option<Vec<BuildStrategy>> {
          match system {
              BuildSystem::PlatformIO =>
  self.analyze_platformio_error(error),
              BuildSystem::CMake => self.analyze_cmake_error(error),
              BuildSystem::Makefile => self.analyze_makefile_error(error),
              BuildSystem::ZephyrWest => self.analyze_zephyr_error(error),
              BuildSystem::STM32CubeIDE => self.analyze_stm32_error(error),
              BuildSystem::SCons => self.analyze_scons_error(error),
          }
      }

      fn analyze_platformio_error(&self, error: &str) ->
  Option<Vec<BuildStrategy>> {
          let mut strategies = Vec::new();

          if error.contains("Could not install package") &&
  error.contains("framework-arduinoespressif32") {

  strategies.push(BuildStrategy::VersionDowngrade("5.4.0".to_string()));

  strategies.push(BuildStrategy::ArchitectureSwitch("amd64".to_string()));

              let patches = HashMap::from([
                  ("platform_packages".to_string(),
  "framework-arduinoespressif32@3.20014.231204".to_string())
              ]);
              strategies.push(BuildStrategy::ConfigPatch(patches));
          }

          if error.contains("linux_x86_64") {

  strategies.push(BuildStrategy::ArchitectureSwitch("arm64".to_string()));
          }

          if strategies.is_empty() { None } else { Some(strategies) }
      }

      fn analyze_cmake_error(&self, error: &str) ->
  Option<Vec<BuildStrategy>> {
          let mut strategies = Vec::new();

          if error.contains("Could not find") && error.contains("compiler")
   {

  strategies.push(BuildStrategy::ToolchainFallback("gcc".to_string()));

  strategies.push(BuildStrategy::ToolchainFallback("clang".to_string()));
          }

          if error.contains("CMAKE_TOOLCHAIN_FILE") {
              let patches = HashMap::from([
                  ("CMAKE_TOOLCHAIN_FILE".to_string(),
  "/usr/local/share/cmake/toolchain.cmake".to_string())
              ]);
              strategies.push(BuildStrategy::ConfigPatch(patches));
          }

          if error.contains("No such file or directory") &&
  error.contains("CMakeCache.txt") {
              // Clean build directory and retry
              strategies.push(BuildStrategy::ConfigPatch(HashMap::new()));
  // Triggers cache clean
          }

          if strategies.is_empty() { None } else { Some(strategies) }
      }

      fn analyze_makefile_error(&self, error: &str) ->
  Option<Vec<BuildStrategy>> {
          let mut strategies = Vec::new();

          if error.contains("No such file or directory") &&
  (error.contains("gcc") || error.contains("make")) {
              strategies.push(BuildStrategy::DependencyResolution(vec![
                  "build-essential".to_string(),
                  "gcc-arm-none-eabi".to_string()
              ]));
          }

          if error.contains("Permission denied") {
              strategies.push(BuildStrategy::ArchitectureSwitch("privileged".to_string()));
          }

          if strategies.is_empty() { None } else { Some(strategies) }
      }

      fn analyze_zephyr_error(&self, error: &str) ->
  Option<Vec<BuildStrategy>> {
          let mut strategies = Vec::new();

          if error.contains("west") && error.contains("not found") {
              strategies.push(BuildStrategy::DependencyResolution(vec!["west".to_string()]));
          }

          if error.contains("ZEPHYR_BASE") {
              let patches = HashMap::from([
                  ("ZEPHYR_BASE".to_string(), "/opt/zephyr".to_string())
              ]);
              strategies.push(BuildStrategy::ConfigPatch(patches));
          }

          if error.contains("board") && error.contains("not supported") {
              strategies.push(BuildStrategy::ConfigPatch(HashMap::from([
                  ("board".to_string(), "qemu_x86".to_string())
              ])));
          }

          if strategies.is_empty() { None } else { Some(strategies) }
      }

      fn analyze_stm32_error(&self, error: &str) ->
  Option<Vec<BuildStrategy>> {
          let mut strategies = Vec::new();

          if error.contains("arm-none-eabi-gcc") {
              strategies.push(BuildStrategy::DependencyResolution(vec![
                  "gcc-arm-none-eabi".to_string()
              ]));
          }

          if error.contains("STM32Make.make") {
              strategies.push(BuildStrategy::ToolchainFallback("Makefile".to_string()));
          }

          if strategies.is_empty() { None } else { Some(strategies) }
      }

      fn analyze_scons_error(&self, error: &str) ->
  Option<Vec<BuildStrategy>> {
          let mut strategies = Vec::new();

          if error.contains("scons") && error.contains("not found") {
              strategies.push(BuildStrategy::DependencyResolution(vec!["scons".to_string()]));
          }

          if error.contains("python") {
              strategies.push(BuildStrategy::ToolchainFallback("python3".to_string()));
          }

          if strategies.is_empty() { None } else { Some(strategies) }
      }

      // Implementation methods for build strategies
      async fn record_success(&self, _path: &Path, _system: BuildSystem, _strategy: BuildStrategy) {
          // TODO: Implement success recording to database
      }

      async fn build_platformio_default(&self, path: &Path) -> Result<BuildResult> {
          // Delegate to original build_platformio function
          crate::execution::build_platformio_original(path).await
      }

      async fn build_cmake_default(&self, path: &Path) -> Result<BuildResult> {
          // Delegate to original build_cmake function
          crate::execution::build_cmake_original(path).await
      }

      async fn build_makefile_default(&self, path: &Path) -> Result<BuildResult> {
          // Delegate to original build_makefile function
          crate::execution::build_makefile_original(path).await
      }

      async fn build_zephyr_default(&self, path: &Path) -> Result<BuildResult> {
          // Delegate to original build_zephyr function
          crate::execution::build_zephyr_original(path).await
      }

      async fn build_stm32_default(&self, path: &Path) -> Result<BuildResult> {
          // Delegate to original build_stm32 function
          crate::execution::build_stm32_original(path).await
      }

      async fn build_scons_default(&self, path: &Path) -> Result<BuildResult> {
          // Delegate to original build_scons function
          crate::execution::build_scons_original(path).await
      }

      pub async fn patch_platformio_config(&self, path: &Path, patches: HashMap<String, String>) -> Result<()> {
          let config_path = path.join("platformio.ini");
          if config_path.exists() {
              let mut content = tokio::fs::read_to_string(&config_path).await?;
              for (key, value) in patches {
                  content = content.replace(&format!("{} =", key), &format!("{} = {}", key, value));
              }
              tokio::fs::write(&config_path, content).await?;
          }
          Ok(())
      }

      pub async fn patch_cmake_config(&self, path: &Path, _patches: HashMap<String, String>) -> Result<()> {
          let config_path = path.join("CMakeCache.txt");
          if config_path.exists() {
              tokio::fs::remove_file(config_path).await?; // Clean cache
          }
          // TODO: Apply CMAKE patches to CMakeLists.txt
          Ok(())
      }

      pub async fn build_in_container(&self, _path: &Path, _system: BuildSystem, _arch: String) -> Result<BuildResult> {
          // TODO: Implement container-based building
          Err(anyhow!("Container building not implemented"))
      }

      async fn build_cmake_with_toolchain(&self, path: &Path, toolchain: &str) -> Result<BuildResult> {
          let build_dir = path.join("build");
          tokio::fs::create_dir_all(&build_dir).await?;

          let configure = Command::new("cmake")
              .arg("-DCMAKE_C_COMPILER=".to_owned() + toolchain)
              .arg("..")
              .current_dir(&build_dir)
              .stdout(Stdio::piped())
              .stderr(Stdio::piped())
              .output()
              .await?;

          if !configure.status.success() {
              return Err(anyhow!("CMake configure with toolchain failed: {}", String::from_utf8_lossy(&configure.stderr)));
          }

          self.build_cmake_default(path).await
      }

      async fn install_cmake_dependencies(&self, _path: &Path, deps: Vec<String>) -> Result<()> {
          for dep in deps {
              let output = Command::new("apt-get")
                  .arg("install")
                  .arg("-y")
                  .arg(&dep)
                  .stdout(Stdio::piped())
                  .stderr(Stdio::piped())
                  .output()
                  .await?;

              if !output.status.success() {
                  return Err(anyhow!("Failed to install dependency {}: {}", dep, String::from_utf8_lossy(&output.stderr)));
              }
          }
          Ok(())
      }

      // Placeholder implementations for other build system strategies
      async fn build_makefile_with_strategy(&self, path: &Path, _strategy: BuildStrategy) -> Result<BuildResult> {
          self.build_makefile_default(path).await
      }

      async fn build_zephyr_with_strategy(&self, path: &Path, _strategy: BuildStrategy) -> Result<BuildResult> {
          self.build_zephyr_default(path).await
      }

      async fn build_stm32_with_strategy(&self, path: &Path, _strategy: BuildStrategy) -> Result<BuildResult> {
          self.build_stm32_default(path).await
      }

      async fn build_scons_with_strategy(&self, path: &Path, _strategy: BuildStrategy) -> Result<BuildResult> {
          self.build_scons_default(path).await
      }
  }

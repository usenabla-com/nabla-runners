packer {
  required_plugins {
    googlecompute = {
      source  = "github.com/hashicorp/googlecompute"
      version = "~> 1.1"
    }
  }
}

variable "project_id" {
  type        = string
  description = "GCP Project ID"
}

variable "zone" {
  type        = string
  description = "GCP Zone"
  default     = "us-central1-a"
}

variable "source_image_family" {
  type        = string
  description = "Source image family"
  default     = "debian-12"
}

variable "image_name" {
  type        = string
  description = "Name for the baked image"
  default     = "nabla-runner"
}

variable "image_family" {
  type        = string
  description = "Image family for the baked image"
  default     = "nabla-runner"
}

variable "github_repo" {
  type        = string
  description = "GitHub repository URL"
  default     = "https://github.com/jdbohrman/nabla-runners.git"
}

variable "github_branch" {
  type        = string
  description = "Git branch to build"
  default     = "main"
}

variable "machine_type" {
  type        = string
  description = "Machine type for building"
  default     = "e2-medium"
}

locals {
  timestamp = regex_replace(timestamp(), "[- TZ:]", "")
  image_name = "${var.image_name}-${local.timestamp}"
}

source "googlecompute" "nabla-runner" {
  project_id          = var.project_id
  source_image_family = var.source_image_family
  source_image_project = "debian-cloud"
  zone                = var.zone
  machine_type        = var.machine_type
  
  image_name        = local.image_name
  image_family      = var.image_family
  image_description = "Nabla Runner service image with pre-installed dependencies"
  
  ssh_username = "packer"
  
  disk_size = 20
  disk_type = "pd-standard"
  
  tags = ["packer-build"]
  
  # Labels for the image
  image_labels = {
    environment = "production"
    service     = "nabla-runner"
    built_by    = "packer"
    build_date  = local.timestamp
  }
}

build {
  sources = ["source.googlecompute.nabla-runner"]
  
  # Install system dependencies
  provisioner "shell" {
    inline = [
      "echo '=== Installing system dependencies ==='",
      "sudo apt-get update",
      "sudo apt-get upgrade -y",
      "sudo apt-get install -y --no-install-recommends \\",
      "  ca-certificates curl git unzip zip tar xz-utils \\",
      "  build-essential pkg-config libssl-dev \\",
      "  cmake ninja-build make gcc g++ \\",
      "  python3 python3-pip python3-venv \\",
      "  device-tree-compiler file",
      "sudo apt-get clean",
      "sudo rm -rf /var/lib/apt/lists/*"
    ]
  }
  
  # Install Rust
  provisioner "shell" {
    inline = [
      "echo '=== Installing Rust ==='",
      "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y",
      "source $HOME/.cargo/env",
      "$HOME/.cargo/bin/rustup component add rustfmt clippy",
      "echo 'export PATH=\"$HOME/.cargo/bin:$PATH\"' | sudo tee -a /etc/profile"
    ]
  }
  
  # Install Python tools
  provisioner "shell" {
    inline = [
      "echo '=== Installing Python tools ==='",
      "sudo pip3 install --break-system-packages platformio west scons"
    ]
  }
  
  # Install ARM toolchain
  provisioner "shell" {
    inline = [
      "echo '=== Installing ARM toolchain ==='",
      "curl -L https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v12.2.1-1.2/xpack-arm-none-eabi-gcc-12.2.1-1.2-linux-x64.tar.gz -o /tmp/arm-gcc.tar.gz",
      "sudo mkdir -p /opt/gcc-arm-none-eabi",
      "sudo tar -xzf /tmp/arm-gcc.tar.gz -C /opt/gcc-arm-none-eabi --strip-components=1",
      "rm /tmp/arm-gcc.tar.gz",
      "echo 'export PATH=\"/opt/gcc-arm-none-eabi/bin:$PATH\"' | sudo tee -a /etc/profile"
    ]
  }
  
  # Clone and build nabla-runner
  provisioner "shell" {
    environment_vars = [
      "GITHUB_REPO=${var.github_repo}",
      "GITHUB_BRANCH=${var.github_branch}"
    ]
    inline = [
      "echo '=== Building nabla-runner ==='",
      "cd /opt",
      "sudo git clone $GITHUB_REPO nabla-runners",
      "cd nabla-runners",
      "sudo git checkout $GITHUB_BRANCH",
      "source $HOME/.cargo/env",
      "sudo -E $HOME/.cargo/bin/cargo build --release",
      "sudo chmod +x target/release/nabla-runner"
    ]
  }
  
  # Create systemd service
  provisioner "file" {
    content = <<-EOF
[Unit]
Description=Nabla Runner Service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=nabla-runner
Group=nabla-runner
WorkingDirectory=/opt/nabla-runners
Environment="RUST_LOG=info"
Environment="PORT=8080"
ExecStart=/opt/nabla-runners/target/release/nabla-runner
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/workspace /tmp

[Install]
WantedBy=multi-user.target
EOF
    destination = "/tmp/nabla-runner.service"
  }
  
  # Set up service and workspace
  provisioner "shell" {
    inline = [
      "echo '=== Setting up service ==='",
      # Create service user
      "sudo useradd -r -s /bin/false -d /opt/nabla-runners nabla-runner || true",
      "sudo chown -R nabla-runner:nabla-runner /opt/nabla-runners",
      
      # Create workspace directory
      "sudo mkdir -p /workspace",
      "sudo chown nabla-runner:nabla-runner /workspace",
      "sudo chmod 755 /workspace",
      
      # Install and enable service
      "sudo mv /tmp/nabla-runner.service /etc/systemd/system/",
      "sudo systemctl daemon-reload",
      "sudo systemctl enable nabla-runner.service",
      
      # Create update script
      "sudo tee /usr/local/bin/update-nabla-runner > /dev/null << 'SCRIPT'",
      "#!/bin/bash",
      "set -e",
      "cd /opt/nabla-runners",
      "sudo systemctl stop nabla-runner",
      "sudo -u nabla-runner git pull",
      "sudo -u nabla-runner /home/packer/.cargo/bin/cargo build --release",
      "sudo systemctl start nabla-runner",
      "echo 'Nabla runner updated successfully'",
      "SCRIPT",
      "sudo chmod +x /usr/local/bin/update-nabla-runner"
    ]
  }
  
  # Add monitoring script
  provisioner "file" {
    content = <<-EOF
#!/bin/bash
# Health check script for nabla-runner

HEALTH_URL="http://localhost:8080/health"
MAX_RETRIES=3
RETRY_DELAY=2

for i in $(seq 1 $MAX_RETRIES); do
    response=$(curl -s -o /dev/null -w "%{http_code}" $HEALTH_URL 2>/dev/null)
    if [ "$response" = "200" ]; then
        echo "Health check passed"
        exit 0
    fi
    
    if [ $i -lt $MAX_RETRIES ]; then
        echo "Health check failed (attempt $i/$MAX_RETRIES), retrying in ${RETRY_DELAY}s..."
        sleep $RETRY_DELAY
    fi
done

echo "Health check failed after $MAX_RETRIES attempts"
exit 1
EOF
    destination = "/tmp/health-check.sh"
  }
  
  provisioner "shell" {
    inline = [
      "sudo mv /tmp/health-check.sh /usr/local/bin/nabla-health-check",
      "sudo chmod +x /usr/local/bin/nabla-health-check"
    ]
  }
  
  # Add startup script for first boot
  provisioner "file" {
    content = <<-EOF
#!/bin/bash
# First boot initialization script

# Wait for network
while ! ping -c 1 google.com &> /dev/null; do
    sleep 1
done

# Log startup
echo "Nabla Runner instance started at $(date)" | logger -t nabla-runner

# Start the service if not already running
if ! systemctl is-active --quiet nabla-runner; then
    systemctl start nabla-runner
fi

# Run health check
sleep 5
if /usr/local/bin/nabla-health-check; then
    echo "Nabla Runner service is healthy" | logger -t nabla-runner
else
    echo "Nabla Runner service health check failed" | logger -t nabla-runner
fi
EOF
    destination = "/tmp/first-boot.sh"
  }
  
  provisioner "shell" {
    inline = [
      "sudo mv /tmp/first-boot.sh /usr/local/bin/nabla-first-boot",
      "sudo chmod +x /usr/local/bin/nabla-first-boot",
      
      # Add to crontab for root
      "echo '@reboot /usr/local/bin/nabla-first-boot' | sudo crontab -"
    ]
  }
  
  # Clean up
  provisioner "shell" {
    inline = [
      "echo '=== Cleaning up ==='",
      "sudo apt-get autoremove -y",
      "sudo apt-get clean",
      "sudo rm -rf /tmp/*",
      "history -c",
      "echo 'Image build complete!'"
    ]
  }
  
  # Final validation
  provisioner "shell" {
    inline = [
      "echo '=== Validating installation ==='",
      "sudo systemctl status nabla-runner --no-pager || true",
      "ls -la /opt/nabla-runners/target/release/nabla-runner",
      "echo 'Image is ready for deployment'"
    ]
  }
}
#!/bin/bash
set -e

# Log all output
exec > >(tee -a /var/log/nabla-runner-startup.log)
exec 2>&1

echo "Starting Nabla Runner setup at $(date)"

# Update system
apt-get update
apt-get upgrade -y

# Install required system packages
apt-get install -y \
    curl \
    git \
    build-essential \
    pkg-config \
    libssl-dev \
    cmake \
    ninja-build \
    make \
    gcc \
    g++ \
    python3 \
    python3-pip \
    python3-venv \
    python3-dev \
    python3-setuptools \
    python3-wheel \
    device-tree-compiler \
    file \
    unzip \
    zip \
    nodejs \
    npm \
    ca-certificates \
    wget

# Install Rust
if ! command -v cargo &> /dev/null; then
    echo "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "Rust already installed"
fi

# Add cargo to PATH for all users
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> /etc/profile
export PATH="$HOME/.cargo/bin:$PATH"

# Install Python tools
# Handle pip upgrade carefully to avoid Debian package conflicts
python3 -m pip install --break-system-packages --upgrade pip setuptools wheel || {
    echo "Warning: pip upgrade failed, continuing with system pip"
}
pip3 install --break-system-packages platformio west scons

# Update PlatformIO and install common platforms
pio upgrade
pio platform install espressif32
pio platform install atmelavr

# Install ARM toolchain for STM32 and similar
if [ ! -d "/opt/gcc-arm-none-eabi" ]; then
    echo "Installing ARM toolchain..."
    curl -L https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v12.2.1-1.2/xpack-arm-none-eabi-gcc-12.2.1-1.2-linux-x64.tar.gz -o /tmp/arm-gcc.tar.gz
    mkdir -p /opt/gcc-arm-none-eabi
    tar -xzf /tmp/arm-gcc.tar.gz -C /opt/gcc-arm-none-eabi --strip-components=1
    rm /tmp/arm-gcc.tar.gz
    echo 'export PATH="/opt/gcc-arm-none-eabi/bin:$PATH"' >> /etc/profile
fi

# Clone the repository
cd /opt
if [ -d "nabla-runners" ]; then
    echo "Repository already exists, pulling latest..."
    cd nabla-runners
    git fetch origin
    git checkout main
    git pull origin main
else
    echo "Cloning repository..."
    git clone "https://github.com/usenabla-com/nabla-runners"
    cd nabla-runners
    git checkout main
fi

# Build the project
echo "Building nabla-runner..."
/root/.cargo/bin/cargo build --release

# Create systemd service
cat > /etc/systemd/system/nabla-runner.service <<EOF
[Unit]
Description=Nabla Runner Service
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/nabla-runners
Environment="RUST_LOG=info"
Environment="PORT=8080"
Environment="CUSTOMER_ID=\${CUSTOMER_ID:-default}"
Environment="ALLOWED_INSTALLATION_IDS=\${ALLOWED_INSTALLATION_IDS:-}"
ExecStart=/opt/nabla-runners/target/release/nabla-runner

[Install]
WantedBy=multi-user.target
EOF

# Create workspace directory
mkdir -p /workspace
chmod 755 /workspace

# Enable and start the service
systemctl daemon-reload
systemctl enable nabla-runner
systemctl restart nabla-runner

# Check service status
sleep 5
if systemctl is-active --quiet nabla-runner; then
    echo "Nabla Runner service started successfully"
    systemctl status nabla-runner --no-pager
else
    echo "Failed to start Nabla Runner service"
    systemctl status nabla-runner --no-pager
    journalctl -u nabla-runner --no-pager -n 50
fi

echo "Nabla Runner setup completed at $(date)"
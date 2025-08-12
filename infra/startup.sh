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
    libffi-dev \
    cmake \
    ninja-build \
    make \
    gcc \
    g++ \
    python3 \
    python3-venv \
    python3-dev \
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

# Install UV - the fast Python package manager
if ! command -v uv &> /dev/null; then
    echo "Installing UV..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    source "$HOME/.local/bin/env"
else
    echo "UV already installed"
fi

# Add UV to PATH for all users
echo 'export PATH="$HOME/.local/bin:$PATH"' >> /etc/profile
export PATH="$HOME/.local/bin:$PATH"

# Clean up any previous UV tool installations of platformio
echo "Cleaning up previous installations..."
uv tool uninstall platformio 2>/dev/null || true
uv tool uninstall west 2>/dev/null || true
uv tool uninstall scons 2>/dev/null || true
rm -rf /root/.local/share/uv/tools/platformio 2>/dev/null || true
rm -rf /root/.local/share/uv/tools/west 2>/dev/null || true
rm -rf /root/.local/share/uv/tools/scons 2>/dev/null || true

# Create a virtual environment for PlatformIO with UV
echo "Setting up PlatformIO environment with UV..."
# Remove old environment if it exists
rm -rf /opt/platformio-env 2>/dev/null || true

# PlatformIO needs pip in its environment, so we'll create a dedicated venv
uv venv /opt/platformio-env --python python3
source /opt/platformio-env/bin/activate

# Install pip and essential packages in the venv (PlatformIO requires it)
uv pip install --python /opt/platformio-env/bin/python pip setuptools wheel

# Install PlatformIO and other tools in the venv
uv pip install --python /opt/platformio-env/bin/python platformio west scons

# Create symlinks for global access (remove old ones first)
rm -f /usr/local/bin/pio /usr/local/bin/platformio /usr/local/bin/west /usr/local/bin/scons
ln -sf /opt/platformio-env/bin/pio /usr/local/bin/pio
ln -sf /opt/platformio-env/bin/platformio /usr/local/bin/platformio
ln -sf /opt/platformio-env/bin/west /usr/local/bin/west
ln -sf /opt/platformio-env/bin/scons /usr/local/bin/scons

# Ensure PlatformIO uses the correct Python with pip
export PLATFORMIO_CORE_DIR=/opt/platformio-core
export PLATFORMIO_PYTHON_EXE=/opt/platformio-env/bin/python

# Update PlatformIO and install common platforms
echo "Configuring PlatformIO platforms..."
/opt/platformio-env/bin/python -m pip install --upgrade pip
/opt/platformio-env/bin/pio pkg update --global || true
/opt/platformio-env/bin/pio pkg install --global --platform espressif32
/opt/platformio-env/bin/pio pkg install --global --platform atmelavr

# Deactivate the virtual environment
deactivate

# Add PlatformIO environment variables to profile
echo 'export PLATFORMIO_CORE_DIR=/opt/platformio-core' >> /etc/profile
echo 'export PLATFORMIO_PYTHON_EXE=/opt/platformio-env/bin/python' >> /etc/profile

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
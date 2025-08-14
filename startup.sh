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
    # Source the UV environment
    export PATH="$HOME/.local/bin:$PATH"
else
    echo "UV already installed"
    export PATH="$HOME/.local/bin:$PATH"
fi

# Add UV to PATH for all users (only if not already added)
if ! grep -q "/.local/bin" /etc/profile; then
    echo 'export PATH="$HOME/.local/bin:$PATH"' >> /etc/profile
fi

# Create a dedicated Python environment for embedded development tools
echo "Setting up Python environment for embedded tools..."

# Remove any existing environment to ensure clean installation
rm -rf /opt/embedded-tools-env 2>/dev/null || true

# Create virtual environment using UV (much faster than python -m venv)
uv venv /opt/embedded-tools-env --python python3

# Install pip and core packages in the environment
echo "Installing core Python packages..."
/opt/embedded-tools-env/bin/python -m ensurepip --upgrade 2>/dev/null || \
    uv pip install --python /opt/embedded-tools-env/bin/python pip

# Upgrade pip and install setuptools/wheel
uv pip install --python /opt/embedded-tools-env/bin/python --upgrade pip setuptools wheel

# Install embedded development tools
echo "Installing PlatformIO, West, and SCons..."
uv pip install --python /opt/embedded-tools-env/bin/python platformio west scons

# Create global command shortcuts
echo "Setting up global commands..."
cat > /usr/local/bin/pio << 'EOF'
#!/bin/bash
export PLATFORMIO_PYTHON_EXE=/opt/embedded-tools-env/bin/python
export PLATFORMIO_CORE_DIR=/opt/platformio-core
exec /opt/embedded-tools-env/bin/pio "$@"
EOF

cat > /usr/local/bin/platformio << 'EOF'
#!/bin/bash
export PLATFORMIO_PYTHON_EXE=/opt/embedded-tools-env/bin/python
export PLATFORMIO_CORE_DIR=/opt/platformio-core
exec /opt/embedded-tools-env/bin/platformio "$@"
EOF

cat > /usr/local/bin/west << 'EOF'
#!/bin/bash
exec /opt/embedded-tools-env/bin/west "$@"
EOF

cat > /usr/local/bin/scons << 'EOF'
#!/bin/bash
exec /opt/embedded-tools-env/bin/scons "$@"
EOF

# Make wrapper scripts executable
chmod +x /usr/local/bin/pio /usr/local/bin/platformio /usr/local/bin/west /usr/local/bin/scons

# Set up PlatformIO
echo "Configuring PlatformIO..."
export PLATFORMIO_PYTHON_EXE=/opt/embedded-tools-env/bin/python
export PLATFORMIO_CORE_DIR=/opt/platformio-core

# Update PlatformIO core
/usr/local/bin/pio pkg update --global || true

# Install common platforms
echo "Installing PlatformIO platforms..."
/usr/local/bin/pio pkg install --global --platform espressif32
/usr/local/bin/pio pkg install --global --platform atmelavr

# Add PlatformIO environment variables to profile (if not already there)
if ! grep -q "PLATFORMIO_CORE_DIR" /etc/profile; then
    echo 'export PLATFORMIO_CORE_DIR=/opt/platformio-core' >> /etc/profile
fi
if ! grep -q "PLATFORMIO_PYTHON_EXE" /etc/profile; then
    echo 'export PLATFORMIO_PYTHON_EXE=/opt/embedded-tools-env/bin/python' >> /etc/profile
fi

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
echo "PlatformIO Python: $PLATFORMIO_PYTHON_EXE"
echo "PlatformIO Core Dir: $PLATFORMIO_CORE_DIR"
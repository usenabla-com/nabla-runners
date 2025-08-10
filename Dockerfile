# GCP Batch Builder Image for Nabla Enterprise
# Includes toolchains for: Cargo (Rust), Make, CMake, PlatformIO, Zephyr West, STM32 (gcc-arm-none-eabi), SCons

FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

# Base utilities and compilers
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    unzip \
    zip \
    tar \
    xz-utils \
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
    device-tree-compiler \
    file \
  && rm -rf /var/lib/apt/lists/*

# Install Rust (for Cargo projects)
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable \
  && /root/.cargo/bin/rustup component add rustfmt clippy
ENV PATH="/root/.cargo/bin:${PATH}"

# Python tooling: PlatformIO, West, SCons
RUN pip3 install --no-cache-dir --break-system-packages platformio west scons

# ARM Embedded GCC toolchain for STM32 and similar
RUN curl -L https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v12.2.1-1.2/xpack-arm-none-eabi-gcc-12.2.1-1.2-linux-x64.tar.gz -o /tmp/arm-gcc.tar.gz \
  && mkdir -p /opt/gcc-arm-none-eabi \
  && tar -xzf /tmp/arm-gcc.tar.gz -C /opt/gcc-arm-none-eabi --strip-components=1 \
  && rm /tmp/arm-gcc.tar.gz
ENV PATH="/opt/gcc-arm-none-eabi/bin:${PATH}"

# Build the Rust runner binary
WORKDIR /tmp/nabla-runner

# Copy only what's needed for the runner build
COPY Cargo.toml ./
# Copy Cargo.lock if it exists (for reproducible builds)
COPY Cargo.loc[k] ./

# Create dummy main file to cache dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    touch src/lib.rs && \
    cargo build --release --bin nabla-runner && \
    rm -rf src

# Now copy actual source code
COPY src ./src

# Build the actual binary
RUN cargo build --release --bin nabla-runner

# Install the binary
RUN cp target/release/nabla-runner /usr/local/bin/nabla-runner && \
    chmod +x /usr/local/bin/nabla-runner

# Cleanup build artifacts
RUN rm -rf /tmp/nabla-runner

# Workspace
RUN useradd -ms /bin/bash builder
WORKDIR /workspace
RUN chown -R builder:builder /workspace

# Cloud Run HTTP server
EXPOSE 8080
CMD ["/usr/local/bin/nabla-runner"]
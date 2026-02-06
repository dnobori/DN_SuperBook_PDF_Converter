# superbook-pdf Backend - Rust API Server with GPU Support
# Multi-stage build for minimal image size

# ===== Stage 1: Build Rust binary =====
FROM docker.io/rust:1.85-bookworm AS builder

# Install dependencies for building
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy Cargo files first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy src to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    mkdir -p src/web && touch src/lib.rs
RUN cargo build --release --features web 2>/dev/null || true
RUN rm -rf src

# Copy actual source
COPY src ./src
COPY benches ./benches

# Build release binary
RUN touch src/main.rs && cargo build --release --features web

# ===== Stage 2: Runtime image with CUDA =====
FROM docker.io/nvidia/cuda:11.8.0-runtime-ubuntu22.04

LABEL maintainer="superbook-pdf"
LABEL description="superbook-pdf Backend API with AI tools"
LABEL component="backend"

ENV DEBIAN_FRONTEND=noninteractive
ENV TZ=Asia/Tokyo

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    # PDF tools
    poppler-utils \
    ghostscript \
    qpdf \
    # OCR
    tesseract-ocr \
    tesseract-ocr-jpn \
    tesseract-ocr-eng \
    # Python for AI tools
    python3 \
    python3-venv \
    # Libraries
    libssl3 \
    libgl1-mesa-glx \
    libglib2.0-0 \
    # Fonts
    fonts-noto-cjk \
    fonts-noto-cjk-extra \
    # Tools
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install uv (fast Python package manager)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:$PATH"

# Create app directory
WORKDIR /app

# Copy Rust binary from builder
COPY --from=builder /build/target/release/superbook-pdf /usr/local/bin/

# Setup Python virtual environment for AI tools
ENV AI_VENV=/app/ai_venv
RUN uv venv $AI_VENV --python python3.10 \
    && VIRTUAL_ENV=$AI_VENV uv pip install realesrgan yomitoku \
    && VIRTUAL_ENV=$AI_VENV uv pip install --force-reinstall \
        torch==2.0.1+cu118 torchvision==0.15.2+cu118 \
        --index-url https://download.pytorch.org/whl/cu118 \
    && VIRTUAL_ENV=$AI_VENV uv pip install "numpy<2"

# Create data directories
RUN mkdir -p /data/input /data/output /data/work

# Environment
ENV NVIDIA_VISIBLE_DEVICES=all
ENV NVIDIA_DRIVER_CAPABILITIES=compute,utility
ENV SUPERBOOK_VENV=$AI_VENV

# API server configuration
ENV SUPERBOOK_BIND=0.0.0.0
ENV SUPERBOOK_PORT=8080
ENV SUPERBOOK_WORK_DIR=/data/work

WORKDIR /data

# Expose API port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/api/stats || exit 1

# Run API server
ENTRYPOINT ["superbook-pdf"]
CMD ["serve", "--bind", "0.0.0.0", "--port", "8080"]

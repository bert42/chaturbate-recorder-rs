# Build stage
FROM registry.access.redhat.com/ubi9/ubi-minimal AS builder

RUN microdnf install -y gcc make openssl-devel perl && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal && \
    microdnf clean all

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock* ./

# Create dummy src to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src target/release/chaturbate-recorder*

# Copy actual source and build
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM registry.access.redhat.com/ubi9/ubi-micro

COPY --from=builder /app/target/release/chaturbate-recorder /usr/local/bin/

WORKDIR /recordings
VOLUME ["/recordings", "/config"]

ENTRYPOINT ["chaturbate-recorder"]
CMD ["--help"]

FROM rust:slim-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    python3 \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /usr/local/bin

COPY --from=builder /app/target/release/nimbox /usr/local/bin/nimbox

RUN chmod +x /usr/local/bin/nimbox

COPY test/requirements.txt /tmp/test-requirements.txt
RUN pip3 install --no-cache-dir --break-system-packages -r /tmp/test-requirements.txt

WORKDIR /workspace

CMD ["/bin/bash"]

FROM rust:1-bookworm AS builder

WORKDIR /app

RUN apt-get update && \
    apt-get install -y --no-install-recommends clang libclang-dev protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY liquid-agent ./liquid-agent
COPY liquid-api ./liquid-api
COPY liquid-cli ./liquid-cli
COPY liquid-config ./liquid-config
COPY liquid-core ./liquid-core
COPY liquid-llm ./liquid-llm
COPY liquid-sql ./liquid-sql
COPY liquid-storage ./liquid-storage

RUN cargo build --release -p liquid-cli && \
    cp /app/target/release/liquid /usr/local/bin/liquid

FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates postgresql-client && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --create-home --uid 10001 --shell /usr/sbin/nologin liquid

WORKDIR /app

ENV HOME=/home/liquid
ENV LIQUID_API_ADDR=0.0.0.0:3001

COPY --from=builder /usr/local/bin/liquid /usr/local/bin/liquid

USER liquid

EXPOSE 3001

ENTRYPOINT ["liquid"]
CMD ["server"]

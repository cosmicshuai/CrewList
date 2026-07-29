# syntax=docker/dockerfile:1

FROM rust:1-slim-bookworm AS builder
WORKDIR /build

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config \
 && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin crewlist-server \
 && cp target/release/crewlist-server /usr/local/bin/crewlist-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 crewlist

COPY --from=builder /usr/local/bin/crewlist-server /usr/local/bin/crewlist-server

USER crewlist
# Inside the container this must be 0.0.0.0 to be reachable at all; the
# loopback restriction is enforced by the host-side port publication in
# docker-compose.yml, not here. SPEC.md §2.1.
ENV CREWLIST_BIND=0.0.0.0:8787
EXPOSE 8787

ENTRYPOINT ["/usr/local/bin/crewlist-server"]

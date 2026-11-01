# Nage container image (SPEC 5.2; decisions 68 and 69). Stage 1 compiles a static
# x86_64-unknown-linux-musl binary (SQLite bundled, time-zone database embedded) in the official
# Rust image for Alpine, whose host target is already musl, so a plain cargo build is static.
# Stage 2 copies it into a distroless static runtime that runs as uid 1000 with /data pre-created
# and owned by that uid, declares an exec-form HEALTHCHECK on `nage health` and sets
# NAGE_DATA_DIR=/data and NAGE_BIND=0.0.0.0:8787.
#
# Both base images are pinned by digest and re-resolved at each milestone PR (decision 64's rule
# for actions, extended to images by decision 68): `docker buildx imagetools inspect <image:tag>`
# prints the current index digest. The rust tag must equal the channel in rust-toolchain.toml;
# scripts/docker-smoke.sh (run by the docker CI job) fails when it does not.
#
# Build and run (README "Docker"): docker build -t nage:dev .   then   docker compose up -d
# Change the address through NAGE_BIND only: the HEALTHCHECK's `nage health` reads NAGE_BIND, so
# a `serve --bind` override would leave it probing the wrong port (decision 69).

# rust:1.99.1-alpine3.24, index digest resolved 2026-10-30 (Docker Hub; on a pull-limit failure
# in CI the same digest is available as mirror.gcr.io/library/rust:1.99.1-alpine3.24, decision 68).
FROM rust:1.99.1-alpine3.24@sha256:b1d5dd5a22a09404d3e798c0cf051b689077360374eb505fa5bb93c479693d87 AS builder
WORKDIR /app
# sqlx's query macros read the committed .sqlx data and need no database (SPEC decision 57); the
# committed .env says the same, and the variable wins over any .env.
ENV SQLX_OFFLINE=true
# The context is the allow-list in .dockerignore: manifests, lock, .cargo/, .env, .sqlx/, crates/,
# migrations/ (decision 68).
COPY . .
# A production build never carries the test-support feature: -p nage, no --all-targets or
# --all-features (SPEC decision 59). The cache mounts keep the cargo registry and the build
# directory between builds on the same machine (they are shared by every build on it and never
# part of the image), so the binary is copied out in the same step. /rootfs/data is the runtime
# stage's /data, created here because that stage has no shell.
RUN --mount=type=cache,id=nage-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=nage-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=nage-target,target=/app/target \
    cargo build --release --locked -p nage \
    && cp target/release/nage /nage \
    && mkdir -p /rootfs/data \
    && chmod 0700 /rootfs/data

# gcr.io/distroless/static-debian13:latest, index digest resolved 2026-10-30. The image carries CA
# certificates, Debian's tzdata (which jiff uses in place of the embedded copy; decision 69) and
# /tmp, and no shell.
FROM gcr.io/distroless/static-debian13:latest@sha256:15f8637509792e05b97d83451b0b6ccfa4abd548f997e406ac1e72c87fe1fa78
LABEL org.opencontainers.image.source="https://github.com/bngoller/nage" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later" \
      org.opencontainers.image.description="Nage: household accounting in one binary with one SQLite file"
COPY --from=builder /nage /usr/local/bin/nage
# /data pre-created and owned by uid 1000 (SPEC 5.2): a fresh named volume mounted there inherits
# the owner and the mode 0700; a bind mount keeps the host directory's own. The directory is
# copied as a child of /rootfs because COPY creates a missing destination itself with mode 0755
# (BuildKit's fsutil), while a copied subdirectory keeps the source's mode.
COPY --from=builder --chown=1000:1000 /rootfs/ /
ENV NAGE_DATA_DIR=/data \
    NAGE_BIND=0.0.0.0:8787
EXPOSE 8787
USER 1000:1000
# The timeout exceeds nage health's own 2 s deadline (decision 44); failures in the start period
# never count and the first success ends it, so a start-up shorter than 30 s (migrations, from
# milestone 2 the pre-migration backup) is never marked unhealthy; a 2xx probe logs at debug
# (decision 47).
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --start-interval=1s --retries=3 \
    CMD ["/usr/local/bin/nage", "health"]
ENTRYPOINT ["/usr/local/bin/nage"]
CMD ["serve"]

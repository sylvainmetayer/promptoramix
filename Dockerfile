# Build et exécution sur la même Debian (bookworm) : même glibc des deux côtés.
# Images épinglées par digest (Renovate les met à jour). Le compilateur est celui de
# rust-toolchain.toml, comme en CI : rustup y ajoute clippy, rustfmt et llvm-tools au
# build, et télécharge la version demandée si l'image en porte une autre.
FROM rust:1.100.0-slim-bookworm@sha256:7c7d4eb4d05b7781187a327809466cfddf29753f6defdc3d19a0e291951b7a37 AS build
WORKDIR /app
# flip (Hetzner cx23) et tout x86-64 récent : popcnt matériel pour count_ones.
ENV RUSTFLAGS="-C target-cpu=x86-64-v2"
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --locked --bin linkx-server

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
COPY --from=build /app/target/release/linkx-server /usr/local/bin/linkx-server
ENV PORT=8080
EXPOSE 8080
USER nobody
CMD ["linkx-server"]

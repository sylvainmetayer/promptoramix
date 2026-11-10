# Build et exécution sur la même Debian (bookworm) : même glibc des deux côtés.
# Images épinglées par digest (Renovate les met à jour). Le compilateur est celui de
# rust-toolchain.toml, comme en CI : rustup y ajoute clippy, rustfmt et llvm-tools au
# build, et télécharge la version demandée si l'image en porte une autre.
FROM rust:1.99.0-slim-bookworm@sha256:2c3a22f0a5533ea2dd5a16627bc841228151faa2d4de2644ac9987e4a2f1f2fa AS build
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

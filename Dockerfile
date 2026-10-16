# Build et exécution sur la même Debian (bookworm) : même glibc des deux côtés.
FROM rust:1.99-slim-bookworm AS build
WORKDIR /app
# flip (Hetzner cx23) et tout x86-64 récent : popcnt matériel pour count_ones.
ENV RUSTFLAGS="-C target-cpu=x86-64-v2"
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --locked --bin linkx-server

FROM debian:bookworm-slim
COPY --from=build /app/target/release/linkx-server /usr/local/bin/linkx-server
ENV PORT=8080
EXPOSE 8080
USER nobody
CMD ["linkx-server"]

FROM rust:1.92-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --bin linkx-server

FROM debian:bookworm-slim
COPY --from=build /app/target/release/linkx-server /usr/local/bin/linkx-server
ENV PORT=8080
EXPOSE 8080
USER nobody
CMD ["linkx-server"]

FROM rust:1.72.0 as build-env

WORKDIR /app
RUN cargo new --bin bitews
COPY ./Cargo.toml ./Cargo.lock ./bitews/

WORKDIR /app/bitews
RUN cargo build --release
RUN rm ./src/*.rs
RUN rm ./target/release/deps/bitews*

COPY ./src ./src
COPY ./web ./web
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12
COPY --from=build-env /app/bitews/target/release/bitews /
CMD ["./bitews"]

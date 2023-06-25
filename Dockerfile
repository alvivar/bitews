FROM rust:1.69.0 as build-env

WORKDIR /app
RUN USER=root cargo new --bin bitews
COPY ./Cargo.toml ./Cargo.lock ./bitews/
WORKDIR /app/bitews
RUN cargo build --release
RUN rm src/*.rs

ADD . .
RUN rm ./target/release/deps/bitews*
RUN cargo build --release

FROM gcr.io/distroless/cc
COPY --from=build-env /app/bitews/target/release/bitews /
CMD ["./bitews"]

FROM rust:1.61.0 as build-env
WORKDIR /app
ADD . /app
RUN cargo build --release

FROM gcr.io/distroless/cc
COPY --from=build-env /app/target/release/bitews /
CMD ["./bitews"]

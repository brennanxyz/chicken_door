FROM rust:buster AS builder

# Install dependencies
RUN mkdir /app
WORKDIR /app
COPY ./Cargo.toml ./Cargo.toml
COPY ./src ./src
RUN cargo build --release

# Build the final image
FROM debian:buster-slim
RUN apt-get update && apt-get install -y libssl-dev ca-certificates
RUN mkdir /app
WORKDIR /app
COPY --from=builder /app/target/release/chicken_door /app/
COPY ./.config.toml /app/.config.toml
COPY ./files/* /app/files/
CMD ["./chicken_door"]
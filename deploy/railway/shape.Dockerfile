FROM rust:1.93-alpine AS builder

RUN apk add --no-cache musl-dev git

WORKDIR /app
COPY . .

RUN cargo build --release -p host-shape

FROM alpine:3.19

RUN apk add --no-cache ca-certificates

COPY --from=builder /app/target/release/host-shape /usr/local/bin/app

EXPOSE 8080

CMD ["app"]

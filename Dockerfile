FROM rust:1.93-alpine AS builder

ARG BIN_PACKAGE

RUN apk add --no-cache musl-dev git

WORKDIR /app
COPY . .

RUN cargo build --release -p ${BIN_PACKAGE}

FROM alpine:3.19

ARG BIN_PACKAGE

RUN apk add --no-cache ca-certificates

COPY --from=builder /app/target/release/${BIN_PACKAGE} /usr/local/bin/app

ENV CMDRVL_HOST_PORT=8080
EXPOSE 8080

CMD ["app"]

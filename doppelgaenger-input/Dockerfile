FROM ghcr.io/drogue-iot/builder:0.1.19 as builder

RUN mkdir /build

COPY . /build

WORKDIR /build

RUN cargo build --release

FROM registry.access.redhat.com/ubi8/ubi-minimal:latest

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-doppelgaenger"

RUN mkdir /app

COPY --from=builder /build/target/release/drogue-doppelgaenger-input /app

CMD [ "/app/drogue-doppelgaenger-input" ]
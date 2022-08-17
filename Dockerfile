ARG BUILDER_IMAGE=ghcr.io/drogue-iot/builder:latest
FROM $BUILDER_IMAGE AS builder

RUN mkdir -p /usr/src/cargo
ADD . /usr/src/cargo/

WORKDIR /usr/src/cargo/

RUN \
    --mount=type=cache,target=/usr/src/.cargo-container-home,z --mount=type=cache,target=/usr/src/cargo/target,z \
    true \
    && mkdir -p /output \
    && cargo build --release \
    && cp target/release/drogue-doppelgaenger-* /output/ \
    && ls /output \
    && true

RUN ls /output

FROM registry.access.redhat.com/ubi9-minimal AS base

RUN microdnf install -y libpq

FROM base AS backend

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-doppelgaenger"

COPY --from=builder /output/drogue-doppelgaenger-backend /
ENTRYPOINT [ "/drogue-doppelgaenger-backend" ]

FROM base AS processor

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-doppelgaenger"

COPY --from=builder /output/drogue-doppelgaenger-processor /
ENTRYPOINT [ "/drogue-doppelgaenger-processor" ]

FROM base AS injector

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-doppelgaenger"

COPY --from=builder /output/drogue-doppelgaenger-injector /
ENTRYPOINT [ "/drogue-doppelgaenger-injector" ]

FROM base AS server

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-doppelgaenger"

COPY --from=builder /output/drogue-doppelgaenger-server /
ENTRYPOINT [ "/drogue-doppelgaenger-server" ]


FROM ghcr.io/drogue-iot/diesel-base:0.2.0 as database-migration

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-doppelgaenger"

RUN mkdir /migrations
COPY database-migration/migrations /migrations

ENTRYPOINT ["/usr/local/bin/diesel"]

ENV RUST_LOG "diesel=debug"

CMD ["migration", "run"]

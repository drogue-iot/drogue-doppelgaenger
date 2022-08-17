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

COPY --from=builder /output/drogue-doppelgaenger-backend /
ENTRYPOINT [ "/drogue-doppelgaenger-backend" ]

FROM base AS processor

COPY --from=builder /output/drogue-doppelgaenger-processor /
ENTRYPOINT [ "/drogue-doppelgaenger-processor" ]

FROM base AS injector

COPY --from=builder /output/drogue-doppelgaenger-injector /
ENTRYPOINT [ "/drogue-doppelgaenger-injector" ]

FROM base AS server

COPY --from=builder /output/drogue-doppelgaenger-server /
ENTRYPOINT [ "/drogue-doppelgaenger-server" ]

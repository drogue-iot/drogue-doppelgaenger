run-deps:
    podman-compose -f develop/compose.yaml up

start-deps:
    podman-compose -f develop/compose.yaml up -d

stop-deps:
    podman-compose -f develop/compose.yaml down

restart-deps: stop-deps start-deps

run-deps:
    podman-compose -f develop/compose.yaml -f develop/compose-health.yaml up

start-deps:
    podman-compose -f develop/compose.yaml -f develop/compose-health.yaml up -d

stop-deps:
    podman-compose -f develop/compose.yaml -f develop/compose-health.yaml down

restart-deps: stop-deps start-deps

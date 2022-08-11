run-deps:
    podman-compose -f develop/docker-compose.yml up

start-deps:
    podman-compose -f develop/docker-compose.yml up -d

stop-deps:
    podman-compose -f develop/docker-compose.yml down

restart-deps: stop-deps start-deps

start-deps:
    podman play kube develop/services.yaml

stop-deps:
    -podman play kube develop/services.yaml --down

restart-deps: stop-deps start-deps

start-deps:
    podman play kube develop/dependencies.yaml

stop-deps:
    podman play kube develop/dependencies.yaml --down

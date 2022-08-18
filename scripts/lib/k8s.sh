function wait_for_resource() {
    local resource="$1"
    shift

    echo "Waiting until $resource exists..."

    while ! kubectl get "$resource" -n "$DROGUE_NS" >/dev/null 2>&1; do
        sleep 5
    done
}

function get_env() {
    local resource="$1"
    shift
    local container="$1"
    shift
    local name="$1"
    shift

    kubectl -n "$DROGUE_NS" get $resource -o jsonpath="{.spec.template.spec.containers[?(@.name==\"$container\")].env[?(@.name==\"$name\")].value}"
}

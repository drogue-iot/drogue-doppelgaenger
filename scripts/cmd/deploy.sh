#!/usr/bin/env bash

set +e

# defaults

# process arguments

help() {
    cat <<EOF
Usage: drgadm deploy
Drogue IoT cloud admin tool - deploy

Options:

  -c <cluster>       The cluster type (default: $CLUSTER)
                       one of: minikube, kind, kubernetes, openshift
  -d <domain>        Set the base DNS domain. Can be auto-detected for Minikube, Kind, and OpenShift.
  -n <namespace>     The namespace to install to (default: $DROGUE_NS)
  -s <key>=<value>   Set a Helm option, can be repeated:
                       -s foo=bar -s bar=baz -s foo.bar=baz
  -S <key>=<value>   Set a Helm option (as string), can be repeated:
                       -S foo=bar -S bar=baz -S foo.bar=baz
  -k                 Don't install dependencies
  -f <vales.yaml>    Add a Helm values files
  -p <profile>       Enable Helm profile (adds 'deploy/profiles/<profile>.yaml')
  -t <timeout>       Helm installation timeout (default: 15m)
  -h                 Show this help

EOF
}

# shellcheck disable=SC2181
[ $? -eq 0 ] || {
    help >&3
    # we don't "fail" but exit here, since we don't want any more output
    exit 1
}

while getopts mhkeMp:c:n:d:s:S:t:Tf: FLAG; do
  case $FLAG in
    c)
        CLUSTER="$OPTARG"
        ;;
    k)
        INSTALL_DEPS=false
        ;;
    n)
        DROGUE_NS="$OPTARG"
        ;;
    s)
        HELM_ARGS="$HELM_ARGS --set $OPTARG"
        ;;
    S)
        HELM_ARGS="$HELM_ARGS --set-string $OPTARG"
        ;;
    f)
        HELM_ARGS="$HELM_ARGS --values $OPTARG"
        ;;
    d)
        DOMAIN="$OPTARG"
        ;;
    p)
        HELM_PROFILE="$OPTARG"
        ;;
    t)
        HELM_TIMEOUT="$OPTARG"
        ;;
    h)
        help >&3
        exit 0
        ;;
    \?)
        help >&3
        exit 0
        ;;
    *)
        help >&3
        # we don't "fail" but exit here, since we don't want any more output
        exit 1
        ;;
    esac
done

set -e

#
# deploy defaults
#

: "${INSTALL_DEPS:=true}"
: "${INSTALL_STRIMZI:=${INSTALL_DEPS}}"
: "${INSTALL_KEYCLOAK_OPERATOR:=${INSTALL_DEPS}}"
: "${HELM_TIMEOUT:=15m}"

case $CLUSTER in
    kind)
        : "${INSTALL_NGINX_INGRESS:=${INSTALL_DEPS}}"
        # test for the ingress controller node flag
        if [[ -z "$(kubectl get node kind-control-plane -o jsonpath="{.metadata.labels['ingress-ready']}")" ]]; then
            die "Kind node 'kind-control-plane' is missing 'ingress-ready' annotation. Please ensure that you properly set up Kind for ingress: https://kind.sigs.k8s.io/docs/user/ingress#create-cluster"
        fi
        ;;
    *)
        ;;
esac

# Check for our standard tools

check_std_tools

# Check if we can connect to the cluster

check_cluster_connection

# Create the namespace first

if ! kubectl get ns "$DROGUE_NS" >/dev/null 2>&1; then
    progress -n "🆕 Creating namespace ($DROGUE_NS) ... "
    kubectl create namespace "$DROGUE_NS"
    progress "done!"
fi

# domain

domain=$(detect_domain)

# install pre-reqs

if [[ "$INSTALL_NGINX_INGRESS" == true ]]; then
    source "$BASEDIR/cmd/__nginx.sh"
fi
if [[ "$INSTALL_STRIMZI" == true ]]; then
    source "$BASEDIR/cmd/__strimzi.sh"
fi
if [[ "$INSTALL_KEYCLOAK_OPERATOR" == true ]]; then
    source "$BASEDIR/cmd/__sso.sh"
fi

# add Helm value files
#
# As these are applies in the order of the command line, the more default ones must come first. As we might already
# have value files from the arguments, we prepend our default value files to the arguments, more specific ones first,
# so we end up with an argument list of more specific ones last. Values provide with the --set argument will always
# override value files properties, so their relation to the values files doesn't matter.

if [[ -f $BASEDIR/local-values.yaml ]]; then
    progress "💡 Adding local values file ($BASEDIR/local-values.yaml)"
    HELM_ARGS="--values $BASEDIR/local-values.yaml $HELM_ARGS"
fi
if [[ "$HELM_PROFILE" ]]; then
    progress "💡 Adding profile values file ($BASEDIR/../deploy/profiles/${HELM_PROFILE}.yaml)"
    HELM_ARGS="--values $BASEDIR/../deploy/profiles/${HELM_PROFILE}.yaml $HELM_ARGS"
fi
if [[ -f $BASEDIR/../deploy/profiles/${CLUSTER}.yaml ]]; then
    progress "💡 Adding cluster type values file ($BASEDIR/../deploy/profiles/${CLUSTER}.yaml)"
    HELM_ARGS="--values $BASEDIR/../deploy/profiles/${CLUSTER}.yaml $HELM_ARGS"
fi

# gather Helm arguments

HELM_ARGS="$HELM_ARGS --timeout=${HELM_TIMEOUT}"
HELM_ARGS="$HELM_ARGS --set global.cluster=$CLUSTER"
HELM_ARGS="$HELM_ARGS --set global.domain=${domain}"
HELM_ARGS="$HELM_ARGS --set coreReleaseName=drogue-doppelgaenger"

echo "Helm arguments: $HELM_ARGS"

# install Drogue IoT

progress "🔨 Deploying Drogue Doppelgänger... "
progress "  ☕ This will take a while!"
progress "  🔬 Track its progress using \`watch kubectl -n $DROGUE_NS get pods\`! "
progress -n "  🚀 Performing deployment... "
helm dependency update "$BASEDIR/../deploy/install"
set -x
# shellcheck disable=SC2086
helm -n "$DROGUE_NS" upgrade drogue-doppelgaenger "$BASEDIR/../deploy/install" --install $HELM_ARGS
set +x
progress "done!"

# source the endpoint information

SILENT=true source "${BASEDIR}/cmd/__endpoints.sh"

# wait for the rest of the deployments

progress -n "⏳ Waiting for deployments to become ready ... "
kubectl wait deployment --timeout=-1s --for=condition=Available -n "$DROGUE_NS" --all
progress "done!"

# show status

progress "📠 Adding cover sheet to TPS report ... done!"
progress "🥳 Deployment ready!"

progress "  API:             ${API_URL}"
progress "  Single Sign On:  ${SSO_URL}"

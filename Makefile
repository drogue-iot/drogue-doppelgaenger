.PHONY: all
all: build-images

CURRENT_DIR ?= $(strip $(shell dirname $(realpath $(lastword $(MAKEFILE_LIST)))))
TOP_DIR ?= $(CURRENT_DIR)
IMAGE_TAG ?= latest
BUILDER_IMAGE ?= ghcr.io/drogue-iot/builder:0.2.3


# evaluate which container tool we use
ifeq (, $(shell which podman 2>/dev/null))
	CONTAINER ?= docker
else
	CONTAINER ?= podman
endif


# evaluate the arguments we need for this container tool
ifeq ($(CONTAINER),docker)
	TEST_CONTAINER_ARGS ?= -v /var/run/docker.sock:/var/run/docker.sock:z --network drogue
	CONTAINER_ARGS ?= -u "$(shell id -u):$(shell id -g)" $(patsubst %,--group-add %,$(shell id -G ))
else ifeq ($(CONTAINER),podman)
	TEST_CONTAINER_ARGS ?= --security-opt label=disable -v $(XDG_RUNTIME_DIR)/podman/podman.sock:/var/run/docker.sock:z
	CONTAINER_ARGS ?= --userns=keep-id
endif


MODULES= \
	backend \
	injector \
	processor \
	server \
	database-migration \


#
# Check if we have a container registry set.
#
.PHONY: require-container-registry
require-container-registry:
ifndef CONTAINER_REGISTRY
	$(error CONTAINER_REGISTRY is undefined)
endif


.PHONY: build-images
build-images:
	set -e; \
	for i in $(MODULES); do \
		$(CONTAINER) build $(TOP_DIR) --build-arg BUILDER_IMAGE=$(BUILDER_IMAGE) --target $${i} -t localhost/drogue-doppelgaenger-$${i}:latest; \
	done


.PHONY: tag-images
tag-images: require-container-registry
	set -e; \
	for i in $(MODULES); do \
		$(CONTAINER) tag localhost/drogue-doppelgaenger-$${i}:latest $(CONTAINER_REGISTRY)/drogue-doppelgaenger-$${i}:$(IMAGE_TAG); \
	done


.PHONY: tag-images
push-images: require-container-registry
	set -e; \
	cd $(TOP_DIR); \
	for i in $(MODULES); do \
		env CONTAINER=$(CONTAINER) ./scripts/bin/retry.sh push -q $(CONTAINER_REGISTRY)/drogue-doppelgaenger-$${i}:$(IMAGE_TAG) &  \
	done; \
	wait

#
# Save all images.
#
.PHONY: save-images
save-images:
	mkdir -p "$(TOP_DIR)/build/images"
	rm -Rf "$(TOP_DIR)/build/images/all.tar"
	$(CONTAINER) save -o "$(TOP_DIR)/build/images/all.tar" $(addprefix localhost/drogue-doppelgaenger-, $(addsuffix :latest, $(MODULES)))


#
# Load image into kind
#
.PHONY: kind-load
kind-load: require-container-registry
	for i in $(MODULES); do \
		kind load docker-image $(CONTAINER_REGISTRY)/drogue-doppelgaenger-$${i}:$(IMAGE_TAG); \
	done


#
# Do a local deploy
#
# For a local deploy, we allow using the default container registry of the project
#
.PHONY: deploy
deploy: CONTAINER_REGISTRY ?= "ghcr.io/drogue-iot"
deploy:
	test -d deploy/helm/charts || git submodule update --init
	env ./scripts/drgadm deploy \
		-s drogueCloudTwin.defaults.images.repository=$(CONTAINER_REGISTRY) \
		-s drogueCloudTwin.defaults.images.tag=latest $(DEPLOY_ARGS)


.PHONY: run-deps
run-deps:
	podman-compose -f $(TOP_DIR)/develop/compose.yaml -f $(TOP_DIR)/develop/compose-health.yaml up

.PHONY: start-deps
start-deps:
	podman-compose -f $(TOP_DIR)/develop/compose.yaml -f $(TOP_DIR)/develop/compose-health.yaml up -d

.PHONY: stop-deps
stop-deps:
	podman-compose -f $(TOP_DIR)/develop/compose.yaml -f $(TOP_DIR)/develop/compose-health.yaml down

.PHONY: restart-deps
restart-deps: stop-deps start-deps


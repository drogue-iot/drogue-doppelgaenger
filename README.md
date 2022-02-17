# Drogue Doppelgänger

It is not a digital twin.

## Running

```shell
podman run \
    --name twin-db \
    -e MONGO_INITDB_ROOT_USERNAME=admin \
    -e MONGO_INITDB_ROOT_PASSWORD=admin1234 \
    -e MONGO_INITDB_DATABASE=twin-db \
    -p 27017:27017 \
    docker.io/library/mongo:5
podman rm twin-db
```
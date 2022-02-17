
## Starting

```shell
podman-compose up
```

## Stopping

```shell
podman-compose down
```

Also deleting the volume:

```shell
podman-compose down -v
```

## Re-create the security key

```shell
openssl rand -base64 756 > replica.key
```

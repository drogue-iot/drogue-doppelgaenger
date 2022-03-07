
# Installation

## MongoDB

```shell
helm -n namespace upgrade --install twindb bitnami/mongodb --values values.yaml
```

## Kafka credentials for the doppelgaenger

you can get the kafka username and secret from the drogue console.

```shell
kubectl create secret generic doppelgaenger-config \
--from-literal=kafka.username=<USERNAME> \
--from-literal=kafka.password=<SECRET>
```

## Deploy prometheus 

```shell
helm install prometheus prometheus-community/prometheus

# Optionnaly, expose the prometheus dashboard (easier debug)
kubectl expose service prometheus-server --type=NodePort --target-port=9090 --name=prom-server

# (for minikube) get the URL to access it 
minikube service prom-server --url
```

## Add Grafana for a dashboard preview

```shell
helm install grafana bitnami/grafana

# expose grafana :
kubectl expose service grafana --type=NodePort --target-port=3000 --name=grafana-server


# (for minikube) get the URL to access it 
minikube service grafana-server --url
```
Add the prometheus server as a source in grafana : `http://prometheus-server:80`
Import the `grafana-dashboard.json` in the grafana UI to see the dashboard. 

## links 
https://opensource.com/article/19/11/introduction-monitoring-prometheus
https://opensource.com/article/21/6/chaos-grafana-prometheus
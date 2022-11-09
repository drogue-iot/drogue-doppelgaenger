RUST_LOG=info

APPLICATION=default
CHECK_DURATION=250ms

STORAGE__DB__HOST=localhost
STORAGE__DB__PORT=5432
STORAGE__DB__DBNAME=drogue
STORAGE__DB__USER=admin
STORAGE__DB__PASSWORD=admin123456

NOTIFIER_SINK__PROPERTIES__BOOTSTRAP_SERVERS=localhost:9092
NOTIFIER_SINK__PROPERTIES__QUEUE_BUFFERING_MAX_MS=50
NOTIFIER_SINK__TOPIC=notifications

NOTIFIER_SOURCE__PROPERTIES__BOOTSTRAP_SERVERS=localhost:9092
NOTIFIER_SOURCE__PROPERTIES__GROUP_ID=server
NOTIFIER_SOURCE__PROPERTIES__SESSION_TIMEOUT_MS=30000
NOTIFIER_SOURCE__PROPERTIES__ENABLE_AUTO_COMMIT=true # might reconsider this
NOTIFIER_SOURCE__TOPIC=notifications

EVENT_SINK__PROPERTIES__BOOTSTRAP_SERVERS=localhost:9092
EVENT_SINK__TOPIC=events

EVENT_SOURCE__PROPERTIES__BOOTSTRAP_SERVERS=localhost:9092
EVENT_SOURCE__PROPERTIES__GROUP_ID=server
EVENT_SOURCE__PROPERTIES__SESSION_TIMEOUT_MS=30000
EVENT_SOURCE__TOPIC=events

INJECTOR__DISABLED=false # <1>
GROUP="${USER}-${HOSTNAME}" # <2>
INJECTOR__SOURCE__MQTT__HOST=mqtt-integration.sandbox.drogue.cloud # <3>
INJECTOR__SOURCE__MQTT__PORT=443
# TODO: modify these if you're using a non-public app
#INJECTOR__SOURCE__MQTT__USERNAME=<user>
#INJECTOR__SOURCE__MQTT__PASSWORD=<token>
INJECTOR__SOURCE__MQTT__TOPIC=\$share/${GROUP}/app/example-app # <4>
INJECTOR__METADATA_MAPPER__TYPE=raw
INJECTOR__METADATA_MAPPER__OVERRIDE_APPLICATION=default
INJECTOR__PAYLOAD_MAPPER__TYPE=simpleJson
INJECTOR__PAYLOAD_MAPPER__ADD_TIMESTAMP=true

COMMAND_SINK__HOST=mqtt-integration.sandbox.drogue.cloud
COMMAND_SINK__PORT=443
# TODO: modify these if you're using a non-public app
#COMMAND_SINK__USERNAME=<user>
#COMMAND_SINK__PASSWORD=<token>

HTTP__DISABLE_TLS=true
RUNTIME__CONSOLE_METRICS__ENABLED=true

#RUNTIME__TRACING=jaeger # <5>
OTEL_TRACES_SAMPLER_ARG=1.0 # 100%

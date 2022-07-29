# To-Do

* [ ] Re-process events: currently we only store them, but don't reprocess missed events
* [ ] Find out what the user can modify, and what is reserved to the internals (like synthetics, lastUpdated, ...)
* [ ] Make `KafkaSource` generic, align with `Notifier` trait
* [ ] Think about handling "by application" limitation.
* [ ] RBAC
* [ ] Allow a way to modify the thing, overriding a non-empty outbox
* [ ] Implement WASM
* [ ] Ensure that reported state "last updated" changes when only the value changes (move logic to machine)

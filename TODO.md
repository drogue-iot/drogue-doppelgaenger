# To-Do

* [ ] Re-process events: currently we only store them, but don't reprocess missed events
* [ ] Find out what the user can modify, and what is reserved to the internals (like synthetics, lastUpdated, ...)
* [ ] Make `KafkaSource` generic, align with `Notifier` trait
* [ ] Think about handling "by application" limitation.
* [ ] RBAC
* [x] Allow a way to modify the thing, overriding a non-empty outbox
  * [ ] Allow more fine-grained control over this
* [ ] Implement WASM
* [x] Ensure that reported state "last updated" changes when only the value changes (move logic to machine)
* [ ] Allow unsetting a desired state, without deleting it

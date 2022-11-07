# To-Do

## 0.2

* [ ] set up a CI
* [ ] Rework the different APIs
  * [ ] Find out what the user can modify, and what is reserved to the internals (like synthetics, lastUpdated, ...)
  * [ ] Which messages can be set
  * [ ] Which external REST based APIs are supported~~~~

## Backlog

* [ ] Think about integrated support for information like (unit/content-type, source timestamp, ...)
* [ ] Make `KafkaSource` generic, align with `Notifier` trait
* [ ] Think about handling "by application" limitation.
* [x] Allow a way to modify the thing, overriding a non-empty outbox
  * [ ] Allow more fine-grained control over this
* [ ] Implement WASM
* [ ] Prevent cyclic events
* [ ] Finer grained RBAC

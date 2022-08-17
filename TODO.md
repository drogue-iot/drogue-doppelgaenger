# To-Do

## 0.1

* [x] ~~RBAC~~ Access control
* [x] Re-process events: currently we only store them, but don't reprocess missed events
* [x] Ensure that reported state "last updated" changes when only the value changes (move logic to machine)
* [x] Allow unsetting a desired state, without deleting it
* [ ] create kubernetes deployment
* [x] create a common `drogue-stuff` repository, containing common code (called `drogue-bazaar`)
* [ ] set up a CI

## Backlog

* [ ] Think about integrated support for information like (unit/content-type, source timestamp, ...)
* [ ] Find out what the user can modify, and what is reserved to the internals (like synthetics, lastUpdated, ...)
* [ ] Make `KafkaSource` generic, align with `Notifier` trait
* [ ] Think about handling "by application" limitation.
* [x] Allow a way to modify the thing, overriding a non-empty outbox
  * [ ] Allow more fine-grained control over this
* [ ] Implement WASM
* [ ] Prevent cyclic events
* [ ] Finer grained RBAC

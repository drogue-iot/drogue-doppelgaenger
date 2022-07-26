'use strict';

class Api {
    constructor(url, application) {
        this.url = new URL(url);

        this.websocketUrl = new URL(url);

        if (this.websocketUrl.protocol === "http:") {
            this.websocketUrl.protocol = "ws:";
        } else if (this.websocketUrl.protocol === "https:") {
            this.websocketUrl.protocol = "wss:";
        }

        this.application = application;
    }

}

class Thing {
    #nextHandle;
    #subscriptions;
    #socket;
    #connecting;

    constructor(api, thing) {
        this.api = api;
        this.thing = thing;
        this.#nextHandle = 0;
        this.#subscriptions = new Map();
        this.#connecting = false;
        // FIXME: change into connecting on first subscribe
        this.#connect();
    }

    #connect() {
        this.#connecting = false;

        const url = this.api.websocketUrl + `api/v1alpha1/things/${encodeURIComponent(this.api.application)}/things/${encodeURIComponent(this.thing)}/notifications`;
        console.debug(`Connecting to: ${url}`);
        this.#socket = new WebSocket(url);
        this.#socket.addEventListener('message', (event) => {
            //console.debug("WS: ", event);
            try {
                const msg = JSON.parse(event.data);
                this.#notifyAll(msg);
            } catch (e) {
                console.info("Failed to notify: ", e);
                this.#socket.close();
                this.#notifyAll({
                    type: "disconnected"
                })
            }
        });
        this.#socket.addEventListener('close', (event) => {
            this.#notifyAll({
                type: "disconnected"
            })
            this.#reconnect();
        });
        this.#socket.addEventListener('error', (event) => {
            this.#notifyAll({
                type: "disconnected"
            })
            this.#reconnect();
        });
    }

    #disconnect() {
        const socket = this.#socket;
        this.#socket = undefined;
        this.#connecting = false;

        socket.close();
    }

    dispose() {
        this.#disconnect();
    }

    #reconnect() {
        // if we had a socket and are not already connecting
        if (this.#socket && !this.#connecting) {
            this.#connecting = true;
            setTimeout(() => {
                this.#connect();
            }, 5000);
        }
    }

    subscribe(callback) {
        const handle = this.nextHandle;
        this.nextHandle += 1;
        const sub = new ThingSubscription(callback, () => {
            this.#unsubscribe(handle);
        });
        this.#subscriptions.set(handle, sub);
    }

    #unsubscribe(handle) {
        let result = this.#subscriptions.delete(handle);
        console.debug("Removed: ${result}")
    }

    #notifyAll(event) {
        for (const sub of this.#subscriptions.values()) {
            sub.notify(event);
        }
    }
}

class ThingSubscription {
    #callback;
    #destroyer;

    constructor(callback, destroyer) {
        this.#callback = callback;
        this.#destroyer = destroyer;
    }

    notify(event) {
        this.#callback(event);
    }

    dispose() {
        if (this.#destroyer !== undefined) {
            this.#destroyer();
            this.#destroyer = undefined;
        }
    }
}

class ThingCard {
    #subscription;
    #card;

    constructor(thing, card, options) {
        this.thing = thing;
        this.#card = card;
        this.options = {
            ...{
                showTimestamps: false,
                labelsToCardStyle: (labels) => {},
                labelsToPropertyStyle: (labels, propertyName) => {},
                refClicked: (ref) => {},
            }, ...options
        };
        this.connected = false;
        this.state = {};
        this.#subscription = this.thing.subscribe((event) => {
            this.#setState(event);
        })
        this.#render();
    }

    dispose() {
        if (this.#subscription !== undefined) {
            this.#subscription.dispose();
        }
    }

    #setState(event) {
        console.debug("Event: ", event);
        this.lastUpdate = new Date();
        switch (event.type) {
            case "initial": {
                this.connected = true;
                this.state = event.thing;
                break;
            }
            case "change": {
                this.connected = true;
                this.state = event.thing;
                break;
            }
            case "disconnected": {
                this.connected = false;
                break;
            }
            default: {
            }
        }

        this.#render();
    }

    #render() {
        if (this.#card === undefined) {
            console.debug("Invalid card target for", this.thing)
            return;
        }

        const labels = this.state?.metadata?.labels || {};

        if (this.connected) {

            let classes = "card ";

            // style
            const style = this.options.labelsToCardStyle(labels);
            //console.debug("Style: ", style);
            switch (style) {
                case "error": {
                    classes += "text-white bg-danger";
                    break;
                }
                case "warning": {
                    classes += "bg-warning";
                    break;
                }
            }

            this.#card.attr('class', classes);
            this.#card.find(".drogue-thing-sub-state").text(`Last Update: ${timestampString(this.lastUpdate)}`)

        } else {
            this.#card.attr('class', "card text-white bg-secondary drogue-thing-sub-disconnected");
            this.#card.find(".drogue-thing-sub-state").text(`Disconnected: ${timestampString(this.lastUpdate)}`)
        }

        const badges = $('<span></span>');
        if (this.state?.metadata?.labels !== undefined) {
            for (const [key, value] of Object.entries(this.state?.metadata?.labels)) {
                if (value === "") {
                    // only add flag labels
                    badges.append($(`<span class="badge bg-secondary">${key}</span>`));
                }
            }
        }
        this.#card.find(".drogue-thing-flags").html(badges);

        // all properties
        this.#card.find("[data-drogue-thing-all-state]").each((idx, element) => {
            const all = $(`<ul class="list-group list-group-flush"></ul>`);
            for (const [key, value] of Object.entries(this.state?.reportedState || {})) {
                // console.debug("Key:", key, " Value:", value);
                const renderedValue = value.value === undefined ? $(`<i>undefined</i>`) : value.value;
                if (this.options.showTimestamps) {
                    const lastUpdate = makeDate(value?.lastUpdate);
                    all.append($(`
<li class="list-group-item d-flex">
    <div class="col-4 fw-bold">${key}</div>
    <div class="col-4 text-end">${renderedValue}</div>
    <div class="col-4 text-muted ps-3 text-end">${timestampString(lastUpdate)}</div>
</li>`))
                } else {
                    all.append($(`
<li class="list-group-item d-flex">
    <div class="col-6 fw-bold">${key}</div>
    <div class="col-6">${renderedValue}</div>
</li>`))
                }

            }
            $(element).html(all);
        })

        // individual properties
        this.#card.find("[data-drogue-thing-reported-state]").each((idx, element) => {
            element = $(element);
            const name = element.data("drogue-thing-reported-state");
            const value = this.state.reportedState?.[name];

            const renderValue = element.find("[data-drogue-thing-render-value]");
            if (renderValue.length) {

                const unit = element.find("[data-drogue-thing-render-unit]").data("drogue-thing-render-unit");
                const lastUpdate = makeDate(value?.lastUpdate);

                let classes = "list-group-item d-flex ";
                if (value !== undefined) {
                    const style = this.options.labelsToPropertyStyle(labels, name);
                    switch (style) {
                        case "error": {
                            classes += "list-group-item-danger";
                            break;
                        }
                        case "warning": {
                            classes += "list-group-item-warning";
                            break;
                        }
                    }
                } else {
                    classes += "list-group-item-secondary";
                }

                element.attr('class', classes);

                let content;
                if (value !== undefined) {
                    content = $(`<span>${value?.value}</span>`);
                    if (unit !== undefined) {
                        content.append(unit);
                    }
                    if (this.options.showTimestamps) {
                        content.append($(`<small class="text-muted">(${timestampString(lastUpdate)})</small>`))
                    }
                } else {
                    content = $(`<i>unknown</i>`);
                }

                renderValue.html(content);
            }

            const renderRefs = element.find("[data-drogue-thing-render-refs]");
            if (renderRefs.length) {
                const lastUpdate = makeDate(value?.lastUpdate);

                let classes = "list-group-item d-flex ";
                if (value !== undefined) {
                    const style = this.options.labelsToPropertyStyle(labels, name);
                    switch (style) {
                        case "error": {
                            classes += "list-group-item-danger";
                            break;
                        }
                        case "warning": {
                            classes += "list-group-item-warning";
                            break;
                        }
                    }
                } else {
                    classes += "list-group-item-secondary";
                }

                element.attr('class', classes);

                let content;
                if (value?.value) {
                    content = $(`<ul class="drogue-ref-group">`);
                    for (const [ref, v] of Object.entries(value?.value)) {
                        const link = $(`<a class="drogue-thing-ref">${ref}</a>`);
                        link.on("click", ()=> {
                            console.debug("Clicked: ", ref);
                            this.options.refClicked(ref);
                        });
                        const item = $(`<li></li>`);
                        item.append(link);
                        content.append(item);
                    }
                    if (this.options.showTimestamps) {
                        content.append($(`<small class="text-muted">(${timestampString(lastUpdate)})</small>`))
                    }
                } else {
                    content = $(`<i>none</i>`);
                }

                renderRefs.html(content);
            }


        });
    }
}

/// Parse a JSON date into a date, handling undefined.
function makeDate(value) {
    if (value) {
        let ts = Date.parse(value);
        if (isNaN(ts)) {
            return undefined;
        }
        return new Date(ts);
    } else {
        return undefined;
    }
}

// render a timestamp
function timestampString(date) {
    if (date === undefined) {
        return "<unknown>";
    }

    return new Intl.DateTimeFormat([], {
        year: "numeric", month: "numeric", day: "numeric",
        hour: "2-digit", minute: "2-digit", second: "2-digit",
        timeZoneName: "short"
    }).format(date);
}
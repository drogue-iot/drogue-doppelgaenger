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
        this.#subscriptions.delete(handle);
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

    destroy() {
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
            }, ...options
        };
        this.connected = false;
        this.state = {};
        this.#render();
        this.#subscription = this.thing.subscribe((event) => {
            this.#setState(event);
        })
    }

    destroy() {
        this.#subscription.destroy();
    }

    #setState(event) {
        console.debug("Event: ", event);
        switch (event.type) {
            case "change": {
                this.connected = true;
                this.lastUpdate = new Date();
                this.state = event.thing;
                break;
            }
            case "disconnected": {
                this.connected = false;
                this.lastUpdate = new Date();
                break;
            }
            default: {
            }
        }

        this.#render();
    }

    #render() {
        const labels = this.state?.metadata?.labels || {};

        if (this.connected) {

            let classes = "card ";

            // style
            const style = this.options.labelsToCardStyle(labels);
            console.log("Style: ", style);
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
            this.#card.find(".drogue-thing-sub-state").text(`Last Update: ${this.lastUpdate?.toISOString()}`)

        } else {
            this.#card.attr('class', "card text-white bg-secondary drogue-thing-sub-disconnected");
            this.#card.find(".drogue-thing-sub-state").text(`Disconnected: ${this.lastUpdate?.toISOString()}`)
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


        // properties
        this.#card.find("[data-drogue-thing-reported-state]").each((idx, element) => {
            element = $(element);
            const name = element.data("drogue-thing-reported-state");
            const value = this.state.reportedState?.[name];
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
                    content.append($(`<small class="text-muted">(${lastUpdate?.toISOString()})</small>`))
                }
            } else {
                content = $(`<i>unknown</i>`);
            }

            element.find("[data-drogue-thing-render-value]").html(content);

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
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

    constructor(api, thing) {
        this.api = api;
        this.thing = thing;
        this.#nextHandle = 0;
        this.#subscriptions = new Map();
        // FIXME: change into connecting on first subscribe
        this.#connect();
    }

    #connect() {
        const url = this.api.websocketUrl + `api/v1alpha1/things/${encodeURIComponent(this.api.application)}/things/${encodeURIComponent(this.thing)}/notifications`;
        console.debug(`Connecting to: ${url}`);
        this.socket = new WebSocket(url);
        this.socket.addEventListener('message', (event) => {
            //console.debug("WS: ", event);
            try {
                const msg = JSON.parse(event.data);
                this.#notifyAll(msg);
            }
            catch (e) {
                this.socket.close();
                this.#notifyAll({
                    type: "disconnected"
                })
            }
        });
        this.socket.addEventListener('close', (event) => {
            this.#notifyAll({
                type: "disconnected"
            })
            this.#connect();
        });
        this.socket.addEventListener('error', (event) => {
            this.#notifyAll({
                type: "disconnected"
            })
            this.#connect();
        });
    }

    #disconnect() {
        this.socket.close();
        delete this.socket;
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

    constructor(thing, card) {
        this.thing = thing;
        this.#card = card;
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
            default: {}
        }

        this.#render();
    }

    #render() {
        if (this.connected) {
            this.#card.removeClass("text-white bg-secondary drogue-thing-sub-disconnected");
            this.#card.find(".drogue-thing-sub-state").text(`Last Update: ${this.lastUpdate}`)
        } else {
            this.#card.addClass("text-white bg-secondary drogue-thing-sub-disconnected");
            this.#card.find(".drogue-thing-sub-state").text(`Disconnected: ${this.lastUpdate}`)
        }

        const labels = $('<span></span>');
        console.log("Labels", this.state?.metadata?.labels);
        if (this.state?.metadata?.labels !== undefined) {
            for (const [key, value] of Object.entries(this.state?.metadata?.labels)) {
                if (value === "") {
                    // only add flag labels
                    labels.append($(`<span class="badge bg-secondary">${key}</span>`));
                }
            }
        }
        console.debug(labels);
        this.#card.find(".drogue-thing-flags").html(labels);

        this.#card.find(".drogue-thing-state").text(JSON.stringify({reported: this.state.reportedState}, null, 2));
    }
}
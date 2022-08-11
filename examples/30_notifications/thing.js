'use strict';

class Api {
    constructor(url, application, options) {
        this.options = {
            ...{
            }, ...options
        };

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
        if (this.api.options.tokenProvider !== undefined) {
            console.debug("Need to fetch a token first");
            this.api.options.tokenProvider()
                .then((token) => {
                    console.debug("Got the token, now we can connect");
                    this.#connectWithToken(token)
                })
                .catch(() => {
                    console.log("Failed to get token");
                    this.#notifyAll({
                        type: "disconnected"
                    })
                    this.#reconnect();
                })
        } else {
            this.#connectWithToken();
        }
    }

    #connectWithToken(token) {
        let url = new URL(this.api.websocketUrl + `api/v1alpha1/things/${encodeURIComponent(this.api.application)}/things/${encodeURIComponent(this.thing)}/notifications`);

        if (token !== undefined) {
            url.searchParams.set("token", token);
        }

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
        this.#socket.addEventListener('close', () => {
            this.#notifyAll({
                type: "disconnected"
            })
            this.#reconnect();
        });
        this.#socket.addEventListener('error', () => {
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

    setDesiredValues(values) {
        this.#send({
            type: "setDesiredValues",
            thing: this.thing,
            values,
        })
    }

    #send(message) {
        if (this.#socket !== undefined) {
            const payload = JSON.stringify(message);
            console.debug("Sending:", payload);
            this.#socket.send(payload);
        } else {
            console.debug("Skipping message, not connected");
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
                labelsToCardStyle: (_labels) => {
                },
                labelsToPropertyStyle: (_labels, _propertyName) => {
                },
                refClicked: (_ref) => {
                },
                showDesired: false,
                controlDesired: false,
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

        this.#mergeStates();

        this.#render();
    }

    #mergeStates() {
        if (this.state !== undefined) {
            const states = {};
            for (const [key, value] of Object.entries(this.state?.reportedState || {})) {
                states[key] = value;
            }
            for (const [key, value] of Object.entries(this.state?.syntheticState || {})) {
                const reported = states[key];
                states[key] = value;
                states[key].reported = reported;
                states[key].synthetic = true;
            }
            for (const [key, value] of Object.entries(this.state?.desiredState || {})) {
                if (states[key] === undefined) {
                    states[key] = {desired: value};
                } else {
                    states[key].desired = value;
                }
            }
            this.state.mergedState = states;
        }
    }

    render() {
        this.#render();
    }

    #render() {
        if (this.#card === undefined) {
            console.debug("Invalid card target for", this.thing)
            return;
        }

        const labels = this.state?.metadata?.labels || {};

        if (this.connected && (this.state?.metadata?.deletionTimestamp === undefined)) {

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
            this.#card.find(".drogue-thing-sub-state").text(`Last Update: ${timestampString(this.lastUpdate, true)}`)

        } else {
            this.#card.attr('class', "card text-white bg-secondary drogue-thing-sub-disconnected");
            this.#card.find(".drogue-thing-sub-state").text(`Disconnected: ${timestampString(this.lastUpdate, true)}`)
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
            this.#renderAllState($(element));
        })

        // individual properties
        this.#card.find("[data-drogue-thing-reported-state]").each((idx, element) => {
            const name = element.data("drogue-thing-reported-state");
            if (name !== undefined) {
                this.#renderState($(element), name);
            }
        });
    }

    // Render using "all states" method
    #renderAllState(element) {
        const all = $(`<ul class="list-group list-group-flush">
</ul>`);

        let headers = $(`
<li class="list-group-item d-flex">
    <div class="col fw-bold fst-italic">Name</div>
    <div class="col fw-bold text-end fst-italic">Value</div>
    <div class="col fw-bold text-end fst-italic">Last Update</div>
</li>`);

        if (this.options.showDesired) {
            headers.append($(`
<div class="col fw-bold text-end fst-italic">Desired</div>
<div class="col fw-bold text-end fst-italic">Last update</div>
<div class="col fw-bold text-end fst-italic">State</div>
<div class="col fw-bold text-end fst-italic">Valid until</div>
`));
            if (this.options.controlDesired) {
                headers.append($(`
<div class="col fw-bold text-end fst-italic"></div>
`));
            }
        }
        all.append(headers);

        const parentTarget = $('[data-drogue-thing-parent]');
        parentTarget.empty();

        for (const [key, value] of Object.entries(this.state?.mergedState || {}).sort()) {
            // console.debug("Key:", key, " Value:", value);

            let renderedValue;
            let renderedType;
            if (key.startsWith("$")) {
                renderedValue = renderReferences(value.value);
                renderedType = "";
            } else {
                renderedValue = renderValue(value.value);
                renderedType = renderType(value.value);
            }

            const lastUpdate = makeDate(value?.lastUpdate);

            let keyExtras = "";

            if (value.synthetic) {
                keyExtras += ` <span class="badge text-bg-light">syn</span>`;
            }

            if (key === '$parent') {
                if (parentTarget) {
                    parentTarget.html(` ${renderReference(value.value, '<i class="bi bi-arrow-90deg-up"></i>')}`);
                    this.#bindOnClickRef(parentTarget);
                    continue;
                } else {
                    keyExtras += ` ${renderReference(value.value, '<i class="bi bi-arrow-90deg-up"></i>')}`;
                }
            }

            let row = $(`
<li class="list-group-item d-flex">
    <div class="col fw-bold">${key}${keyExtras}</div>
    <div class="col pe-4 text-end text-truncate">${renderedValue} ${renderedType}</div>
    <div class="col text-muted text-end">${timestampString(lastUpdate, !this.options.showDesired)}</div>
</li>`);

            this.#bindOnClickRef(row);

            if (this.options.showDesired) {
                if (value.desired !== undefined) {
                    const renderedDesiredValue = renderValue(value.desired.value);
                    const recon = value.desired.reconciliation;

                    let renderedState;
                    let when = "";
                    switch (value.desired.reconciliation.state) {
                        case "reconciling":
                            when = timestampString(recon.lastAttempt);
                            renderedState = `
<div class="spinner-border spinner-border-sm" role="status"><span class="visually-hidden">Loading...</span></div>
`;
                            break;
                        case "succeeded":
                            when = timestampString(recon.when);
                            renderedState = `<span class="badge text-bg-success">success</span>`;
                            break;
                        case "failed":
                            when = timestampString(recon.when);
                            renderedState = `<span class="badge text-bg-danger">failed</span>`;
                            break;
                        default:
                            renderedState = `<span class="badge text-bg-secondary">unknown (${value.desired.reconciliation.state})</span>`;
                            break;
                    }

                    const lastUpdate = makeDate(value?.desired.lastUpdate);

                    let validUntil = value?.desired.validUntil;
                    if (validUntil === undefined) {
                        validUntil = "∞";
                    } else {
                        validUntil = timestampString(validUntil);
                    }

                    row.append($(`
<div class="col text-end">${renderedDesiredValue} ${renderType(value.desired.value)}</div>
<div class="col text-muted text-end">${timestampString(lastUpdate)}</div>
<div class="col text-end"> <span>${renderedState}</span> <span class="text-muted">${when}</span></div>
<div class="col text-muted text-end">${validUntil}</div>
`))
                } else {
                    row.append($(`<div class="col"></div><div class="col"></div><div class="col"></div><div class="col"></div>`))
                }

                if (this.options.controlDesired) {
                    if (value.desired !== undefined) {
                        row.append($(`<div class="col text-end"><button class="btn btn-outline-secondary" data-set-desired-start="${key}">Set</button></div>`));
                    } else {
                        row.append($(`<div class="col text-end"></div>`));
                    }
                }

            }

            all.append(row);

        }

        all.find("button[data-set-desired-start]").on("click", (event) => {
            const name = $(event.currentTarget).attr("data-set-desired-start");
            this.#showSetDesiredModal(name);
        });

        element.html(all);
    }

    #showSetDesiredModal(name) {
        $('#set-desired-modal').remove();
        $(document.body).prepend($(`
<div id="set-desired-modal" class="modal" tabindex="-1">
    <div class="modal-dialog modal-dialog-centered">
        <div class="modal-content">
            <div class="modal-header">
                <h5 class="modal-title">Set desired state</h5>
                <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Close"></button>
            </div>
        <div class="modal-body">
            <div class="container-fluid">
<form method="dialog">
    <input type="submit" hidden />
    <div class="mb-3">
        <label for="desiredValueInput" class="form-label">Desired value</label>
        <input type="text" class="form-control" id="desiredValueInput" aria-describedby="desiredValueInputHelp">
        <div id="desiredValueInputHelp" class="form-text">A JSON value. If it isn't valid JSON, it will be used as String</div>
    </div>
    <div class="mb-3 form-check">
        <input type="checkbox" class="form-check-input" id="desiredAsString">
        <label class="form-check-label" for="desiredAsString">Force to string</label>
    </div>
    <div class="mb-3">
        <label class="form-label">Value <span id="desiredRenderedType"></span></label>
        <div>
            <pre><code id="desiredRenderedValue"></code>&nbsp;</pre>
        </div>
    </div>
    <div class="mb-3">
        <label for="desiredValidForInput" class="form-label">Valid for</label>
        <input type="text" class="form-control" id="desiredValidForInput" aria-describedby="desiredValidForInputHelp">
        <div id="desiredValidForInputHelp" class="form-text">An optional human-readable duration (e.g. <code>10s</code>, <code>5m</code>, or <code>10m 30s)</code></div>
    </div>
</form>
                </div>
            </div>
            <div class="modal-footer">
                <button type="button" class="btn btn-outline-secondary" data-bs-dismiss="modal">Cancel</button>
                <button type="button" class="btn btn-secondary" id="set-desired-model-btn-clear">Clear</button>
                <button type="button" class="btn btn-primary" id="set-desired-model-btn-set">Set</button>
            </div>
        </div>
    </div>
</div>
`));

        const modal = new bootstrap.Modal('#set-desired-modal', {});

        function getValue() {
            let forceString = $('#desiredAsString').prop("checked");
            let value = $('#desiredValueInput').val();

            if (!forceString) {
                if (value === "") {
                    return null;
                }
                try {
                    value = JSON.parse(value);
                }
                catch {}
            }

            return value;
        }

        function validate() {
            let value = getValue();
            let type = renderType(value);
            value = renderValue(value, true);

            $('#desiredRenderedType').html(type);
            $('#desiredRenderedValue').html(value);
        }

        validate();

        function doSubmit(thing) {
            const value = getValue();

            let validFor = $('#desiredValidForInput').val();
            if (validFor === "") {
                validFor = undefined;
            }

            console.log(thing);
            thing.setDesiredValues({[name]: {value, validFor}});
            modal.hide();
        }

        let m = $('#set-desired-modal');
        m.find('input').on('keyup', validate );
        m.find('form').on('submit', () => {
            doSubmit(this);
        });

        const thing = this.thing;
        $('#set-desired-model-btn-set').on('click', (event)=> {
            doSubmit(thing);
        });
        $('#set-desired-model-btn-clear').on('click', (event)=> {
            thing.setDesiredValues({[name]: {value: null}});
            modal.hide();
        });

        m.on("shown.bs.modal", () => {
            $("#desiredValueInput").focus();
        });

        modal.show();
    }

    // Render a single state
    #renderState(element, name) {
        const value = this.state.mergedState?.[name];

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
                for (const [ref, _] of Object.entries(value?.value)) {
                    const link = $(`<a class="drogue-thing-ref">${ref}</a>`);
                    this.#bindOnClickRef(link);
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


    }

    #bindOnClickRef(target) {
        target.find('a[data-drogue-ref]').on('click', (event) => {
            const ele = $(event.currentTarget);
            const ref = ele.attr('data-drogue-ref');
            this.options.refClicked(ref);
        })
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
function timestampString(date, noBreak) {
    if (date === undefined) {
        return "<unknown>";
    }

    if (typeof date === "string") {
        date = new Date(date);
    }

    const d = new Intl.DateTimeFormat([], {
        year: "numeric", month: "numeric", day: "numeric",
    }).format(date);

    const t = new Intl.DateTimeFormat([], {
        hour: "2-digit", minute: "2-digit", second: "2-digit",
        timeZoneName: "short"
    }).format(date);

    if (noBreak) {
        return t + " " + d;
    } else {
        return t + "<br>" + d;
    }
}

function renderType(value) {
    let type = typeof value;
    if (value === null) {
        type = 'null';
    }

    return `<span class="badge text-bg-light">${type}</span>`;
}

function renderReference(value, label) {
    let encodedValue = value.replace(/[\u00A0-\u9999<>&]/g, function(i) {
        return '&#'+i.charCodeAt(0)+';';
    });

    if (label === undefined) {
        label = encodedValue;
    }

    return `<a class="drogue-thing-ref" data-drogue-ref="${encodedValue}">${label}</a>`;
}

function renderReferences(value) {

    if (typeof value === "object") {
        let refs = ``;
        for (const [k, _] of Object.entries(value)) {
            refs += `<li>${renderReferences(k)}</li>`;
        }
        return `<ul class="drogue-ref-group">${refs}</ul>`;
    } else if (typeof value === "string" ) {
        return renderReference(value);
    } else {
        return "";
    }

}

function renderValue(value, pretty) {
    if (value === undefined) {
        return "";
    } else if (value === null) {
        return "";
    } else if ( ((typeof value) === "object") || ((typeof value) === "array") ) {
        if (pretty) {
            return JSON.stringify(value, null, 2);
        } else {
            return JSON.stringify(value);
        }
    } else {
        return value.toString();
    }
}
function encodeDevice(device) {
    return device.replaceAll(/[^a-zA-Z0-9]/g, "-")
}

function updateDevice(update) {
    //console.log(update)

    const device = update.document.device;
    const state = update.document;
    const cardId = "device-card-" + encodeDevice(device);

    let card = $("#" + cardId);

    if (card.length === 0) {

        // insert
        card = $(`
<div class="col">
    <div class="card">
        <div class="card-header"></div>
        <div class="device-content"></div>
    </div>
</div>
`)

        card.attr("id", cardId)
        card.attr("data-device", device)
        card.find(".card-header").text(device)

        const cards = $("#devices");
        let best = null;
        cards.children().each((idx, element) => {
            const currentDevice = $(element).attr("data-device");

            if (!currentDevice) {
                return;
            }

            if (device.localeCompare(currentDevice) > 0) {
                if (best === null || (best.device.localeCompare(currentDevice) < 0)) {
                    best = {device: currentDevice, element};
                }
            }
        })

        if (best) {
            $(best.element).after(card)
        } else {
            cards.prepend(card)
        }

    }

    const content = render(state);

    // update
    card.find(".device-content").empty().append(content);
    card.addClass("updated");
    setTimeout(() => {
        card.removeClass("updated");
    }, 200);
}

function render(state) {
    const content = $(`<ul class="list-group list-group-flush"></ul>`);

    if (state.payload !== undefined) {
        const payload = state.payload["$binary"];
        try {
            state = JSON.parse(ENC.decode(payload));
            console.log("State:", state)
        }
        catch(err) {
            console.log("Unable to decode: ", err)
            state.payload["$binary"] = "Failed to decode: " + err + "\n<br><br>\n" + payload
        }

    }

    if (state.features !== undefined) {
        console.debug("Using JSON payload:", state.features);
        for (const feature of Object.keys(state.features).sort()) {
            let properties;
            if (state.features[feature].properties !== undefined) {
                properties = state.features[feature].properties;
            } else {
                properties = state.features[feature];
            }
            content.append(renderFeature(feature, properties))
        }

    } else if (state.payload !== undefined) {
        const payload = state.payload["$binary"];
        // console.debug("Using binary payload: ", payload);
        content.append($(`<div><code>${payload}</code></div>`));

    }

    return content;
}

function renderFeature(feature, properties) {
    let timestamp = properties["timestamp"];
    if (typeof timestamp === "number") {
        let date = new Date(timestamp);
        timestamp = date.toISOString();
    }

    if (typeof timestamp === "string") {
        delete properties.timestamp;
        timestamp = ` <span class="badge bg-light text-dark">${timestamp}</span>`;
    } else {
        timestamp = ""
    }

    let content = $(`<div class="row"></div>`);
    content.append($(`<div class="col feature-title">${feature}${timestamp}</div>`));

    for (const property of Object.keys(properties).sort()) {
        let value = properties[property];

        const row = $(`<div class="row"></div>`);
        row.append($(`<div class="col-3 property-title">${property}</div>`));

        const t = typeof value;
        if (t !== "string" && t !== "boolean" && t !== "number") {
            value = $(`<code><pre>${JSON.stringify(value, null, 2)}</pre></code>`);
        }
        row.append($(`<div class="col-9"></div>`).append(value));

        content = content.add(row);
        //content.append($(`<dt>${property}</dt><dd><code><pre>${JSON.stringify(value, null, 2)}</pre></code></dd>`))
    }

    return $(`<li class="list-group-item"></li>`).append(content);
}


function bufferToHex (buffer) {
    return [...new Uint8Array (buffer)]
        .map (b => b.toString (16).padStart (2, "0"))
        .join ("");
}

class Encryption {

    #ready = false;
    #sessionKey;
    #session;

    constructor() {
    }

    #checkReady() {

        if (this.#ready && this.#sessionKey) {

            this.#session = undefined;

            const session = new Olm.InboundGroupSession();
            session.create(this.#sessionKey);

            this.#session = session;

        }

    }

    set sessionKey(key) {
        this.#sessionKey = key;

        this.#checkReady();
    }

    set ready(ready) {
        this.#ready = ready;

        this.#checkReady();
    }

    decode(payload) {
        if (!this.#ready) {
            throw "Not ready yet";
        }

        if (!this.#session) {
            throw "Session key missing";
        }

        payload = payload.replace(/=+$/, "");

        //console.log("Decoding: ", payload)

        return this.#session.decrypt(payload).plaintext;
    }

}

const ENC = new Encryption();

function olmReady() {
    ENC.ready = true;
}

function olmSetSessionKey(key) {
    ENC.sessionKey = key;
}
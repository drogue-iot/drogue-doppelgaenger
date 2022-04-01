
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
                if (best === null || ( best.device.localeCompare(currentDevice) < 0)) {
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
    for (const feature of Object.keys(state.features).sort()) {
        const properties = state.features[feature].properties;
        content.append(renderFeature(feature, properties))
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
        if (t !== "string" && t !== "boolean" && t !== "number" ) {
            value = $(`<code><pre>${JSON.stringify(value, null, 2)}</pre></code>`);
        }
        row.append($(`<div class="col-9"></div>`).append(value));

        content = content.add(row);
        //content.append($(`<dt>${property}</dt><dd><code><pre>${JSON.stringify(value, null, 2)}</pre></code></dd>`))
    }

    return $(`<li class="list-group-item"></li>`).append(content);
}
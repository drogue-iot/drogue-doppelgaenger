function updateDevice(update) {
    //console.log(update)

    const device = update.document.device;
    const state = update.document;
    const cardId = "device-card-" + device;

    let card = $("#" + cardId);

    if (card.length === 0) {

        // insert
        card = $(`
<div class="col">
    <div class="card">
        <div class="card-header"></div>
        <div class="card-body">
            <p class="card-text"></p>
        </div>
    </div>
</div>
`)

        card.attr("id", cardId)
        card.attr("data-device", device)
        card.find(".card-header").text(device)

        const cards = $("#devices");
        let best = null;
        cards.children().each((idx, element) => {
            console.log(element)
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

    const content = $(`<dl></dl>`);
    for (const feature of Object.keys(state.features).sort()) {
        const properties = state.features[feature]
        content.append($(`<dt>${feature}</dt><dd><code>${JSON.stringify(properties, null, 2)}</code></dd>`))
    }

    // update
    card.find(".card-text").empty().append(content)
    card.addClass("updated");
    setTimeout(() => {
        card.removeClass("updated");
    }, 200);
}
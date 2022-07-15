
function updateLabel(key, value) {
    if (value !== undefined) {
        if (newState.metadata.labels === undefined) {
            newState.metadata.labels = {};
        }
        newState.metadata.labels[key] = value;
    } else {
        if (newState.metadata.labels !== undefined) {
            delete newState.metadata.labels[key];
        }
    }
}

function flagLabel(key, state) {
    updateLabel(key, state ? "" : undefined)
}

// check over temp
flagLabel("overTemp", newState?.reportedState?.temperature?.value > 60);
flagLabel("highTemp", newState?.reportedState?.temperature?.value > 50);

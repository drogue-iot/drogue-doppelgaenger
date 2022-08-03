
function updateLabel(key, value) {
    if (value !== undefined) {
        if (context.newState.metadata.labels === undefined) {
            context.newState.metadata.labels = {};
        }
        context.newState.metadata.labels[key] = value;
    } else {
        if (context.newState.metadata.labels !== undefined) {
            delete context.newState.metadata.labels[key];
        }
    }
}

function flagLabel(key, state) {
    updateLabel(key, state ? "" : undefined)
}

// check over temp
flagLabel("overTemp", context.newState?.reportedState?.temperature?.value > 60);
flagLabel("highTemp", context.newState?.reportedState?.temperature?.value > 50);

if (newState.metadata.annotations === undefined) {
    newState.metadata.annotations = {};
}
if (newState.reportedState === undefined) {
    newState.reportedState = {};
}

const lastUpdate = new Date().toISOString();
const value = (newState.reportedState["timer"]?.value || 0) + 1;
newState.reportedState["timer"] = { value, lastUpdate };

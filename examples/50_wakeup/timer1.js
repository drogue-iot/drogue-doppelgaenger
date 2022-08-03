if (context.newState.metadata.annotations === undefined) {
    context.newState.metadata.annotations = {};
}
if (context.newState.reportedState === undefined) {
    context.newState.reportedState = {};
}

const lastUpdate = new Date().toISOString();
const value = (context.newState.reportedState["timer"]?.value || 0) + 1;
context.newState.reportedState["timer"] = { value, lastUpdate };

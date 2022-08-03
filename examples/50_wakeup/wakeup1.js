
wakeup("1s");

let counter = context.newState.reportedState?.["counter"]?.value;

if (counter === undefined) {
    if (context.newState.reportedState === undefined) {
        context.newState.reportedState = {};
    }

    context.newState.reportedState["counter"] = {value: 1, lastUpdate: new Date().toISOString()};
} else {
    context.newState.reportedState["counter"].value = counter + 1;
    context.newState.reportedState["counter"].lastUpdate = new Date().toISOString();
}

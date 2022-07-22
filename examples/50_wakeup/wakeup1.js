// set the waker
function wakeup(delay) {
    waker = delay;
}

wakeup("1s");

let counter = newState.reportedState?.["counter"]?.value;

if (counter === undefined) {
    if (newState.reportedState === undefined) {
        newState.reportedState = {};
    }

    newState.reportedState["counter"] = {value: 1, lastUpdate: new Date().toISOString()};
} else {
    newState.reportedState["counter"].value = counter + 1;
    newState.reportedState["counter"].lastUpdate = new Date().toISOString();
}

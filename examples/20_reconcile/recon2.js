const WARNING_THRESHOLD = 20;
const ALARM_THRESHOLD = 28;
const PROPERTY = "temp";

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
flagLabel("highTemp", newState?.reportedState?.[PROPERTY]?.value > WARNING_THRESHOLD);
flagLabel("overTemp", newState?.reportedState?.[PROPERTY]?.value > ALARM_THRESHOLD);

function log(text) {
    //logs.push(text)
}

//log(`Before: ${JSON.stringify(newState, null, 2)}`);

function changed(property) {
    let currentValue = currentState?.reportedState?.[property]?.value;
    let newValue = newState?.reportedState?.[property]?.value;

    return currentValue !== newValue;
}

function changedAnd(property, predicate) {
    let currentValue = currentState?.reportedState?.[property]?.value;
    let newValue = newState?.reportedState?.[property]?.value;

    return (currentValue !== newValue) && predicate(newValue);
}

function whenChanged(property, callback, or) {
    let currentValue = currentState?.reportedState?.[property]?.value;
    let newValue = newState?.reportedState?.[property]?.value;

    let orResult = false;
    if (or !== undefined) {
        orResult = or(newValue);
    }

    if ( (currentValue !== newValue) || orResult) {
        callback(newValue, currentValue);
    }
}

function whenConditionChanged(condition, property, mapper, callback) {
    const conditionAnnotation = "condition/" + condition;
    const hasAnnotation = newState.metadata.annotations?.[conditionAnnotation] !== undefined;

    log(`Has annotation: ${hasAnnotation}`);

    whenChanged(property, (newValue, oldValue) => {

        // add annotation
        if (newState.metadata.annotations === undefined) {
            newState.metadata.annotations = {};
        }
        newState.metadata.annotations[conditionAnnotation] = "true";

        const newCondition = mapper(newValue);
        const oldCondition = mapper(oldValue);

        log(`Eval condition - old: ${oldCondition}, new: ${newCondition}`);

        if (!hasAnnotation || newCondition !== oldCondition) {
            callback(newCondition);
        }

    }, () => { return !hasAnnotation; } )
}

// Should be a system function
function sendMessage(thing, message) {
    log(`Schedule message - Thing: ${thing}, Message: ${JSON.stringify(message, null, 2)}`);
    outbox.push({thing, message});
}

function sendMerge(thing, merge) {
    sendMessage(thing, {merge})
}

function sendPatch(thing, patch) {
    sendMessage(thing, {patch})
}

function addReference(thing) {
    const me = newState.metadata.name;
    const lastUpdate = new Date().toISOString();
    sendMerge(thing, {
        reportedState: {
            "$refs": {
                lastUpdate,
                value: {
                    [me]: {},
                }
            }
        }
    })
}

function removeReference(thing) {
    const me = newState.metadata.name;
    const lastUpdate = new Date().toISOString();
    sendMerge(thing, {
        reportedState: {
            "$refs": {
                lastUpdate,
                value: {
                    [me]: null,
                }
            }
        }
    })
}

//log(`Post(functions): ${JSON.stringify(newState, null, 2)}`);

/*
Renaming doesn't work. We need synthetics for this!

renameReportedState("temp", "temperature");
renameReportedState("batt", "battery");
renameReportedState("hum", "humidity");
renameReportedState("geoloc", "location");
*/

//log(`Post(renameReportedState): ${JSON.stringify(newState, null, 2)}`);

whenConditionChanged("overTemp", PROPERTY, (value) => {
    return value > ALARM_THRESHOLD;
}, (condition) => {
    log(`Condition change: ${condition}`);
    if (condition) {
        addReference("overTempGroup");
    } else {
        removeReference("overTempGroup");
    }
});


//log(`Post(whenConditionChanged): ${JSON.stringify(newState, null, 2)}`);
// Add a message to the log output.
function log(msg) {
    context.logs.push(msg);
}

// Schedule a message to be sent.
function sendMessage(thing, message) {
    context.outbox.push({thing, message});
}

// Set the waker
function wakeup(delay) {
    context.waker = delay;
}
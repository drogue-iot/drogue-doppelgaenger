const degc = context.newState.reportedState?.["temperature"]?.value;

if (degc !== undefined) {
    degc * 9/5 + 32
} else {
    undefined
}

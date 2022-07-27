const value1 = input.newState.reportedState?.["value1"]?.value;
const value2 = input.newState.reportedState?.["value2"]?.value;

if (value1 !== undefined && value2 !== undefined) {
    value1 + value2
} else {
    null
}

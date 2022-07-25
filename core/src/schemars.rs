use schemars::{
    gen::SchemaGenerator,
    schema::{Metadata, Schema, SchemaObject},
    JsonSchema,
};

pub(crate) fn humantime(gen: &mut SchemaGenerator) -> Schema {
    let mut schema: SchemaObject = <String>::json_schema(gen).into();
    schema.metadata = Some(Box::new(Metadata {
        id: None,
        title: Some("Human readable duration".to_string()),
        description: Some(r#"Durations in a format intended for humans:

The format uses a combination of amount and time unit (like: h, m, s) to define a duration. For example:

* `1m` = 1 minute = 60 seconds
* `30s` = 30 seconds
* `1h 30m` = 90 minutes

Time units are:

* ms, millis, msec = milliseconds 
* s, seconds, second, secs, sec = seconds
* m, minutes, minute, mins, min = minutes
* h, hours, hour, hrs, hrs = hours
* d, days, day = days
* w, weeks, week = weeks
* M, months, month = month (=30.44d)
* y, years, year = year (=365.25d)

"#.to_string()),
        default: None,
        deprecated: false,
        read_only: false,
        write_only: false,
        examples: vec!["1m".into(), "30s".into(), "1h 30m".into()],
    }));
    schema.into()
}

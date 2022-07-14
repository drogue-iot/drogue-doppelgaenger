use schemars::gen::SchemaSettings;
use schemars::schema_for;

fn main() {
    let schema = schemars::gen::SchemaGenerator::from(SchemaSettings::openapi3())
        .into_root_schema_for::<drogue_doppelgaenger_common::model::Thing>();
    println!("{}", serde_yaml::to_string(&schema).unwrap());
}

use schemars::gen::SchemaSettings;

#[derive(schemars::JsonSchema)]
#[allow(unused)]
struct Wrapper {
    error_information: drogue_doppelgaenger_core::error::ErrorInformation,
    thing: drogue_doppelgaenger_core::model::Thing,
    desired_state_update: drogue_doppelgaenger_core::service::DesiredStateUpdate,
}

fn main() {
    let schema = schemars::gen::SchemaGenerator::from(SchemaSettings::openapi3())
        .into_root_schema_for::<Wrapper>();
    println!("{}", serde_yaml::to_string(&schema).unwrap());
}

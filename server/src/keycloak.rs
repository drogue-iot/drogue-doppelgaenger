use ::keycloak::types::CredentialRepresentation;
use keycloak::KeycloakError;
use std::collections::HashMap;
use std::fmt::Display;

pub const SERVICE_CLIENT_SECRET: &str = "bfa33cd2-18bd-11ed-a9f9-d45d6455d2cc";

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Keycloak {
    /// Disable keycloak setup.
    ///
    /// We use a dedicated "disable" flag, so that we can run with defaults and keycloak enabled
    /// by default.
    pub disabled: bool,

    #[serde(default = "default::keycloak_url")]
    pub url: String,
    #[serde(default = "default::realm")]
    pub realm: String,
    #[serde(default = "default::keycloak_admin_user")]
    pub admin_user: String,
    #[serde(default = "default::keycloak_admin_password")]
    pub admin_password: String,

    #[serde(default = "default::keycloak_realm_user")]
    pub realm_user: String,
    #[serde(default = "default::keycloak_realm_password")]
    pub realm_password: String,
}

impl Default for Keycloak {
    fn default() -> Self {
        Self {
            disabled: false,
            url: default::keycloak_url(),
            realm: default::realm(),
            admin_user: default::keycloak_admin_user(),
            admin_password: default::keycloak_admin_password(),
            realm_user: default::keycloak_realm_user(),
            realm_password: default::keycloak_realm_password(),
        }
    }
}

mod default {

    pub fn keycloak_url() -> String {
        "http://localhost:8081".into()
    }

    pub fn realm() -> String {
        "doppelgaenger".into()
    }

    pub fn keycloak_admin_user() -> String {
        "admin".into()
    }

    pub fn keycloak_admin_password() -> String {
        "admin123456".into()
    }

    pub fn keycloak_realm_user() -> String {
        "admin".into()
    }

    pub fn keycloak_realm_password() -> String {
        "admin123456".into()
    }
}

trait IgnoreConflict {
    type Output;
    fn ignore_conflict<S1, S2>(self, r#type: S1, name: S2) -> Self
    where
        S1: Display,
        S2: Display;
}

impl<T> IgnoreConflict for Result<T, KeycloakError>
where
    T: Default,
{
    type Output = T;

    fn ignore_conflict<S1, S2>(self, r#type: S1, name: S2) -> Self
    where
        S1: Display,
        S2: Display,
    {
        match self {
            Ok(result) => Ok(result),
            Err(KeycloakError::HttpFailure { status: 409, .. }) => {
                log::trace!("The {type} '{name}' already exists");
                Ok(T::default())
            }
            Err(err) => {
                log::warn!("Error creating '{name}' {type}: {err}");
                Err(err)
            }
        }
    }
}

pub async fn configure_keycloak(config: &Keycloak) -> anyhow::Result<()> {
    if config.disabled {
        return Ok(());
    }

    log::info!("Configuring keycloak... ");

    let url = &config.url;
    let user = config.admin_user.clone();
    let password = config.admin_password.clone();
    let client = reqwest::Client::new();
    let admin_token = keycloak::KeycloakAdminToken::acquire(url, &user, &password, &client).await?;
    let admin = keycloak::KeycloakAdmin::new(url, admin_token, client);

    // configure the realm
    let r = keycloak::types::RealmRepresentation {
        realm: Some(config.realm.clone()),
        enabled: Some(true),
        ..Default::default()
    };

    admin
        .post(r)
        .await
        .ignore_conflict("realm", "doppelgaenger")?;

    let mut mapper_config = HashMap::new();
    mapper_config.insert("included.client.audience".into(), "api".into());
    mapper_config.insert("id.token.claim".into(), "false".into());
    mapper_config.insert("access.token.claim".into(), "true".into());
    let mappers = vec![keycloak::types::ProtocolMapperRepresentation {
        id: None,
        name: Some("add-audience".to_string()),
        protocol: Some("openid-connect".to_string()),
        protocol_mapper: Some("oidc-audience-mapper".to_string()),
        config: Some(mapper_config),
    }];

    // Configure oauth account
    let mut c: keycloak::types::ClientRepresentation = Default::default();
    c.client_id.replace("api".to_string());
    c.enabled.replace(true);
    c.implicit_flow_enabled.replace(true);
    c.standard_flow_enabled.replace(true);
    c.direct_access_grants_enabled.replace(true);
    c.service_accounts_enabled.replace(false);
    c.full_scope_allowed.replace(true);
    c.root_url.replace("".to_string());
    c.redirect_uris.replace(vec!["*".to_string()]);
    c.web_origins.replace(vec!["*".to_string()]);
    c.client_authenticator_type
        .replace("client-secret".to_string());
    c.public_client.replace(true);
    c.secret.take();
    c.protocol_mappers.replace(mappers);

    admin
        .realm_clients_post(&config.realm, c)
        .await
        .ignore_conflict("client", "api")?;

    // Configure service account
    let mut c: keycloak::types::ClientRepresentation = Default::default();
    c.client_id.replace("services".to_string());
    c.implicit_flow_enabled.replace(false);
    c.standard_flow_enabled.replace(false);
    c.direct_access_grants_enabled.replace(false);
    c.service_accounts_enabled.replace(true);
    c.full_scope_allowed.replace(true);
    c.enabled.replace(true);
    c.client_authenticator_type
        .replace("client-secret".to_string());
    c.public_client.replace(false);
    c.secret.replace(SERVICE_CLIENT_SECRET.to_string());

    let mut mapper_config: HashMap<String, serde_json::value::Value> = HashMap::new();
    mapper_config.insert("included.client.audience".into(), "services".into());
    mapper_config.insert("id.token.claim".into(), "false".into());
    mapper_config.insert("access.token.claim".into(), "true".into());
    let mappers = vec![keycloak::types::ProtocolMapperRepresentation {
        id: None,
        name: Some("add-audience".to_string()),
        protocol: Some("openid-connect".to_string()),
        protocol_mapper: Some("oidc-audience-mapper".to_string()),
        config: Some(mapper_config),
    }];
    c.protocol_mappers.replace(mappers);

    admin
        .realm_clients_post(&config.realm, c)
        .await
        .ignore_conflict("client", "services")?;

    // Configure roles
    let mut admin_role = keycloak::types::RoleRepresentation::default();
    admin_role.name.replace("doppelgaenger-admin".to_string());
    admin
        .realm_roles_post(&config.realm, admin_role.clone())
        .await
        .ignore_conflict("role", "doppelgaenger-admin")?;

    let mut user_role = keycloak::types::RoleRepresentation::default();
    user_role.name.replace("doppelgaenger-user".to_string());
    admin
        .realm_roles_post(&config.realm, user_role.clone())
        .await
        .ignore_conflict("role", "doppelgaenger-user")?;

    // Read back
    let user_role = admin
        .realm_roles_with_role_name_get(&config.realm, "doppelgaenger-user")
        .await;
    let admin_role = admin
        .realm_roles_with_role_name_get(&config.realm, "doppelgaenger-admin")
        .await;

    match (user_role, admin_role) {
        (Ok(user_role), Ok(admin_role)) => {
            // Add to default roles if not present
            if let Err(e) = admin
                .realm_roles_with_role_name_composites_post(
                    &config.realm,
                    &format!("default-roles-{}", config.realm),
                    vec![admin_role, user_role],
                )
                .await
            {
                anyhow::bail!("Error associating roles with default: {:?}", e);
            }
        }
        _ => {
            anyhow::bail!("Error retrieving 'doppelgaenger-user' and 'doppelgaenger-admin' roles");
        }
    }

    // configure the admin user

    let u = keycloak::types::UserRepresentation {
        username: Some(config.admin_user.clone()),
        enabled: Some(true),
        credentials: Some(vec![CredentialRepresentation {
            type_: Some("password".into()),
            value: Some(config.admin_password.clone()),
            temporary: Some(false),
            ..Default::default()
        }]),
        ..Default::default()
    };

    admin
        .realm_users_post(&config.realm, u)
        .await
        .ignore_conflict("user", &config.admin_user)?;

    // done

    Ok(())
}

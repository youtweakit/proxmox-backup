use anyhow::Error;
use ::serde::{Deserialize, Serialize};

use proxmox_router::{Permission, Router, RpcEnvironment};
use proxmox_schema::api;

use pbs_api_types::{NODE_SCHEMA, PRIV_SYS_AUDIT, PRIV_SYS_MODIFY};

use crate::api2::node::apt::update_apt_proxy_config;
use crate::config::node::{NodeConfig, NodeConfigUpdater};

pub const ROUTER: Router = Router::new()
    .get(&API_METHOD_GET_NODE_CONFIG)
    .put(&API_METHOD_UPDATE_NODE_CONFIG);

#[api(
    input: {
        properties: {
            node: { schema: NODE_SCHEMA },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_AUDIT, false),
    },
    returns: {
        type: NodeConfig,
    },
)]
/// Get the node configuration
pub fn get_node_config(mut rpcenv: &mut dyn RpcEnvironment) -> Result<NodeConfig, Error> {
    let (config, digest) = crate::config::node::config()?;
    rpcenv["digest"] = proxmox::tools::digest_to_hex(&digest).into();
    Ok(config)
}

#[api()]
#[derive(Serialize, Deserialize)]
#[serde(rename_all="kebab-case")]
#[allow(non_camel_case_types)]
/// Deletable property name
pub enum DeletableProperty {
    /// Delete the acme property.
    acme,
    /// Delete the acmedomain0 property.
    acmedomain0,
    /// Delete the acmedomain1 property.
    acmedomain1,
    /// Delete the acmedomain2 property.
    acmedomain2,
    /// Delete the acmedomain3 property.
    acmedomain3,
    /// Delete the acmedomain4 property.
    acmedomain4,
    /// Delete the http-proxy property.
    http_proxy,
}

#[api(
    input: {
        properties: {
            node: { schema: NODE_SCHEMA },
            digest: {
                description: "Digest to protect against concurrent updates",
                optional: true,
            },
            update: {
                type: NodeConfigUpdater,
                flatten: true,
            },
            delete: {
                description: "List of properties to delete.",
                type: Array,
                optional: true,
                items: {
                    type: DeletableProperty,
                }
            },
        },
    },
    access: {
        permission: &Permission::Privilege(&["system"], PRIV_SYS_MODIFY, false),
    },
    protected: true,
)]
/// Update the node configuration
pub fn update_node_config(
    // node: String, // not used
    update: NodeConfigUpdater,
    delete: Option<Vec<DeletableProperty>>,
    digest: Option<String>,
) -> Result<(), Error> {
    let _lock = crate::config::node::lock()?;
    let (mut config, expected_digest) = crate::config::node::config()?;
    if let Some(digest) = digest {
        // FIXME: GUI doesn't handle our non-inlined digest part here properly...
        if !digest.is_empty() {
            let digest = proxmox::tools::hex_to_digest(&digest)?;
            crate::tools::detect_modified_configuration_file(&digest, &expected_digest)?;
        }
    }

    if let Some(delete) = delete {
        for delete_prop in delete {
            match delete_prop {
                DeletableProperty::acme => { config.acme = None; },
                DeletableProperty::acmedomain0 => { config.acmedomain0 = None; },
                DeletableProperty::acmedomain1 => { config.acmedomain1 = None; },
                DeletableProperty::acmedomain2 => { config.acmedomain2 = None; },
                DeletableProperty::acmedomain3 => { config.acmedomain3 = None; },
                DeletableProperty::acmedomain4 => { config.acmedomain4 = None; },
                DeletableProperty::http_proxy => { config.http_proxy = None; },
            }
        }
    }

    if update.acme.is_some() { config.acme = update.acme; }
    if update.acmedomain0.is_some() { config.acmedomain0 = update.acmedomain0; }
    if update.acmedomain1.is_some() { config.acmedomain1 = update.acmedomain1; }
    if update.acmedomain2.is_some() { config.acmedomain2 = update.acmedomain2; }
    if update.acmedomain3.is_some() { config.acmedomain3 = update.acmedomain3; }
    if update.acmedomain4.is_some() { config.acmedomain4 = update.acmedomain4; }
    if update.http_proxy.is_some() { config.http_proxy = update.http_proxy; }

    crate::config::node::save_config(&config)?;

    update_apt_proxy_config(config.http_proxy().as_ref())?;

    Ok(())
}

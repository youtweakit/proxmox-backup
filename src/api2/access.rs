use anyhow::{bail, format_err, Error};

use serde_json::{json, Value};
use std::collections::HashMap;
use std::collections::HashSet;

use proxmox::api::{api, RpcEnvironment, Permission};
use proxmox::api::router::{Router, SubdirMap};
use proxmox::{sortable, identity};
use proxmox::{http_err, list_subdirs_api_method};

use crate::tools::ticket::{self, Empty, Ticket};
use crate::auth_helpers::*;
use crate::api2::types::*;
use crate::tools::{FileLogOptions, FileLogger};

use crate::config::acl as acl_config;
use crate::config::acl::{PRIVILEGES, PRIV_SYS_AUDIT, PRIV_PERMISSIONS_MODIFY};
use crate::config::cached_user_info::CachedUserInfo;

pub mod user;
pub mod domain;
pub mod acl;
pub mod role;

/// returns Ok(true) if a ticket has to be created
/// and Ok(false) if not
fn authenticate_user(
    userid: &Userid,
    password: &str,
    path: Option<String>,
    privs: Option<String>,
    port: Option<u16>,
) -> Result<bool, Error> {
    let user_info = CachedUserInfo::new()?;

    let auth_id = Authid::from(userid.clone());
    if !user_info.is_active_auth_id(&auth_id) {
        bail!("user account disabled or expired.");
    }

    if password.starts_with("PBS:") {
        if let Ok(ticket_userid) = Ticket::<Userid>::parse(password)
            .and_then(|ticket| ticket.verify(public_auth_key(), "PBS", None))
        {
            if *userid == ticket_userid {
                return Ok(true);
            }
            bail!("ticket login failed - wrong userid");
        }
    } else if password.starts_with("PBSTERM:") {
        if path.is_none() || privs.is_none() || port.is_none() {
            bail!("cannot check termnal ticket without path, priv and port");
        }

        let path = path.ok_or_else(|| format_err!("missing path for termproxy ticket"))?;
        let privilege_name = privs
            .ok_or_else(|| format_err!("missing privilege name for termproxy ticket"))?;
        let port = port.ok_or_else(|| format_err!("missing port for termproxy ticket"))?;

        if let Ok(Empty) = Ticket::parse(password)
            .and_then(|ticket| ticket.verify(
                public_auth_key(),
                ticket::TERM_PREFIX,
                Some(&ticket::term_aad(userid, &path, port)),
            ))
        {
            for (name, privilege) in PRIVILEGES {
                if *name == privilege_name {
                    let mut path_vec = Vec::new();
                    for part in path.split('/') {
                        if part != "" {
                            path_vec.push(part);
                        }
                    }
                    user_info.check_privs(&auth_id, &path_vec, *privilege, false)?;
                    return Ok(false);
                }
            }

            bail!("No such privilege");
        }
    }

    let _ = crate::auth::authenticate_user(userid, password)?;
    Ok(true)
}

#[api(
    input: {
        properties: {
            username: {
                type: Userid,
            },
            password: {
                schema: PASSWORD_SCHEMA,
            },
            path: {
                type: String,
                description: "Path for verifying terminal tickets.",
                optional: true,
            },
            privs: {
                type: String,
                description: "Privilege for verifying terminal tickets.",
                optional: true,
            },
            port: {
                type: Integer,
                description: "Port for verifying terminal tickets.",
                optional: true,
            },
        },
    },
    returns: {
        properties: {
            username: {
                type: String,
                description: "User name.",
            },
            ticket: {
                type: String,
                description: "Auth ticket.",
            },
            CSRFPreventionToken: {
                type: String,
                description: "Cross Site Request Forgery Prevention Token.",
            },
        },
    },
    protected: true,
    access: {
        permission: &Permission::World,
    },
)]
/// Create or verify authentication ticket.
///
/// Returns: An authentication ticket with additional infos.
fn create_ticket(
    username: Userid,
    password: String,
    path: Option<String>,
    privs: Option<String>,
    port: Option<u16>,
    rpcenv: &mut dyn RpcEnvironment,
) -> Result<Value, Error> {
    let logger_options = FileLogOptions {
        append: true,
        prefix_time: true,
        ..Default::default()
    };
    let mut auth_log = FileLogger::new("/var/log/proxmox-backup/api/auth.log", logger_options)?;

    match authenticate_user(&username, &password, path, privs, port) {
        Ok(true) => {
            let ticket = Ticket::new("PBS", &username)?.sign(private_auth_key(), None)?;

            let token = assemble_csrf_prevention_token(csrf_secret(), &username);

            auth_log.log(format!("successful auth for user '{}'", username));

            Ok(json!({
                "username": username,
                "ticket": ticket,
                "CSRFPreventionToken": token,
            }))
        }
        Ok(false) => Ok(json!({
            "username": username,
        })),
        Err(err) => {
            let client_ip = match rpcenv.get_client_ip().map(|addr| addr.ip()) {
                Some(ip) => format!("{}", ip),
                None => "unknown".into(),
            };

            let msg = format!(
                "authentication failure; rhost={} user={} msg={}",
                client_ip,
                username,
                err.to_string()
            );
            auth_log.log(&msg);
            log::error!("{}", msg);

            Err(http_err!(UNAUTHORIZED, "permission check failed."))
        }
    }
}

#[api(
    input: {
        properties: {
            userid: {
                type: Userid,
            },
            password: {
                schema: PASSWORD_SCHEMA,
            },
        },
    },
    access: {
        description: "Anybody is allowed to change there own password. In addition, users with 'Permissions:Modify' privilege may change any password.",
        permission: &Permission::Anybody,
    },

)]
/// Change user password
///
/// Each user is allowed to change his own password. Superuser
/// can change all passwords.
fn change_password(
    userid: Userid,
    password: String,
    rpcenv: &mut dyn RpcEnvironment,
) -> Result<Value, Error> {

    let current_user: Userid = rpcenv
        .get_auth_id()
        .ok_or_else(|| format_err!("unknown user"))?
        .parse()?;
    let current_auth = Authid::from(current_user.clone());

    let mut allowed = userid == current_user;

    if userid == "root@pam" { allowed = true; }

    if !allowed {
        let user_info = CachedUserInfo::new()?;
        let privs = user_info.lookup_privs(&current_auth, &[]);
        if (privs & PRIV_PERMISSIONS_MODIFY) != 0 { allowed = true; }
    }

    if !allowed {
        bail!("you are not authorized to change the password.");
    }

    let authenticator = crate::auth::lookup_authenticator(userid.realm())?;
    authenticator.store_password(userid.name(), &password)?;

    Ok(Value::Null)
}

#[api(
    input: {
        properties: {
            auth_id: {
                type: Authid,
                optional: true,
            },
            path: {
                schema: ACL_PATH_SCHEMA,
                optional: true,
            },
        },
    },
    access: {
        permission: &Permission::Anybody,
        description: "Requires Sys.Audit on '/access', limited to own privileges otherwise.",
    },
    returns: {
        description: "Map of ACL path to Map of privilege to propagate bit",
        type: Object,
        properties: {},
        additional_properties: true,
    },
)]
/// List permissions of given or currently authenticated user / API token.
///
/// Optionally limited to specific path.
pub fn list_permissions(
    auth_id: Option<Authid>,
    path: Option<String>,
    rpcenv: &dyn RpcEnvironment,
) -> Result<HashMap<String, HashMap<String, bool>>, Error> {
    let current_auth_id: Authid = rpcenv.get_auth_id().unwrap().parse()?;

    let user_info = CachedUserInfo::new()?;
    let user_privs = user_info.lookup_privs(&current_auth_id, &["access"]);

    let auth_id = if user_privs & PRIV_SYS_AUDIT == 0 {
        match auth_id {
            Some(auth_id) => {
                if auth_id == current_auth_id {
                    auth_id
                } else if auth_id.is_token()
                    && !current_auth_id.is_token()
                    && auth_id.user() == current_auth_id.user() {
                    auth_id
                } else {
                    bail!("not allowed to list permissions of {}", auth_id);
                }
            },
            None => current_auth_id,
        }
    } else {
        match auth_id {
            Some(auth_id) => auth_id,
            None => current_auth_id,
        }
    };


    fn populate_acl_paths(
        mut paths: HashSet<String>,
        node: acl_config::AclTreeNode,
        path: &str
    ) -> HashSet<String> {
        for (sub_path, child_node) in node.children {
            let sub_path = format!("{}/{}", path, &sub_path);
            paths = populate_acl_paths(paths, child_node, &sub_path);
            paths.insert(sub_path);
        }
        paths
    }

    let paths = match path {
        Some(path) => {
            let mut paths = HashSet::new();
            paths.insert(path);
            paths
        },
        None => {
            let mut paths = HashSet::new();

            let (acl_tree, _) = acl_config::config()?;
            paths = populate_acl_paths(paths, acl_tree.root, "");

            // default paths, returned even if no ACL exists
            paths.insert("/".to_string());
            paths.insert("/access".to_string());
            paths.insert("/datastore".to_string());
            paths.insert("/remote".to_string());
            paths.insert("/system".to_string());

            paths
        },
    };

    let map = paths
        .into_iter()
        .fold(HashMap::new(), |mut map: HashMap<String, HashMap<String, bool>>, path: String| {
            let split_path = acl_config::split_acl_path(path.as_str());
            let (privs, propagated_privs) = user_info.lookup_privs_details(&auth_id, &split_path);

            match privs {
                0 => map, // Don't leak ACL paths where we don't have any privileges
                _ => {
                    let priv_map = PRIVILEGES
                        .iter()
                        .fold(HashMap::new(), |mut priv_map, (name, value)| {
                            if value & privs != 0 {
                                priv_map.insert(name.to_string(), value & propagated_privs != 0);
                            }
                            priv_map
                        });

                    map.insert(path, priv_map);
                    map
                },
            }});

    Ok(map)
}

#[sortable]
const SUBDIRS: SubdirMap = &sorted!([
    ("acl", &acl::ROUTER),
    (
        "password", &Router::new()
            .put(&API_METHOD_CHANGE_PASSWORD)
    ),
    (
        "permissions", &Router::new()
            .get(&API_METHOD_LIST_PERMISSIONS)
    ),
    (
        "ticket", &Router::new()
            .post(&API_METHOD_CREATE_TICKET)
    ),
    ("domains", &domain::ROUTER),
    ("roles", &role::ROUTER),
    ("users", &user::ROUTER),
]);

pub const ROUTER: Router = Router::new()
    .get(&list_subdirs_api_method!(SUBDIRS))
    .subdirs(SUBDIRS);

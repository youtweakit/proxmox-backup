use failure::*;

use proxmox::tools::fs::{file_get_contents, file_set_contents};
use proxmox::tools::*; // required to use IPRE!() macro ???

use crate::api2::*;
use crate::api_schema::*;
//use crate::api_schema::router::*;
use crate::api2::types::*;

use lazy_static::lazy_static;

use std::sync::{Arc, Mutex};
use openssl::sha;
use regex::Regex;

use serde_json::{json, Value};

static RESOLV_CONF_FN: &str = "/etc/resolv.conf";

fn read_etc_resolv_conf() -> Result<Value, Error> {

    let mut result = json!({});

    let mut nscount = 0;

    let raw = file_get_contents(RESOLV_CONF_FN)?;

    result["digest"] = Value::from(proxmox::tools::digest_to_hex(&sha::sha256(&raw)));

    let data = String::from_utf8(raw)?;

    lazy_static! {
        static ref DOMAIN_REGEX: Regex = Regex::new(r"^\s*(?:search|domain)\s+(\S+)\s*").unwrap();
        static ref SERVER_REGEX: Regex = Regex::new(
            concat!(r"^\s*nameserver\s+(", IPRE!(),  r")\s*")).unwrap();
    }

    for line in data.lines() {

        if let Some(caps) = DOMAIN_REGEX.captures(&line) {
            result["search"] = Value::from(&caps[1]);
        } else if let Some(caps) = SERVER_REGEX.captures(&line) {
            nscount += 1;
            if nscount > 3 { continue };
            let nameserver = &caps[1];
            let id = format!("dns{}", nscount);
            result[id] = Value::from(nameserver);
        }
    }

    Ok(result)
}

fn update_dns(
    param: Value,
    _info: &ApiMethod,
    _rpcenv: &mut dyn RpcEnvironment,
) -> Result<Value, Error> {

    lazy_static! {
        static ref MUTEX: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    }

    let _guard = MUTEX.lock();

    let search = crate::tools::required_string_param(&param, "search")?;

    let raw = file_get_contents(RESOLV_CONF_FN)?;
    let old_digest = proxmox::tools::digest_to_hex(&sha::sha256(&raw));

    if let Some(digest) = param["digest"].as_str() {
        crate::tools::assert_if_modified(&old_digest, &digest)?;
    }

    let old_data = String::from_utf8(raw)?;

    let mut data = format!("search {}\n", search);

    for opt in &["dns1", "dns2", "dns3"] {
        if let Some(server) = param[opt].as_str() {
            data.push_str(&format!("nameserver {}\n", server));
        }
    }

    // append other data
    lazy_static! {
        static ref SKIP_REGEX: Regex = Regex::new(r"^(search|domain|nameserver)\s+").unwrap();
    }
    for line in old_data.lines() {
        if SKIP_REGEX.is_match(line) { continue; }
        data.push_str(line);
        data.push('\n');
    }

    file_set_contents(RESOLV_CONF_FN, data.as_bytes(), None)?;

    Ok(Value::Null)
}

fn get_dns(
    _param: Value,
    _info: &ApiMethod,
    _rpcenv: &mut dyn RpcEnvironment,
) -> Result<Value, Error> {

    read_etc_resolv_conf()
}

pub fn router() -> Router {

    let route = Router::new()
        .get(
            ApiMethod::new(
                get_dns,
                ObjectSchema::new("Read DNS settings.")
                    .required("node", NODE_SCHEMA.clone())
            ).returns(
                ObjectSchema::new("Returns DNS server IPs and sreach domain.")
                    .required("digest", PVE_CONFIG_DIGEST_SCHEMA.clone())
                    .optional("search", SEARCH_DOMAIN_SCHEMA.clone())
                    .optional("dns1", FIRST_DNS_SERVER_SCHEMA.clone())
                    .optional("dns2", SECOND_DNS_SERVER_SCHEMA.clone())
                    .optional("dns3", THIRD_DNS_SERVER_SCHEMA.clone())
            )
        )
        .put(
            ApiMethod::new(
                update_dns,
                ObjectSchema::new("Returns DNS server IPs and sreach domain.")
                    .required("node", NODE_SCHEMA.clone())
                    .required("search", SEARCH_DOMAIN_SCHEMA.clone())
                    .optional("dns1", FIRST_DNS_SERVER_SCHEMA.clone())
                    .optional("dns2", SECOND_DNS_SERVER_SCHEMA.clone())
                    .optional("dns3", THIRD_DNS_SERVER_SCHEMA.clone())
                    .optional("digest", PVE_CONFIG_DIGEST_SCHEMA.clone())
            ).protected(true)
        );

    route
}

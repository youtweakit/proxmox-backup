use failure::*;


use crate::tools;
use crate::api2::*;
//use crate::api::schema::*;
//use crate::api::router::*;

use lazy_static::lazy_static;

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use openssl::sha;

use serde_json::{json, Value};

static RESOLV_CONF_FN: &str = "/etc/resolv.conf";

fn read_etc_resolv_conf() -> Result<Value, Error> {

    let mut result = json!({});

    let mut nscount = 0;

    let raw = tools::file_get_contents(RESOLV_CONF_FN)?;

    result["digest"] = Value::from(tools::digest_to_hex(&sha::sha256(&raw)));

    let data = String::from_utf8(raw)?;

    lazy_static! {
        static ref DOMAIN_REGEX: regex::Regex = regex::Regex::new(r"^\s*(?:search|domain)\s+(\S+)\s*").unwrap();
        static ref SERVER_REGEX: regex::Regex = regex::Regex::new(
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

fn update_dns(param: Value, _info: &ApiMethod) -> Result<Value, Error> {

    let search = tools::required_string_param(&param, "search")?;

    let mut data = format!("search {}\n", search);

    for opt in &["dns1", "dns2", "dns3"] {
        if let Some(server) = param[opt].as_str() {
            data.push_str(&format!("nameserver {}\n", server));
        }
    }

    tools::file_set_contents(RESOLV_CONF_FN, data.as_bytes(), None)?;

    Ok(Value::Null)
}

fn get_dns(_param: Value, _info: &ApiMethod) -> Result<Value, Error> {

    read_etc_resolv_conf()
}

lazy_static! {
    pub static ref SEARCH_DOMAIN_SCHEMA: Arc<Schema> =
        StringSchema::new("Search domain for host-name lookup.").into();

    pub static ref FIRST_DNS_SERVER_SCHEMA: Arc<Schema> =
        StringSchema::new("First name server IP address.")
        .format(IP_FORMAT.clone()).into();

    pub static ref SECOND_DNS_SERVER_SCHEMA: Arc<Schema> =
        StringSchema::new("Second name server IP address.")
        .format(IP_FORMAT.clone()).into();

    pub static ref THIRD_DNS_SERVER_SCHEMA: Arc<Schema> =
        StringSchema::new("Third name server IP address.")
        .format(IP_FORMAT.clone()).into();
}

pub fn router() -> Router {

    let route = Router::new()
        .get(
            ApiMethod::new(
                get_dns,
                ObjectSchema::new("Read DNS settings.")
            ).returns(
                ObjectSchema::new("Returns DNS server IPs and sreach domain.")
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
                    .required("search", SEARCH_DOMAIN_SCHEMA.clone())
                    .optional("dns1", FIRST_DNS_SERVER_SCHEMA.clone())
                    .optional("dns2", SECOND_DNS_SERVER_SCHEMA.clone())
                    .optional("dns3", THIRD_DNS_SERVER_SCHEMA.clone())
             )
        );

    route
}

#[macro_use]
extern crate apitest;

use failure::*;

use std::collections::HashMap;


use apitest::json_schema::*;
use apitest::api_info::*;

use serde_derive::{Serialize, Deserialize};
use serde_json::{json, Value};

use url::form_urlencoded;

use hyper::{Method, Body, Request, Response, Server, StatusCode};
use hyper::rt::Future;
use hyper::service::service_fn_ok;




#[derive(Serialize, Deserialize)]
struct Myparam {
    test: bool,
}

fn test_api_handler(param: Value) -> Result<Value, Error> {
    println!("This is a test {}", param);

   // let force: Option<bool> = Some(false);

    //if let Some(force) = param.force {
    //}

    let _force =  param["force"].as_bool()
        .ok_or_else(|| format_err!("missing parameter 'force'"))?;

    if let Some(_force) = param["force"].as_bool() {
    }

    let _tmp: Myparam = serde_json::from_value(param)?;


    Ok(json!(null))
}

static TEST_API_METHOD: ApiMethod = ApiMethod {
    description: "This is a simple test.",
    properties: &propertymap!{
        force => &Boolean!{
            optional => Some(true),
            description => "Test for boolean options."
        }
    },
    returns: &Jss::Null,
    handler: test_api_handler,
};


methodinfo!{
    API3_TEST,
}

methodinfo!{
    API3_NODES,
    get => &TEST_API_METHOD
}

methodinfo!{
    API_ROOT,
    get => &TEST_API_METHOD,
    subdirs => &subdirmap!{
        test => &API3_TEST,
        nodes => &API3_NODES
    }
}

macro_rules! http_error {
    ($status:ident, $msg:expr) => {{
        let mut resp = Response::new(Body::from($msg));
        *resp.status_mut() = StatusCode::$status;
        return resp;
    }}
}

fn parse_query(query: &str) -> Value {

    println!("PARSE QUERY {}", query);

    // fixme: what about repeated parameters (arrays?)
    let mut raw_param = HashMap::new();
    for (k, v) in form_urlencoded::parse(query.as_bytes()) {
        println!("QUERY PARAM {} value {}", k, v);
        raw_param.insert(k, v);
    }
    println!("QUERY HASH {:?}", raw_param);

    return json!(null);
}

fn handle_request(req: Request<Body>) -> Response<Body> {

    let method = req.method();
    let path = req.uri().path();
    let query = req.uri().query();
    let components: Vec<&str> = path.split('/').filter(|x| !x.is_empty()).collect();
    let comp_len = components.len();

    println!("REQUEST {} {}", method, path);
    println!("COMPO {:?}", components);

    if comp_len >= 1 && components[0] == "api3" {
        println!("GOT API REQUEST");
        if comp_len >= 2 {
            let format = components[1];
            if format != "json" {
                http_error!(NOT_FOUND, format!("Unsupported format '{}'\n", format))
            }

            if let Some(info) = API_ROOT.find_method(&components[2..]) {
                println!("FOUND INFO");
                let api_method_opt = match method {
                    &Method::GET => info.get,
                    &Method::PUT => info.put,
                    &Method::POST => info.post,
                    &Method::DELETE => info.delete,
                    _ => None,
                };
                let api_method = match api_method_opt {
                    Some(m) => m,
                    _ => http_error!(NOT_FOUND, format!("No such method '{} {}'\n", method, path)),
                };

                // handle auth

                // extract param
                let param = match query {
                    Some(data) => parse_query(data),
                    None => json!({}),
                };

                match (api_method.handler)(param) {
                    Ok(res) => {
                        let json_str = res.to_string();
                        return Response::new(Body::from(json_str));
                    }
                    Err(err) => {
                        http_error!(NOT_FOUND, format!("Method returned error '{}'\n", err));
                    }
                }

            } else {
                http_error!(NOT_FOUND, format!("No such path '{}'\n", path));
            }
        }
    }

    Response::new(Body::from("RETURN WEB GUI\n"))
}

fn main() {
    println!("Fast Static Type Definitions 1");

    let addr = ([127, 0, 0, 1], 8007).into();

    let new_svc = || {
        // service_fn_ok converts our function into a `Service`
        service_fn_ok(handle_request)
    };

    let server = Server::bind(&addr)
        .serve(new_svc)
        .map_err(|e| eprintln!("server error: {}", e));

    // Run this server for... forever!
    hyper::rt::run(server);
}

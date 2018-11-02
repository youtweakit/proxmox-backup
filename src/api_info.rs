use failure::*;

use crate::json_schema::*;
use serde_json::{Value};

#[derive(Debug)]
pub struct ApiMethod<'a> {
    pub description: &'a str,
    pub properties: &'a PropertyMap<'a>,
    pub returns: &'a Jss<'a>,
    pub handler: fn(Value) -> Result<Value, Error>,
}

pub type SubdirMap<'a> = crate::static_map::StaticMap<'a, &'a str, &'a MethodInfo<'a>>;

#[macro_export]
macro_rules! subdirmap {
    ($($name:ident => $e:expr),*) => {{
        SubdirMap {
            entries: &[
                $( ( stringify!($name),  $e), )*
            ]
        }
    }}
}

#[derive(Debug)]
pub struct MethodInfo<'a> {
    pub get: Option<&'a ApiMethod<'a>>,
    pub put: Option<&'a ApiMethod<'a>>,
    pub post: Option<&'a ApiMethod<'a>>,
    pub delete: Option<&'a ApiMethod<'a>>,
    pub subdirs: Option<&'a SubdirMap<'a>>,
}

impl<'a> MethodInfo<'a> {

    pub fn find_method(&'a self, components: &[&str]) -> Option<&'a MethodInfo<'a>> {

        if components.len() == 0 { return Some(self); };

        let (dir, rest) = (components[0], &components[1..]);

        if let Some(ref dirmap) = self.subdirs {
            if let Some(info) = dirmap.get(&dir) {
                return info.find_method(rest);
            }
        }

        None
    }
}

pub static METHOD_INFO_DEFAULTS: MethodInfo = MethodInfo {
    get: None,
    put: None,
    post: None,
    delete: None,
    subdirs: None,
};

#[macro_export]
macro_rules! methodinfo {
    ($name:ident, $($option:ident => $e:expr),*) => {
        static $name: MethodInfo = MethodInfo {
            $( $option:  Some($e), )*
            ..METHOD_INFO_DEFAULTS
        };
    }
}

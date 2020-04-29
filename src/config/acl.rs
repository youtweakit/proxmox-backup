use std::io::Write;
use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet};
use std::path::{PathBuf, Path};
use std::sync::{Arc, RwLock};

use anyhow::{bail, Error};

use lazy_static::lazy_static;

use proxmox::tools::{fs::replace_file, fs::CreateOptions};

// define Privilege bitfield

pub const PRIV_SYS_AUDIT: u64                    = 1 << 0;
pub const PRIV_SYS_MODIFY: u64                   = 1 << 1;
pub const PRIV_SYS_POWER_MANAGEMENT: u64         = 1 << 2;

pub const PRIV_DATASTORE_AUDIT: u64              = 1 << 3;
pub const PRIV_DATASTORE_MODIFY: u64             = 1 << 4;
pub const PRIV_DATASTORE_READ: u64               = 1 << 5;

/// Datastore.Backup also requires backup ownership
pub const PRIV_DATASTORE_BACKUP: u64             = 1 << 6;
/// Datastore.Prune also requires backup ownership
pub const PRIV_DATASTORE_PRUNE: u64              = 1 << 7;

pub const PRIV_PERMISSIONS_MODIFY: u64           = 1 << 8;

pub const PRIV_REMOTE_AUDIT: u64                 = 1 << 9;
pub const PRIV_REMOTE_MODIFY: u64                = 1 << 10;
pub const PRIV_REMOTE_READ: u64                  = 1 << 11;
pub const PRIV_REMOTE_PRUNE: u64                 = 1 << 12;

pub const ROLE_ADMIN: u64 = std::u64::MAX;
pub const ROLE_NO_ACCESS: u64 = 0;

pub const ROLE_AUDIT: u64 =
PRIV_SYS_AUDIT |
PRIV_DATASTORE_AUDIT;

/// Datastore.Admin can do anything on the datastore.
pub const ROLE_DATASTORE_ADMIN: u64 =
PRIV_DATASTORE_AUDIT |
PRIV_DATASTORE_MODIFY |
PRIV_DATASTORE_READ |
PRIV_DATASTORE_BACKUP |
PRIV_DATASTORE_PRUNE;

/// Datastore.Reader can read datastore content an do restore
pub const ROLE_DATASTORE_READER: u64 =
PRIV_DATASTORE_AUDIT |
PRIV_DATASTORE_READ;

/// Datastore.Backup can do backup and restore, but no prune.
pub const ROLE_DATASTORE_BACKUP: u64 =
PRIV_DATASTORE_BACKUP;

/// Datastore.PowerUser can do backup, restore, and prune.
pub const ROLE_DATASTORE_POWERUSER: u64 =
PRIV_DATASTORE_PRUNE |
PRIV_DATASTORE_BACKUP;

/// Datastore.Audit can audit the datastore.
pub const ROLE_DATASTORE_AUDIT: u64 =
PRIV_DATASTORE_AUDIT;

/// Remote.Audit can audit the remote
pub const ROLE_REMOTE_AUDIT: u64 =
PRIV_REMOTE_AUDIT;

/// Remote.Admin can do anything on the remote.
pub const ROLE_REMOTE_ADMIN: u64 =
PRIV_REMOTE_AUDIT |
PRIV_REMOTE_MODIFY |
PRIV_REMOTE_READ |
PRIV_REMOTE_PRUNE;

/// Remote.SyncOperator can do read and prune on the remote.
pub const ROLE_REMOTE_SYNC_OPERATOR: u64 =
PRIV_REMOTE_AUDIT |
PRIV_REMOTE_READ |
PRIV_REMOTE_PRUNE;

pub const ROLE_NAME_NO_ACCESS: &str ="NoAccess";

lazy_static! {
    pub static ref ROLE_NAMES: HashMap<&'static str, (u64, &'static str)> = {
        let mut map = HashMap::new();

        map.insert("Admin", (
            ROLE_ADMIN,
            "Administrator",
        ));
        map.insert("Audit", (
            ROLE_AUDIT,
            "Auditor",
        ));
        map.insert(ROLE_NAME_NO_ACCESS, (
            ROLE_NO_ACCESS,
            "Disable access",
        ));

        map.insert("Datastore.Admin", (
            ROLE_DATASTORE_ADMIN,
            "Datastore Administrator",
        ));
        map.insert("Datastore.Reader", (
            ROLE_DATASTORE_READER,
            "Datastore Reader (inspect datastore content and do restores)",
        ));
        map.insert("Datastore.Backup", (
            ROLE_DATASTORE_BACKUP,
            "Datastore Backup (backup and restore owned backups)",
        ));
        map.insert("Datastore.PowerUser", (
            ROLE_DATASTORE_POWERUSER,
            "Datastore PowerUser (backup, restore and prune owned backup)",
        ));
        map.insert("Datastore.Audit", (
            ROLE_DATASTORE_AUDIT,
            "Datastore Auditor",
        ));

        map.insert("Remote.Audit", (
            ROLE_REMOTE_AUDIT,
            "Remote Auditor",
        ));
        map.insert("Remote.Admin", (
            ROLE_REMOTE_ADMIN,
            "Remote Administrator",
        ));
        map.insert("Remote.SyncOperator", (
            ROLE_REMOTE_SYNC_OPERATOR,
            "Syncronisation Opertator",
        ));

        map
    };
}

pub fn split_acl_path(path: &str) -> Vec<&str> {

    let items = path.split('/');

    let mut components = vec![];

    for name in items {
        if name.is_empty() { continue; }
        components.push(name);
    }

    components
}

pub struct AclTree {
    pub root: AclTreeNode,
}

pub struct AclTreeNode {
    pub users: HashMap<String, HashMap<String, bool>>,
    pub groups: HashMap<String, HashMap<String, bool>>,
    pub children: BTreeMap<String, AclTreeNode>,
}

impl AclTreeNode {

    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            groups: HashMap::new(),
            children: BTreeMap::new(),
        }
    }

    pub fn extract_roles(&self, user: &str, all: bool) -> HashSet<String> {
        let user_roles = self.extract_user_roles(user, all);
        if !user_roles.is_empty() {
            // user privs always override group privs
            return user_roles
        };

        self.extract_group_roles(user, all)
    }

    pub fn extract_user_roles(&self, user: &str, all: bool) -> HashSet<String> {

        let mut set = HashSet::new();

        let roles = match self.users.get(user) {
            Some(m) => m,
            None => return set,
        };

        for (role, propagate) in roles {
            if *propagate || all {
                if role == ROLE_NAME_NO_ACCESS {
                    // return a set with a single role 'NoAccess'
                    let mut set = HashSet::new();
                    set.insert(role.to_string());
                    return set;
                }
                set.insert(role.to_string());
            }
        }

        set
    }

    pub fn extract_group_roles(&self, _user: &str, all: bool) -> HashSet<String> {

        let mut set = HashSet::new();

        for (_group, roles) in &self.groups {
            let is_member = false; // fixme: check if user is member of the group
            if !is_member { continue; }

            for (role, propagate) in roles {
                if *propagate || all {
                    if role == ROLE_NAME_NO_ACCESS {
                        // return a set with a single role 'NoAccess'
                        let mut set = HashSet::new();
                        set.insert(role.to_string());
                        return set;
                    }
                    set.insert(role.to_string());
                }
            }
        }

        set
    }

    pub fn delete_group_role(&mut self, group: &str, role: &str) {
        let roles = match self.groups.get_mut(group) {
            Some(r) => r,
            None => return,
        };
        roles.remove(role);
    }

    pub fn delete_user_role(&mut self, userid: &str, role: &str) {
        let roles = match self.users.get_mut(userid) {
            Some(r) => r,
            None => return,
        };
        roles.remove(role);
    }

    pub fn insert_group_role(&mut self, group: String, role: String, propagate: bool) {
        let map = self.groups.entry(group).or_insert_with(|| HashMap::new());
        if role == ROLE_NAME_NO_ACCESS {
            map.clear();
            map.insert(role, propagate);
        } else {
            map.remove(ROLE_NAME_NO_ACCESS);
            map.insert(role, propagate);
        }
    }

    pub fn insert_user_role(&mut self, user: String, role: String, propagate: bool) {
        let map = self.users.entry(user).or_insert_with(|| HashMap::new());
        if role == ROLE_NAME_NO_ACCESS {
            map.clear();
            map.insert(role, propagate);
        } else {
            map.remove(ROLE_NAME_NO_ACCESS);
            map.insert(role, propagate);
        }
    }
}

impl AclTree {

    pub fn new() -> Self {
        Self { root: AclTreeNode::new() }
    }

    fn get_node(&mut self, path: &[&str]) -> Option<&mut AclTreeNode> {
        let mut node = &mut self.root;
        for comp in path {
            node = match node.children.get_mut(*comp) {
                Some(n) => n,
                None => return None,
            };
        }
        Some(node)
    }

    fn get_or_insert_node(&mut self, path: &[&str]) -> &mut AclTreeNode {
        let mut node = &mut self.root;
        for comp in path {
            node = node.children.entry(String::from(*comp))
                .or_insert_with(|| AclTreeNode::new());
        }
        node
    }

    pub fn delete_group_role(&mut self, path: &str, group: &str, role: &str) {
        let path = split_acl_path(path);
        let node = match self.get_node(&path) {
            Some(n) => n,
            None => return,
        };
        node.delete_group_role(group, role);
    }

    pub fn delete_user_role(&mut self, path: &str, userid: &str, role: &str) {
        let path = split_acl_path(path);
        let node = match self.get_node(&path) {
            Some(n) => n,
            None => return,
        };
        node.delete_user_role(userid, role);
    }

    pub fn insert_group_role(&mut self, path: &str, group: &str, role: &str, propagate: bool) {
        let path = split_acl_path(path);
        let node = self.get_or_insert_node(&path);
        node.insert_group_role(group.to_string(), role.to_string(), propagate);
    }

    pub fn insert_user_role(&mut self, path: &str, user: &str, role: &str, propagate: bool) {
        let path = split_acl_path(path);
        let node = self.get_or_insert_node(&path);
        node.insert_user_role(user.to_string(), role.to_string(), propagate);
    }

    fn write_node_config(
        node: &AclTreeNode,
        path: &str,
        w: &mut dyn Write,
    ) -> Result<(), Error> {

        let mut role_ug_map0 = HashMap::new();
        let mut role_ug_map1 = HashMap::new();

        for (user, roles) in &node.users {
            // no need to save, because root is always 'Administrator'
            if user == "root@pam" { continue; }
            for (role, propagate) in roles {
                let role = role.as_str();
                let user = user.to_string();
                if *propagate {
                    role_ug_map1.entry(role).or_insert_with(|| BTreeSet::new())
                        .insert(user);
                } else {
                    role_ug_map0.entry(role).or_insert_with(|| BTreeSet::new())
                        .insert(user);
                }
            }
        }

        for (group, roles) in &node.groups {
            for (role, propagate) in roles {
                let group = format!("@{}", group);
                if *propagate {
                    role_ug_map1.entry(role).or_insert_with(|| BTreeSet::new())
                        .insert(group);
                } else {
                    role_ug_map0.entry(role).or_insert_with(|| BTreeSet::new())
                        .insert(group);
                }
            }
        }

        fn group_by_property_list(
            item_property_map: &HashMap<&str, BTreeSet<String>>,
        ) -> BTreeMap<String, BTreeSet<String>> {
            let mut result_map = BTreeMap::new();
            for (item, property_map) in item_property_map {
                let item_list = property_map.iter().fold(String::new(), |mut acc, v| {
                    if !acc.is_empty() { acc.push(','); }
                    acc.push_str(v);
                    acc
                });
                result_map.entry(item_list).or_insert_with(|| BTreeSet::new())
                    .insert(item.to_string());
            }
            result_map
        }

        let uglist_role_map0 = group_by_property_list(&role_ug_map0);
        let uglist_role_map1 = group_by_property_list(&role_ug_map1);

        fn role_list(roles: &BTreeSet<String>) -> String {
            if roles.contains(ROLE_NAME_NO_ACCESS) { return String::from(ROLE_NAME_NO_ACCESS); }
            roles.iter().fold(String::new(), |mut acc, v| {
                if !acc.is_empty() { acc.push(','); }
                acc.push_str(v);
                acc
            })
        }

        for (uglist, roles) in &uglist_role_map0 {
            let role_list = role_list(roles);
            writeln!(w, "acl:0:{}:{}:{}", if path.is_empty() { "/" } else { path }, uglist, role_list)?;
        }

        for (uglist, roles) in &uglist_role_map1 {
            let role_list = role_list(roles);
            writeln!(w, "acl:1:{}:{}:{}", if path.is_empty() { "/" } else { path }, uglist, role_list)?;
        }

        for (name, child) in node.children.iter() {
            let child_path = format!("{}/{}", path, name);
            Self::write_node_config(child, &child_path, w)?;
        }

        Ok(())
    }

    pub fn write_config(&self, w: &mut dyn Write) -> Result<(), Error> {
        Self::write_node_config(&self.root, "", w)
    }

    fn parse_acl_line(&mut self, line: &str) -> Result<(), Error> {

        let items: Vec<&str> = line.split(':').collect();

        if items.len() != 5 {
            bail!("wrong number of items.");
        }

        if items[0] != "acl" {
            bail!("line does not start with 'acl'.");
        }

        let propagate = if items[1] == "0" {
            false
        } else if items[1] == "1" {
            true
        } else {
            bail!("expected '0' or '1' for propagate flag.");
        };

        let path = split_acl_path(items[2]);
        let node = self.get_or_insert_node(&path);

        let uglist: Vec<&str> = items[3].split(',').map(|v| v.trim()).collect();

        let rolelist: Vec<&str> = items[4].split(',').map(|v| v.trim()).collect();

        for user_or_group in &uglist {
            for role in &rolelist {
                if !ROLE_NAMES.contains_key(role) {
                    bail!("unknown role '{}'", role);
                }
                if user_or_group.starts_with('@') {
                    let group = &user_or_group[1..];
                    node.insert_group_role(group.to_string(), role.to_string(), propagate);
                } else {
                    node.insert_user_role(user_or_group.to_string(), role.to_string(), propagate);
                }
            }
        }

        Ok(())
    }

    pub fn load(filename: &Path) -> Result<(Self, [u8;32]), Error> {
        let mut tree = Self::new();

        let raw = match std::fs::read_to_string(filename) {
            Ok(v) => v,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    String::new()
                } else {
                    bail!("unable to read acl config {:?} - {}", filename, err);
                }
            }
        };

        let digest = openssl::sha::sha256(raw.as_bytes());

        for (linenr, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() { continue; }
            if let Err(err) = tree.parse_acl_line(line) {
                bail!("unable to parse acl config {:?}, line {} - {}",
                      filename, linenr+1, err);
            }
        }

        Ok((tree, digest))
    }

    pub fn from_raw(raw: &str) -> Result<Self, Error> {
        let mut tree = Self::new();
        for (linenr, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() { continue; }
            if let Err(err) = tree.parse_acl_line(line) {
                bail!("unable to parse acl config data, line {} - {}", linenr+1, err);
            }
        }
        Ok(tree)
    }

    pub fn roles(&self, userid: &str, path: &[&str]) -> HashSet<String> {

        let mut node = &self.root;
        let mut role_set = node.extract_roles(userid, path.is_empty());

        for (pos, comp) in path.iter().enumerate() {
            let last_comp = (pos + 1) == path.len();
            node = match node.children.get(*comp) {
                Some(n) => n,
                None => return role_set, // path not found
            };
            let new_set = node.extract_roles(userid, last_comp);
            if !new_set.is_empty() {
                // overwrite previous settings
                role_set = new_set;
            }
        }

        role_set
    }
}

pub const ACL_CFG_FILENAME: &str = "/etc/proxmox-backup/acl.cfg";
pub const ACL_CFG_LOCKFILE: &str = "/etc/proxmox-backup/.acl.lck";

pub fn config() -> Result<(AclTree, [u8; 32]), Error> {
    let path = PathBuf::from(ACL_CFG_FILENAME);
    AclTree::load(&path)
}

pub fn cached_config() -> Result<Arc<AclTree>, Error> {

    struct ConfigCache {
        data: Option<Arc<AclTree>>,
        last_mtime: i64,
        last_mtime_nsec: i64,
    }

    lazy_static! {
        static ref CACHED_CONFIG: RwLock<ConfigCache> = RwLock::new(
            ConfigCache { data: None, last_mtime: 0, last_mtime_nsec: 0 });
    }

    let stat = match nix::sys::stat::stat(ACL_CFG_FILENAME) {
        Ok(stat) => Some(stat),
        Err(nix::Error::Sys(nix::errno::Errno::ENOENT)) => None,
        Err(err) => bail!("unable to stat '{}' - {}", ACL_CFG_FILENAME, err),
    };

    if let Some(stat) = stat {
        let cache = CACHED_CONFIG.read().unwrap();
        if stat.st_mtime == cache.last_mtime && stat.st_mtime_nsec == cache.last_mtime_nsec {
            if let Some(ref config) = cache.data {
                return Ok(config.clone());
            }
        }
    }

    let (config, _digest) = config()?;
    let config = Arc::new(config);

    let mut cache = CACHED_CONFIG.write().unwrap();
    if let Some(stat) = stat {
        cache.last_mtime = stat.st_mtime;
        cache.last_mtime_nsec = stat.st_mtime_nsec;
    }
    cache.data = Some(config.clone());

    Ok(config)
}

pub fn save_config(acl: &AclTree) -> Result<(), Error> {
    let mut raw: Vec<u8> = Vec::new();

    acl.write_config(&mut raw)?;

    let backup_user = crate::backup::backup_user()?;
    let mode = nix::sys::stat::Mode::from_bits_truncate(0o0640);
    // set the correct owner/group/permissions while saving file
    // owner(rw) = root, group(r)= backup
    let options = CreateOptions::new()
        .perm(mode)
        .owner(nix::unistd::ROOT)
        .group(backup_user.gid);

    replace_file(ACL_CFG_FILENAME, &raw, options)?;

    Ok(())
}

#[cfg(test)]
mod test {

    use anyhow::{Error};
    use super::AclTree;

    fn check_roles(
        tree: &AclTree,
        user: &str,
        path: &str,
        expected_roles: &str,
    ) {

        let path_vec = super::split_acl_path(path);
        let mut roles = tree.roles(user, &path_vec)
            .iter().map(|v| v.clone()).collect::<Vec<String>>();
        roles.sort();
        let roles = roles.join(",");

        assert_eq!(roles, expected_roles, "\nat check_roles for '{}' on '{}'", user, path);
    }

    #[test]
    fn test_acl_line_compression() -> Result<(), Error> {

        let tree = AclTree::from_raw(r###"
acl:0:/store/store2:user1:Admin
acl:0:/store/store2:user2:Admin
acl:0:/store/store2:user1:Datastore.Backup
acl:0:/store/store2:user2:Datastore.Backup
"###)?;

        let mut raw: Vec<u8> = Vec::new();
        tree.write_config(&mut raw)?;
        let raw = std::str::from_utf8(&raw)?;

        assert_eq!(raw, "acl:0:/store/store2:user1,user2:Admin,Datastore.Backup\n");

        Ok(())
    }

    #[test]
    fn test_roles_1() -> Result<(), Error> {

        let tree = AclTree::from_raw(r###"
acl:1:/storage:user1@pbs:Admin
acl:1:/storage/store1:user1@pbs:Datastore.Backup
acl:1:/storage/store2:user2@pbs:Datastore.Backup
"###)?;
        check_roles(&tree, "user1@pbs", "/", "");
        check_roles(&tree, "user1@pbs", "/storage", "Admin");
        check_roles(&tree, "user1@pbs", "/storage/store1", "Datastore.Backup");
        check_roles(&tree, "user1@pbs", "/storage/store2", "Admin");

        check_roles(&tree, "user2@pbs", "/", "");
        check_roles(&tree, "user2@pbs", "/storage", "");
        check_roles(&tree, "user2@pbs", "/storage/store1", "");
        check_roles(&tree, "user2@pbs", "/storage/store2", "Datastore.Backup");

        Ok(())
    }

    #[test]
    fn test_role_no_access() -> Result<(), Error> {

        let tree = AclTree::from_raw(r###"
acl:1:/:user1@pbs:Admin
acl:1:/storage:user1@pbs:NoAccess
acl:1:/storage/store1:user1@pbs:Datastore.Backup
"###)?;
        check_roles(&tree, "user1@pbs", "/", "Admin");
        check_roles(&tree, "user1@pbs", "/storage", "NoAccess");
        check_roles(&tree, "user1@pbs", "/storage/store1", "Datastore.Backup");
        check_roles(&tree, "user1@pbs", "/storage/store2", "NoAccess");
        check_roles(&tree, "user1@pbs", "/system", "Admin");

        let tree = AclTree::from_raw(r###"
acl:1:/:user1@pbs:Admin
acl:0:/storage:user1@pbs:NoAccess
acl:1:/storage/store1:user1@pbs:Datastore.Backup
"###)?;
        check_roles(&tree, "user1@pbs", "/", "Admin");
        check_roles(&tree, "user1@pbs", "/storage", "NoAccess");
        check_roles(&tree, "user1@pbs", "/storage/store1", "Datastore.Backup");
        check_roles(&tree, "user1@pbs", "/storage/store2", "Admin");
        check_roles(&tree, "user1@pbs", "/system", "Admin");

        Ok(())
    }

    #[test]
    fn test_role_add_delete() -> Result<(), Error> {

        let mut tree = AclTree::new();

        tree.insert_user_role("/", "user1@pbs", "Admin", true);
        tree.insert_user_role("/", "user1@pbs", "Audit", true);

        check_roles(&tree, "user1@pbs", "/", "Admin,Audit");

        tree.insert_user_role("/", "user1@pbs", "NoAccess", true);
        check_roles(&tree, "user1@pbs", "/", "NoAccess");

        let mut raw: Vec<u8> = Vec::new();
        tree.write_config(&mut raw)?;
        let raw = std::str::from_utf8(&raw)?;

        assert_eq!(raw, "acl:1:/:user1@pbs:NoAccess\n");

        Ok(())
    }

    #[test]
    fn test_no_access_overwrite() -> Result<(), Error> {

        let mut tree = AclTree::new();

        tree.insert_user_role("/storage", "user1@pbs", "NoAccess", true);

        check_roles(&tree, "user1@pbs", "/storage", "NoAccess");

        tree.insert_user_role("/storage", "user1@pbs", "Admin", true);
        tree.insert_user_role("/storage", "user1@pbs", "Audit", true);

        check_roles(&tree, "user1@pbs", "/storage", "Admin,Audit");

        tree.insert_user_role("/storage", "user1@pbs", "NoAccess", true);

        check_roles(&tree, "user1@pbs", "/storage", "NoAccess");

        Ok(())
    }

}

use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr as _,
};

use rustc_hash::FxHashSet;
use url::Url;

/// Permissions for accessing environment variables
#[derive(Clone, Debug)]
pub enum EnvPermissions {
    /// All environment variables are accessible
    All {
        /// Environment variables that are denied access
        denied: FxHashSet<String>,
    },
    /// Some specific environment variables are accessible
    Some {
        /// Environment variables that are allowed access
        allowed: FxHashSet<String>,
        /// Environment variables that are denied access, these take precedence over allowed
        denied: FxHashSet<String>,
    },
}

/// Permissions for accessing network resources
#[derive(Clone, Debug)]
pub enum NetPermissions {
    /// All network resources are accessible
    All {
        /// Network resources that are denied access
        denied: FxHashSet<String>,
    },
    /// Some specific network resources are accessible
    Some {
        /// Network resources that are allowed access
        allowed: FxHashSet<String>,
        /// Network resources that are denied access, these take precedence over allowed
        denied: FxHashSet<String>,
    },
}

/// Permissions for reading files from the filesystem
#[derive(Clone, Debug)]
pub enum ReadPermissions {
    /// All paths are readable
    All {
        /// Paths that are denied read access
        denied: FxHashSet<PathBuf>,
    },
    /// Some specific paths are readable
    Some {
        /// Paths that are allowed read access
        allowed: FxHashSet<PathBuf>,
        /// Paths that are denied read access, these take precedence over allowed
        denied: FxHashSet<PathBuf>,
    },
}

/// Permissions for writing files to the filesystem
#[derive(Clone, Debug)]
pub enum WritePermissions {
    /// All paths are writable
    All {
        /// Paths that are denied write access
        denied: FxHashSet<PathBuf>,
    },
    /// Some specific paths are writable
    Some {
        /// Paths that are allowed write access
        allowed: FxHashSet<PathBuf>,
        /// Paths that are denied write access, these take precedence over allowed
        denied: FxHashSet<PathBuf>,
    },
}

/// Permissions for accessing various resources
#[derive(Clone, Debug)]
pub enum Permissions {
    /// All resources are allowed access
    All {
        /// Environment variables that are denied access
        denied_env: FxHashSet<String>,
        /// Network resources that are denied access
        denied_net: FxHashSet<String>,
        /// Paths that are denied read access
        denied_read: FxHashSet<PathBuf>,
        /// Paths that are denied write access
        denied_write: FxHashSet<PathBuf>,
    },
    /// Some resources are allowed access
    Some {
        /// Environment variables that are allowed access
        env: EnvPermissions,
        /// Network resources that are allowed access
        net: NetPermissions,
        /// Filesystem read permissions
        read: ReadPermissions,
        /// Filesystem write permissions
        write: WritePermissions,
    },
}

impl Permissions {
    /// Checks if the given environment variable key is allowed
    pub fn is_env_allowed<S: AsRef<str>>(&self, key: S) -> bool {
        let key = key.as_ref();
        match self {
            Permissions::All { denied_env, .. } => !denied_env.contains(key),
            Permissions::Some { env, .. } => match env {
                EnvPermissions::All { denied } => !denied.contains(key),
                EnvPermissions::Some { allowed, denied } => {
                    allowed.contains(key) && !denied.contains(key)
                }
            },
        }
    }

    /// Checks if a path matches any entry in the set.
    /// Supports exact match and directory-level inheritance (`path.starts_with(entry)`).
    fn is_path_matched(set: &FxHashSet<PathBuf>, path: &Path) -> bool {
        set.iter()
            .any(|entry| path == entry || path.starts_with(entry))
    }

    /// Checks if the given path is allowed for reading
    pub fn is_read_allowed(&self, path: &Path) -> bool {
        match self {
            Permissions::All { denied_read, .. } => !Self::is_path_matched(denied_read, path),
            Permissions::Some { read, .. } => match read {
                ReadPermissions::All { denied } => !Self::is_path_matched(denied, path),
                ReadPermissions::Some { allowed, denied } => {
                    Self::is_path_matched(allowed, path) && !Self::is_path_matched(denied, path)
                }
            },
        }
    }

    /// Checks if the given path is allowed for writing
    pub fn is_write_allowed(&self, path: &Path) -> bool {
        match self {
            Permissions::All { denied_write, .. } => !Self::is_path_matched(denied_write, path),
            Permissions::Some { write, .. } => match write {
                WritePermissions::All { denied } => !Self::is_path_matched(denied, path),
                WritePermissions::Some { allowed, denied } => {
                    Self::is_path_matched(allowed, path) && !Self::is_path_matched(denied, path)
                }
            },
        }
    }

    fn is_domain_or_ip_allowed(expected: &FxHashSet<String>, addr: &str) -> bool {
        if let Ok(sock_addr) = SocketAddr::from_str(addr) {
            // host with port e.g. 1.1.1.1:1234
            let (ip, port) = (sock_addr.ip(), sock_addr.port());
            expected.contains(&format!("{ip}:{port}")) || expected.contains(&ip.to_string())
        } else if let Ok(ip_addr) = IpAddr::from_str(addr) {
            // host without port e.g. 1.1.1.1
            expected.contains(&ip_addr.to_string())
        } else {
            // domain name e.g. example.com or example.com:1234
            let parts: Vec<_> = addr.split(':').collect();
            match (parts.first(), parts.get(1)) {
                (Some(host), None) if !host.is_empty() => expected.contains(*host),
                // when list = ("example.com"), both "example.com" and "example.com:1234" matches
                (Some(host), Some(port)) => {
                    expected.contains(*host) || expected.contains(&format!("{host}:{port}"))
                }
                _ => false,
            }
        }
    }

    /// Checks if the given network address is allowed
    pub fn is_net_allowed<S: AsRef<str>>(&self, addr: S) -> bool {
        let addr = addr.as_ref();
        match self {
            Permissions::All { denied_net, .. } => !Self::is_domain_or_ip_allowed(denied_net, addr),
            Permissions::Some { net, .. } => match net {
                NetPermissions::All { denied } => !Self::is_domain_or_ip_allowed(denied, addr),
                NetPermissions::Some { allowed, denied } => {
                    Self::is_domain_or_ip_allowed(allowed, addr)
                        && !Self::is_domain_or_ip_allowed(denied, addr)
                }
            },
        }
    }

    /// Checks if the given URL is allowed
    pub fn is_url_allowed(&self, url: &Url) -> bool {
        // when list = ("example.com:443"), we should allow "https://example.com"
        match (url.host_str(), url.port_or_known_default()) {
            (Some(host), Some(port)) => self.is_net_allowed(format!("{host}:{port}")),
            (Some(host), None) => self.is_net_allowed(host),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustc_hash::FxHashSet;
    use test_case::test_case;
    use url::Url;

    use crate::permission::{
        EnvPermissions, NetPermissions, Permissions, ReadPermissions, WritePermissions,
    };

    /// Helper to build a default `Permissions::Some` with only env/net set,
    /// using permissive defaults for read/write.
    fn some_perm_env_net(env: EnvPermissions, net: NetPermissions) -> Permissions {
        Permissions::Some {
            env,
            net,
            read: ReadPermissions::Some {
                allowed: FxHashSet::default(),
                denied: FxHashSet::default(),
            },
            write: WritePermissions::Some {
                allowed: FxHashSet::default(),
                denied: FxHashSet::default(),
            },
        }
    }

    #[test_case(true, "A", &[])]
    #[test_case(false, "A", &["A"])]
    #[test_case(true, "A", &["B"])]
    fn test_all_env(expected: bool, actual: &str, denied_env: &[&str]) {
        let perm = Permissions::All {
            denied_env: denied_env.iter().map(|s| (*s).to_string()).collect(),
            denied_net: FxHashSet::default(),
            denied_read: FxHashSet::default(),
            denied_write: FxHashSet::default(),
        };
        assert_eq!(expected, perm.is_env_allowed(actual));
    }

    #[test_case(true, "", &[])]
    #[test_case(true, "1.1.1.1", &[])]
    #[test_case(false, "1.1.1.1", &["1.1.1.1"])]
    #[test_case(true, "1.1.1.1", &["2.2.2.2"])]
    #[test_case(true, "1.1.1.1:1234", &[])]
    #[test_case(false, "1.1.1.1:1234", &["1.1.1.1"])]
    #[test_case(false, "1.1.1.1:1234", &["1.1.1.1:1234"])]
    #[test_case(true, "1.1.1.1:1234", &["1.1.1.1:1235"])]
    #[test_case(true,"example.com", &[])]
    #[test_case(false,"example.com", &["example.com"])]
    #[test_case(true,"example.com", &["example.com:443"])]
    #[test_case(true,"example.com", &["another.com"])]
    #[test_case(true,"example.com", &["another.com:443"])]
    #[test_case(true,"example.com:443", &[])]
    #[test_case(false,"example.com:443", &["example.com"])]
    #[test_case(false,"example.com:443", &["example.com:443"])]
    #[test_case(true,"example.com:443", &["another.com"])]
    #[test_case(true,"example.com:443", &["another.com:443"])]
    fn test_all_net(expected: bool, actual: &str, denied_net: &[&str]) {
        let perm = Permissions::All {
            denied_env: FxHashSet::default(),
            denied_net: denied_net.iter().map(|s| (*s).to_string()).collect(),
            denied_read: FxHashSet::default(),
            denied_write: FxHashSet::default(),
        };
        assert_eq!(expected, perm.is_net_allowed(actual));
    }

    #[test_case(true, "A", &[])]
    #[test_case(false, "A", &["A"])]
    #[test_case(true, "A",  &["B"])]
    fn test_some_all_env(expected: bool, actual: &str, denied_env: &[&str]) {
        let perm = some_perm_env_net(
            EnvPermissions::All {
                denied: denied_env.iter().map(|s| (*s).to_string()).collect(),
            },
            NetPermissions::All {
                denied: FxHashSet::default(),
            },
        );
        assert_eq!(expected, perm.is_env_allowed(actual));
    }

    #[test_case(false, "A", &[], &[])]
    #[test_case(true, "A", &["A"], &[])]
    #[test_case(false, "A", &["A"], &["A"])]
    #[test_case(true, "A", &["A"], &["B"])]
    #[test_case(false, "A", &["B"], &[])]
    #[test_case(false, "A", &["B"], &["A"])]
    #[test_case(false, "A", &["B"], &["B"])]
    fn test_some_some_env(expected: bool, actual: &str, allowed_env: &[&str], denied_env: &[&str]) {
        let perm = some_perm_env_net(
            EnvPermissions::Some {
                allowed: allowed_env.iter().map(|s| (*s).to_string()).collect(),
                denied: denied_env.iter().map(|s| (*s).to_string()).collect(),
            },
            NetPermissions::All {
                denied: FxHashSet::default(),
            },
        );
        assert_eq!(expected, perm.is_env_allowed(actual));
    }

    #[test_case(true, "", &[])]
    #[test_case(true, "1.1.1.1", &[])]
    #[test_case(false, "1.1.1.1", &["1.1.1.1"])]
    #[test_case(true, "1.1.1.1", &["2.2.2.2"])]
    #[test_case(true, "1.1.1.1:1234", &[])]
    #[test_case(false, "1.1.1.1:1234", &["1.1.1.1"])]
    #[test_case(false, "1.1.1.1:1234", &["1.1.1.1:1234"])]
    #[test_case(true, "1.1.1.1:1234", &["1.1.1.1:1235"])]
    #[test_case(true,"example.com", &[])]
    #[test_case(false,"example.com", &["example.com"])]
    #[test_case(true,"example.com", &["example.com:443"])]
    #[test_case(true,"example.com", &["another.com"])]
    #[test_case(true,"example.com", &["another.com:443"])]
    #[test_case(true,"example.com:443", &[])]
    #[test_case(false,"example.com:443", &["example.com"])]
    #[test_case(false,"example.com:443", &["example.com:443"])]
    #[test_case(true,"example.com:443", &["another.com"])]
    #[test_case(true,"example.com:443", &["another.com:443"])]
    fn test_some_all_net(expected: bool, actual: &str, denied_net: &[&str]) {
        let perm = some_perm_env_net(
            EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            NetPermissions::All {
                denied: denied_net.iter().map(|s| (*s).to_string()).collect(),
            },
        );
        assert_eq!(expected, perm.is_net_allowed(actual));
    }

    #[test_case(false, "", &[], &[])]
    #[test_case(false, "1.1.1.1", &[], &[])]
    #[test_case(true, "1.1.1.1", &["1.1.1.1"], &[])]
    #[test_case(false, "1.1.1.1", &["1.1.1.1"], &["1.1.1.1"])]
    #[test_case(false, "1.1.1.1", &["2.2.2.2"], &[])]
    #[test_case(false, "1.1.1.1:1234", &[], &[])]
    #[test_case(true, "1.1.1.1:1234", &["1.1.1.1"], &[])]
    #[test_case(false, "1.1.1.1:1234", &["1.1.1.1"], &["1.1.1.1"])]
    #[test_case(true, "1.1.1.1:1234", &["1.1.1.1:1234"], &[])]
    #[test_case(false, "1.1.1.1:1234", &["1.1.1.1:1234"], &["1.1.1.1:1234"])]
    #[test_case(false, "1.1.1.1:1234", &["1.1.1.1:1235"], &[])]
    #[test_case(false, "example.com", &[], &[])]
    #[test_case(true, "example.com", &["example.com"], &[])]
    #[test_case(false, "example.com", &["example.com"], &["example.com"])]
    #[test_case(false, "example.com", &["example.com:443"], &[])]
    #[test_case(false, "example.com", &["another.com"], &[])]
    #[test_case(false, "example.com", &["another.com:443"], &[])]
    #[test_case(false, "example.com:443", &[], &[])]
    #[test_case(true, "example.com:443", &["example.com"], &[])]
    #[test_case(false, "example.com:443", &["example.com"], &["example.com"])]
    #[test_case(true, "example.com:443", &["example.com:443"], &[])]
    #[test_case(false, "example.com:443", &["example.com:443"], &["example.com:443"])]
    #[test_case(false, "example.com:443", &["another.com"], &[])]
    #[test_case(false, "example.com:443", &["another.com:443"], &[])]

    fn test_some_some_net(expected: bool, actual: &str, allowed_net: &[&str], denied_net: &[&str]) {
        let perm = some_perm_env_net(
            EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            NetPermissions::Some {
                allowed: allowed_net.iter().map(|s| (*s).to_string()).collect(),
                denied: denied_net.iter().map(|s| (*s).to_string()).collect(),
            },
        );
        assert_eq!(expected, perm.is_net_allowed(actual));
    }

    #[test_case(false, "http://1.1.1.1", &[], &[])]
    #[test_case(true, "http://1.1.1.1", &["1.1.1.1"], &[])]
    #[test_case(false, "http://1.1.1.1", &["1.1.1.1"], &["1.1.1.1"])]
    #[test_case(true, "http://1.1.1.1", &["1.1.1.1:80"], &[])]
    #[test_case(false, "http://1.1.1.1", &["1.1.1.1:80"], &["1.1.1.1:80"])]
    #[test_case(false,"http://example.com", &[], &[])]
    #[test_case(true, "http://example.com", &["example.com"], &[])]
    #[test_case(false, "http://example.com", &["example.com"], &["example.com"])]
    #[test_case(true, "http://example.com", &["example.com:80"], &[])]
    #[test_case(false, "http://example.com", &["example.com:80"], &["example.com:80"])]
    #[test_case(false, "http://example.com", &["another.com"], &[])]
    #[test_case(false, "http://example.com", &["another.com:80"], &[])]
    #[test_case(false, "ssh://example.com", &[], &[])]
    #[test_case(false, "unix:/run/foo.socket", &[], &[])]
    fn test_url(expected: bool, actual: &str, allowed_net: &[&str], denied_net: &[&str]) {
        let perm = some_perm_env_net(
            EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            NetPermissions::Some {
                allowed: allowed_net.iter().map(|s| (*s).to_string()).collect(),
                denied: denied_net.iter().map(|s| (*s).to_string()).collect(),
            },
        );
        assert_eq!(
            expected,
            perm.is_url_allowed(&actual.parse::<Url>().unwrap())
        );
    }

    // --- Read permission tests ---

    #[test_case(true, "/tmp/file.txt", &[]; "all_read_no_deny")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"]; "all_read_deny_parent")]
    #[test_case(false, "/tmp/file.txt", &["/tmp/file.txt"]; "all_read_deny_exact")]
    #[test_case(true, "/home/file.txt", &["/tmp"]; "all_read_deny_other")]
    fn test_all_read(expected: bool, path: &str, denied: &[&str]) {
        let perm = Permissions::All {
            denied_env: FxHashSet::default(),
            denied_net: FxHashSet::default(),
            denied_read: denied.iter().map(|s| PathBuf::from(s)).collect(),
            denied_write: FxHashSet::default(),
        };
        assert_eq!(expected, perm.is_read_allowed(&PathBuf::from(path)));
    }

    #[test_case(true, "/tmp/file.txt", &[]; "all_write_no_deny")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"]; "all_write_deny_parent")]
    #[test_case(false, "/tmp/file.txt", &["/tmp/file.txt"]; "all_write_deny_exact")]
    #[test_case(true, "/home/file.txt", &["/tmp"]; "all_write_deny_other")]
    fn test_all_write(expected: bool, path: &str, denied: &[&str]) {
        let perm = Permissions::All {
            denied_env: FxHashSet::default(),
            denied_net: FxHashSet::default(),
            denied_read: FxHashSet::default(),
            denied_write: denied.iter().map(|s| PathBuf::from(s)).collect(),
        };
        assert_eq!(expected, perm.is_write_allowed(&PathBuf::from(path)));
    }

    #[test_case(false, "/tmp/file.txt", &[], &[]; "some_read_no_allow")]
    #[test_case(true, "/tmp/file.txt", &["/tmp"], &[]; "some_read_allow_parent")]
    #[test_case(true, "/tmp/file.txt", &["/tmp/file.txt"], &[]; "some_read_allow_exact")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"], &["/tmp"]; "some_read_deny_overrides")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"], &["/tmp/file.txt"]; "some_read_deny_exact")]
    #[test_case(true, "/tmp/a/b.txt", &["/tmp"], &["/tmp/c"]; "some_read_deny_other_subdir")]
    #[test_case(false, "/home/file.txt", &["/tmp"], &[]; "some_read_wrong_dir")]
    fn test_some_some_read(expected: bool, path: &str, allowed: &[&str], denied: &[&str]) {
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
            read: ReadPermissions::Some {
                allowed: allowed.iter().map(|s| PathBuf::from(s)).collect(),
                denied: denied.iter().map(|s| PathBuf::from(s)).collect(),
            },
            write: WritePermissions::Some {
                allowed: FxHashSet::default(),
                denied: FxHashSet::default(),
            },
        };
        assert_eq!(expected, perm.is_read_allowed(&PathBuf::from(path)));
    }

    #[test_case(true, "/tmp/file.txt", &[]; "some_all_read_no_deny")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"]; "some_all_read_deny_parent")]
    fn test_some_all_read(expected: bool, path: &str, denied: &[&str]) {
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
            read: ReadPermissions::All {
                denied: denied.iter().map(|s| PathBuf::from(s)).collect(),
            },
            write: WritePermissions::Some {
                allowed: FxHashSet::default(),
                denied: FxHashSet::default(),
            },
        };
        assert_eq!(expected, perm.is_read_allowed(&PathBuf::from(path)));
    }

    #[test_case(false, "/tmp/file.txt", &[], &[]; "some_write_no_allow")]
    #[test_case(true, "/tmp/file.txt", &["/tmp"], &[]; "some_write_allow_parent")]
    #[test_case(true, "/tmp/file.txt", &["/tmp/file.txt"], &[]; "some_write_allow_exact")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"], &["/tmp"]; "some_write_deny_overrides")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"], &["/tmp/file.txt"]; "some_write_deny_exact")]
    #[test_case(false, "/home/file.txt", &["/tmp"], &[]; "some_write_wrong_dir")]
    fn test_some_some_write(expected: bool, path: &str, allowed: &[&str], denied: &[&str]) {
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
            read: ReadPermissions::Some {
                allowed: FxHashSet::default(),
                denied: FxHashSet::default(),
            },
            write: WritePermissions::Some {
                allowed: allowed.iter().map(|s| PathBuf::from(s)).collect(),
                denied: denied.iter().map(|s| PathBuf::from(s)).collect(),
            },
        };
        assert_eq!(expected, perm.is_write_allowed(&PathBuf::from(path)));
    }

    #[test_case(true, "/tmp/file.txt", &[]; "some_all_write_no_deny")]
    #[test_case(false, "/tmp/file.txt", &["/tmp"]; "some_all_write_deny_parent")]
    fn test_some_all_write(expected: bool, path: &str, denied: &[&str]) {
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
            read: ReadPermissions::Some {
                allowed: FxHashSet::default(),
                denied: FxHashSet::default(),
            },
            write: WritePermissions::All {
                denied: denied.iter().map(|s| PathBuf::from(s)).collect(),
            },
        };
        assert_eq!(expected, perm.is_write_allowed(&PathBuf::from(path)));
    }
}

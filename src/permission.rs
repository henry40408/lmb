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

/// Permissions for accessing file system paths
#[derive(Clone, Debug)]
pub enum FsPermissions {
    /// All file system paths are accessible
    All {
        /// Paths that are denied access (canonicalized)
        denied: FxHashSet<PathBuf>,
    },
    /// Some specific file system paths are accessible
    Some {
        /// Paths that are allowed access (canonicalized), these are prefix-matched
        allowed: FxHashSet<PathBuf>,
        /// Paths that are denied access (canonicalized), these take precedence over allowed
        denied: FxHashSet<PathBuf>,
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

/// Permissions for accessing various resources
#[derive(Clone, Debug)]
pub enum Permissions {
    /// All resources are allowed access
    All {
        /// Environment variables that are denied access
        denied_env: FxHashSet<String>,
        /// File system paths that are denied access
        denied_fs: FxHashSet<PathBuf>,
        /// Network resources that are denied access
        denied_net: FxHashSet<String>,
    },
    /// Some resources are allowed access
    Some {
        /// Environment variables that are allowed access
        env: EnvPermissions,
        /// File system paths that are allowed access
        fs: FsPermissions,
        /// Network resources that are allowed access
        net: NetPermissions,
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

    /// Checks if the given file system path is allowed.
    ///
    /// For existing paths, the path is canonicalized before checking.
    /// For non-existing paths (e.g. write targets), the parent directory
    /// is canonicalized and the file name is appended.
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        let canonicalized = if path.exists() {
            path.canonicalize().ok()
        } else {
            // For non-existing paths, canonicalize parent and append file name
            path.parent()
                .and_then(|p| p.canonicalize().ok())
                .zip(path.file_name())
                .map(|(parent, name)| parent.join(name))
        };
        let Some(path) = canonicalized else {
            return false;
        };
        match self {
            Permissions::All { denied_fs, .. } => !Self::is_path_prefix_matched(denied_fs, &path),
            Permissions::Some { fs, .. } => match fs {
                FsPermissions::All { denied } => !Self::is_path_prefix_matched(denied, &path),
                FsPermissions::Some { allowed, denied } => {
                    Self::is_path_prefix_matched(allowed, &path)
                        && !Self::is_path_prefix_matched(denied, &path)
                }
            },
        }
    }

    fn is_path_prefix_matched(set: &FxHashSet<PathBuf>, path: &Path) -> bool {
        set.iter().any(|prefix| path.starts_with(prefix))
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

    use crate::permission::{EnvPermissions, FsPermissions, NetPermissions, Permissions};

    /// Helper to create a default `FsPermissions` for tests that don't care about fs
    fn default_fs() -> FsPermissions {
        FsPermissions::All {
            denied: FxHashSet::default(),
        }
    }

    #[test_case(true, "A", &[])]
    #[test_case(false, "A", &["A"])]
    #[test_case(true, "A", &["B"])]
    fn test_all_env(expected: bool, actual: &str, denied_env: &[&str]) {
        let perm = Permissions::All {
            denied_env: denied_env.iter().map(|s| (*s).to_string()).collect(),
            denied_fs: FxHashSet::default(),
            denied_net: FxHashSet::default(),
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
            denied_fs: FxHashSet::default(),
            denied_net: denied_net.iter().map(|s| (*s).to_string()).collect(),
        };
        assert_eq!(expected, perm.is_net_allowed(actual));
    }

    #[test_case(true, "A", &[])]
    #[test_case(false, "A", &["A"])]
    #[test_case(true, "A",  &["B"])]
    fn test_some_all_env(expected: bool, actual: &str, denied_env: &[&str]) {
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: denied_env.iter().map(|s| (*s).to_string()).collect(),
            },
            fs: default_fs(),
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
        };
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
        let perm = Permissions::Some {
            env: EnvPermissions::Some {
                allowed: allowed_env.iter().map(|s| (*s).to_string()).collect(),
                denied: denied_env.iter().map(|s| (*s).to_string()).collect(),
            },
            fs: default_fs(),
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
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
    fn test_some_all_net(expected: bool, actual: &str, denied_net: &[&str]) {
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: default_fs(),
            net: NetPermissions::All {
                denied: denied_net.iter().map(|s| (*s).to_string()).collect(),
            },
        };
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
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: default_fs(),
            net: NetPermissions::Some {
                allowed: allowed_net.iter().map(|s| (*s).to_string()).collect(),
                denied: denied_net.iter().map(|s| (*s).to_string()).collect(),
            },
        };
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
        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: default_fs(),
            net: NetPermissions::Some {
                allowed: allowed_net.iter().map(|s| (*s).to_string()).collect(),
                denied: denied_net.iter().map(|s| (*s).to_string()).collect(),
            },
        };
        assert_eq!(
            expected,
            perm.is_url_allowed(&actual.parse::<Url>().unwrap())
        );
    }

    #[test]
    fn test_all_fs_allowed() {
        let dir = std::env::temp_dir();
        let file = dir.join("lmb_test_perm_exists.txt");
        std::fs::write(&file, "test").ok();

        let perm = Permissions::All {
            denied_env: FxHashSet::default(),
            denied_fs: FxHashSet::default(),
            denied_net: FxHashSet::default(),
        };
        assert!(perm.is_path_allowed(&file));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn test_all_fs_denied() {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize temp dir");
        let file = dir.join("lmb_test_perm_denied.txt");
        std::fs::write(&file, "test").ok();

        let perm = Permissions::All {
            denied_env: FxHashSet::default(),
            denied_fs: [dir].into_iter().collect(),
            denied_net: FxHashSet::default(),
        };
        assert!(!perm.is_path_allowed(&file));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn test_some_fs_allowed() {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize temp dir");
        let file = dir.join("lmb_test_perm_some.txt");
        std::fs::write(&file, "test").ok();

        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: FsPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
        };
        assert!(perm.is_path_allowed(&file));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn test_some_fs_not_allowed() {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize temp dir");
        let file = dir.join("lmb_test_perm_not_allowed.txt");
        std::fs::write(&file, "test").ok();

        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: FsPermissions::Some {
                allowed: [PathBuf::from("/nonexistent")].into_iter().collect(),
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
        };
        assert!(!perm.is_path_allowed(&file));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn test_some_fs_deny_takes_precedence() {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize temp dir");
        let file = dir.join("lmb_test_perm_deny_prec.txt");
        std::fs::write(&file, "test").ok();

        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: FsPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: [dir].into_iter().collect(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
        };
        assert!(!perm.is_path_allowed(&file));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn test_fs_nonexistent_path_with_existing_parent() {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize temp dir");

        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: FsPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
        };
        // File doesn't exist but parent does — should resolve via parent canonicalization
        assert!(perm.is_path_allowed(&std::env::temp_dir().join("lmb_nonexistent_file.txt")));
    }

    #[test]
    fn test_fs_nonexistent_parent() {
        let perm = Permissions::All {
            denied_env: FxHashSet::default(),
            denied_fs: FxHashSet::default(),
            denied_net: FxHashSet::default(),
        };
        // Both file and parent don't exist — canonicalization fails, should deny
        assert!(!perm.is_path_allowed(std::path::Path::new("/totally/nonexistent/path/file.txt")));
    }
}

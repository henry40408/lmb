use std::{
    net::{IpAddr, SocketAddr},
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

/// Permissions for accessing various resources
#[derive(Clone, Debug)]
pub enum Permissions {
    /// All resources are allowed access
    All {
        /// Environment variables that are denied access
        denied_env: FxHashSet<String>,
        /// Network resources that are denied access
        denied_net: FxHashSet<String>,
    },
    /// Some resources are allowed access
    Some {
        /// Environment variables that are allowed access
        env: EnvPermissions,
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
            Permissions::All { denied_net, .. } => {
                !Self::is_domain_or_ip_allowed(denied_net, addr)
            }
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
    use rustc_hash::FxHashSet;
    use test_case::test_case;
    use url::Url;

    use crate::permission::{EnvPermissions, NetPermissions, Permissions};

    #[test_case(true, "A", &[])]
    #[test_case(false, "A", &["A"])]
    #[test_case(true, "A", &["B"])]
    fn test_all_env(expected: bool, actual: &str, denied_env: &[&str]) {
        let perm = Permissions::All {
            denied_env: denied_env.iter().map(|s| (*s).to_string()).collect(),
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
}

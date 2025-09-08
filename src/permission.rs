use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr as _,
};

use url::Url;

/// Permissions for accessing environment variables
#[derive(Clone, Debug)]
pub enum EnvPermissions {
    /// All environment variables are accessible
    All {
        /// Environment variables that are denied access
        denied: Vec<String>,
    },
    /// Some specific environment variables are accessible
    Some {
        /// Environment variables that are allowed access
        allowed: Vec<String>,
        /// Environment variables that are denied access, these take precedence over allowed
        denied: Vec<String>,
    },
}

/// Permissions for accessing network resources
#[derive(Clone, Debug)]
pub enum NetPermissions {
    /// All network resources are accessible
    All {
        /// Network resources that are denied access
        denied: Vec<String>,
    },
    /// Some specific network resources are accessible
    Some {
        /// Network resources that are allowed access
        allowed: Vec<String>,
        /// Network resources that are denied access, these take precedence over allowed
        denied: Vec<String>,
    },
}

/// Permissions for accessing various resources
#[derive(Clone, Debug)]
pub enum Permissions {
    /// All resources are allowed access
    All {
        /// Environment variables that are denied access
        denied_env: Vec<String>,
        /// Network resources that are denied access
        denied_net: Vec<String>,
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
        let key = key.as_ref().to_string();
        match self {
            Permissions::All { denied_env, .. } => !denied_env.contains(&key),
            Permissions::Some { env, .. } => match env {
                EnvPermissions::All { denied } => !denied.contains(&key),
                EnvPermissions::Some { allowed, denied } => {
                    if allowed.contains(&key) {
                        !denied.contains(&key)
                    } else {
                        false
                    }
                }
            },
        }
    }

    fn is_domain_or_ip_allowed<S: AsRef<str>>(expected: &[S], addr: &S) -> bool {
        let expected = expected
            .iter()
            .map(|s| s.as_ref().to_string())
            .collect::<Vec<_>>();
        let addr = addr.as_ref();
        if let Ok(addr) = SocketAddr::from_str(addr) {
            // host with port e.g. 1.1.1.1:1234
            let (ip, port) = (addr.ip(), addr.port());
            expected.contains(&format!("{ip}:{port}"))
        } else if let Ok(addr) = IpAddr::from_str(addr) {
            // host without port e.g. 1.1.1.1
            expected.contains(&addr.to_string())
        } else {
            // domain name e.g. example.com or example.com:1234
            let parts = addr.split(':').collect::<Vec<_>>();
            match (parts.first(), parts.get(1)) {
                (Some(host), None) if !host.is_empty() => expected.contains(&(*host).to_string()),
                // when list = ("example.com"), both "example.com" and "example.com:1234" matches
                (Some(host), Some(port)) => {
                    expected.contains(&(*host).to_string())
                        || expected.contains(&format!("{host}:{port}"))
                }
                _ => false,
            }
        }
    }

    /// Checks if the given network address is allowed
    pub fn is_net_allowed<S: AsRef<str>>(&self, addr: S) -> bool {
        let addr = addr.as_ref().to_string();
        match self {
            Permissions::All { denied_net, .. } => {
                !Self::is_domain_or_ip_allowed(denied_net, &addr)
            }
            Permissions::Some { net, .. } => match net {
                NetPermissions::All { denied } => !Self::is_domain_or_ip_allowed(denied, &addr),
                NetPermissions::Some { allowed, denied } => {
                    if Self::is_domain_or_ip_allowed(allowed, &addr) {
                        !Self::is_domain_or_ip_allowed(denied, &addr)
                    } else {
                        false
                    }
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
    use url::Url;

    use crate::permission::{EnvPermissions, NetPermissions, Permissions};

    #[test]
    fn test_permissions() {
        let perm = Permissions::All {
            denied_env: vec!["A".to_string()],
            denied_net: vec!["1.1.1.1".to_string()],
        };
        assert!(!perm.is_env_allowed("A"));
        assert!(perm.is_env_allowed("B"));
        assert!(!perm.is_net_allowed("1.1.1.1"));
        assert!(perm.is_net_allowed("2.2.2.2"));

        let perm = Permissions::Some {
            env: EnvPermissions::All {
                denied: vec!["A".to_string()],
            },
            net: NetPermissions::Some {
                allowed: vec![],
                denied: vec![],
            },
        };
        assert!(!perm.is_env_allowed("A"));
        assert!(perm.is_env_allowed("B"));

        let perm = Permissions::Some {
            env: EnvPermissions::Some {
                allowed: vec!["A".to_string()],
                denied: vec!["B".to_string()],
            },
            net: NetPermissions::Some {
                allowed: vec!["example.com:1234".to_string()],
                denied: vec!["example.com:1235".to_string()],
            },
        };
        assert!(perm.is_env_allowed("A"));
        assert!(!perm.is_env_allowed("B"));
        assert!(!perm.is_env_allowed("C"));

        assert!(!perm.is_net_allowed(""));
        assert!(!perm.is_net_allowed(":1234"));
        assert!(perm.is_net_allowed("example.com:1234"));
        assert!(!perm.is_net_allowed("example.com:1235"));

        // no port is specific and domain name is not explicitly allowed,
        // it should be considered as denied
        assert!(!perm.is_net_allowed("example.com"));

        assert!(!perm.is_url_allowed(&"ssh://example.com".parse::<Url>().unwrap()));
        assert!(!perm.is_url_allowed(&"unix:/run/foo.socket".parse::<Url>().unwrap()));
    }

    #[test]
    fn test_deny_domain_but_allow_port() {
        let perm = Permissions::Some {
            env: EnvPermissions::Some {
                allowed: vec![],
                denied: vec![],
            },
            // the following configuration conflicts because "example.com" is already denied
            net: NetPermissions::Some {
                allowed: vec!["example.com:1234".to_string()],
                denied: vec!["example.com".to_string()],
            },
        };
        assert!(!perm.is_net_allowed(""));
        assert!(!perm.is_net_allowed("example.com:1234"));
        assert!(!perm.is_net_allowed("example.com"));
    }
}

use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// The parsed form of a host, distinguishing IP families from hostnames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum HostKind {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Hostname(String),
}

/// A host + optional port, as found in `[ips]`, `[ips_fixed]`, and similar sections.
///
/// Accepted forms (per analysis §6.16):
/// - `host port`          — space-separated two tokens
/// - `host:port`          — colon-separated, but only when there is exactly one colon
///                          (avoids ambiguity with bare IPv6 addresses like `fe80::1`)
/// - `[ipv6addr]:port`    — RFC 2732 bracketed IPv6 with port
/// - `host`               — bare host, port is `None`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostPort {
    pub host: HostKind,
    pub port: Option<u16>,
}

impl<'de> Deserialize<'de> for HostPort {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = HostPort;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a host[:port] string")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<HostPort, E> {
                HostPort::from_str(s).map_err(|e| E::custom(e.message()))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<HostPort, E> {
                self.visit_str(&s)
            }
        }
        de.deserialize_str(V)
    }
}

impl HostPort {
    fn parse_host(s: &str) -> HostKind {
        if let Ok(ip4) = s.parse::<Ipv4Addr>() {
            HostKind::Ipv4(ip4)
        } else if let Ok(ip6) = s.parse::<Ipv6Addr>() {
            HostKind::Ipv6(ip6)
        } else {
            HostKind::Hostname(s.to_owned())
        }
    }
}

impl FromStr for HostPort {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if s.is_empty() {
            return Err(ConfigError::grammar("HostPort", s, "empty value"));
        }

        // Form 1: `[ipv6]:port` — bracketed IPv6 with optional port
        if s.starts_with('[') {
            let close = s.find(']').ok_or_else(|| {
                ConfigError::grammar("HostPort", s, "unmatched `[` in bracketed IPv6 address")
            })?;
            let ipv6_str = &s[1..close];
            let ipv6: Ipv6Addr = ipv6_str.parse().map_err(|_| {
                ConfigError::grammar("HostPort", s, "invalid IPv6 address in brackets")
            })?;
            let rest = &s[close + 1..];
            let port = if rest.is_empty() {
                None
            } else {
                let port_str = rest.strip_prefix(':').ok_or_else(|| {
                    ConfigError::grammar(
                        "HostPort",
                        s,
                        "expected `:port` after bracketed IPv6 address",
                    )
                })?;
                Some(port_str.parse::<u16>().map_err(|_| {
                    ConfigError::grammar("HostPort", s, "invalid port number")
                })?)
            };
            return Ok(HostPort {
                host: HostKind::Ipv6(ipv6),
                port,
            });
        }

        // Form 2: `host port` — space-separated
        if let Some(space_pos) = s.find(char::is_whitespace) {
            let host_str = &s[..space_pos];
            let port_str = s[space_pos..].trim();
            if !port_str.is_empty() {
                let port = port_str.parse::<u16>().map_err(|_| {
                    ConfigError::grammar("HostPort", s, "invalid port number")
                })?;
                return Ok(HostPort {
                    host: Self::parse_host(host_str),
                    port: Some(port),
                });
            }
        }

        // Form 3: `host:port` — but only if there is exactly one colon
        // (bare IPv6 addresses have multiple colons, so we must not split on them).
        let colon_count = s.chars().filter(|&c| c == ':').count();
        if colon_count == 1 {
            let colon_pos = s.find(':').unwrap();
            let host_str = &s[..colon_pos];
            let port_str = &s[colon_pos + 1..];
            let port = port_str.parse::<u16>().map_err(|_| {
                ConfigError::grammar("HostPort", s, "invalid port number")
            })?;
            return Ok(HostPort {
                host: Self::parse_host(host_str),
                port: Some(port),
            });
        }

        // Form 4: bare host (IPv4, IPv6, or hostname), no port
        Ok(HostPort {
            host: Self::parse_host(s),
            port: None,
        })
    }
}

impl std::fmt::Display for HostPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.host {
            HostKind::Ipv4(a) => write!(f, "{}", a)?,
            HostKind::Ipv6(a) => write!(f, "{}", a)?,
            HostKind::Hostname(h) => write!(f, "{}", h)?,
        }
        if let Some(port) = self.port {
            write!(f, " {}", port)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp(s: &str) -> HostPort {
        s.parse().unwrap_or_else(|e| panic!("parse failed for `{s}`: {e}"))
    }

    #[test]
    fn space_separated() {
        let h = hp("192.168.1.1 6006");
        assert_eq!(h.host, HostKind::Ipv4("192.168.1.1".parse().unwrap()));
        assert_eq!(h.port, Some(6006));
    }

    #[test]
    fn colon_separated() {
        let h = hp("192.168.1.1:6006");
        assert_eq!(h.host, HostKind::Ipv4("192.168.1.1".parse().unwrap()));
        assert_eq!(h.port, Some(6006));
    }

    #[test]
    fn bracketed_ipv6_with_port() {
        let h = hp("[fe80::1]:51235");
        assert_eq!(h.host, HostKind::Ipv6("fe80::1".parse().unwrap()));
        assert_eq!(h.port, Some(51235));
    }

    #[test]
    fn bare_hostname() {
        let h = hp("r.ripple.com");
        assert_eq!(h.host, HostKind::Hostname("r.ripple.com".to_owned()));
        assert_eq!(h.port, None);
    }

    #[test]
    fn bare_ipv4() {
        let h = hp("10.0.0.1");
        assert_eq!(h.host, HostKind::Ipv4("10.0.0.1".parse().unwrap()));
        assert_eq!(h.port, None);
    }

    #[test]
    fn bare_ipv6_no_port() {
        // Multiple colons → no colon-split; treated as bare IPv6
        let h = hp("fe80::1");
        assert_eq!(h.host, HostKind::Ipv6("fe80::1".parse().unwrap()));
        assert_eq!(h.port, None);
    }

    #[test]
    fn ipv6_space_port() {
        let h = hp("fe80::1 51235");
        assert_eq!(h.host, HostKind::Ipv6("fe80::1".parse().unwrap()));
        assert_eq!(h.port, Some(51235));
    }

    #[test]
    fn hostname_with_port() {
        let h = hp("example.com:6006");
        assert_eq!(h.host, HostKind::Hostname("example.com".to_owned()));
        assert_eq!(h.port, Some(6006));
    }

    #[test]
    fn empty_is_error() {
        assert!("".parse::<HostPort>().is_err());
    }

    // ---- additional coverage ----

    #[test]
    fn whitespace_only_is_error() {
        assert!("   ".parse::<HostPort>().is_err());
    }

    #[test]
    fn multi_colon_non_bracketed_is_bare_ipv6() {
        // a:b:c is a valid IPv6 address — should be parsed as bare IPv6, not error
        // "::1" is the loopback IPv6 address
        let h = hp("::1");
        assert_eq!(h.host, HostKind::Ipv6("::1".parse().unwrap()));
        assert_eq!(h.port, None);
    }

    #[test]
    fn multi_colon_non_ipv6_hostname_is_error() {
        // "a:b:c" has exactly 2 colons — not a valid IPv6, and colon_count != 1
        // so it falls through to bare host parse, which classifies it as Hostname
        // (since it can't parse as IPv4 or IPv6)
        let h = hp("a:b:c");
        // It's classified as a hostname (not rejected)
        assert!(matches!(h.host, HostKind::Hostname(_)));
        assert_eq!(h.port, None);
    }

    #[test]
    fn invalid_port_colon_form() {
        assert!("example.com:notaport".parse::<HostPort>().is_err());
    }

    #[test]
    fn invalid_port_space_form() {
        assert!("example.com notaport".parse::<HostPort>().is_err());
    }

    #[test]
    fn missing_close_bracket() {
        assert!("[fe80::1".parse::<HostPort>().is_err());
    }

    #[test]
    fn bracketed_invalid_ipv6() {
        assert!("[notanipv6]:6006".parse::<HostPort>().is_err());
    }

    #[test]
    fn bracketed_ipv6_no_port() {
        // [ipv6] with no :port is valid — port is None
        let h = hp("[::1]");
        assert_eq!(h.host, HostKind::Ipv6("::1".parse().unwrap()));
        assert_eq!(h.port, None);
    }

    #[test]
    fn bracketed_ipv6_invalid_port() {
        assert!("[::1]:notaport".parse::<HostPort>().is_err());
    }

    #[test]
    fn port_zero() {
        // Port 0 is a valid u16 — should parse successfully
        let h = hp("example.com:0");
        assert_eq!(h.port, Some(0));
    }

    #[test]
    fn port_max_65535() {
        let h = hp("example.com:65535");
        assert_eq!(h.port, Some(65535));
    }

    #[test]
    fn port_overflow_65536() {
        assert!("example.com:65536".parse::<HostPort>().is_err());
    }

    #[test]
    fn ipv4_classification() {
        let h = hp("127.0.0.1");
        assert!(matches!(h.host, HostKind::Ipv4(_)));
    }

    #[test]
    fn ipv6_classification() {
        let h = hp("2001:db8::1");
        assert!(matches!(h.host, HostKind::Ipv6(_)));
    }

    #[test]
    fn hostname_classification() {
        let h = hp("localhost");
        assert!(matches!(h.host, HostKind::Hostname(_)));
    }

    #[test]
    fn trimmed_whitespace_input() {
        let h = hp("  192.168.0.1  ");
        assert_eq!(h.host, HostKind::Ipv4("192.168.0.1".parse().unwrap()));
        assert_eq!(h.port, None);
    }

    #[test]
    fn bracketed_ipv6_garbage_after_bracket() {
        // [::1]garbage (no colon prefix) should be an error
        assert!("[::1]garbage".parse::<HostPort>().is_err());
    }
}

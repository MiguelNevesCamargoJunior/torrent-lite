//! Conservative public-destination policy. Enforcement belongs at socket creation.
//!
//! Sources: IANA IPv4 and IPv6 Special-Purpose Address Registries, 2026-10-25.
//! https://www.iana.org/assignments/iana-ipv4-special-registry/
//! https://www.iana.org/assignments/iana-ipv6-special-registry/

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationPolicy {
    PublicOnly,
    AllowLan,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrackerUrlError {
    #[error("tracker URL is too long")]
    TooLong,
    #[error("invalid tracker URL")]
    Invalid,
    #[error("tracker destination is not permitted")]
    Forbidden,
}

fn in_v4_range(address: Ipv4Addr, network: [u8; 4], prefix: u32) -> bool {
    let shift = 32 - prefix;
    u32::from(address) >> shift == u32::from(Ipv4Addr::from(network)) >> shift
}

fn in_v6_range(address: Ipv6Addr, network: Ipv6Addr, prefix: u32) -> bool {
    let shift = 128 - prefix;
    u128::from(address) >> shift == u128::from(network) >> shift
}

impl DestinationPolicy {
    pub fn permits_ip(self, address: IpAddr) -> bool {
        if self == Self::AllowLan {
            match address {
                IpAddr::V4(ip)
                    if in_v4_range(ip, [10, 0, 0, 0], 8)
                        || in_v4_range(ip, [127, 0, 0, 0], 8)
                        || in_v4_range(ip, [172, 16, 0, 0], 12)
                        || in_v4_range(ip, [192, 168, 0, 0], 16) =>
                {
                    return true;
                }
                IpAddr::V6(ip)
                    if ip.is_loopback()
                        || in_v6_range(ip, Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        match address {
            IpAddr::V4(address) => {
                // Deny all currently non-global special blocks and the whole 192.0.0/24
                // protocol block, including its small globally reachable exceptions.
                const DENIED: [([u8; 4], u32); 16] = [
                    ([0, 0, 0, 0], 8),
                    ([10, 0, 0, 0], 8),
                    ([100, 64, 0, 0], 10),
                    ([127, 0, 0, 0], 8),
                    ([169, 254, 0, 0], 16),
                    ([172, 16, 0, 0], 12),
                    ([192, 0, 0, 0], 24),
                    ([192, 0, 2, 0], 24),
                    ([192, 88, 99, 0], 24),
                    ([192, 168, 0, 0], 16),
                    ([198, 18, 0, 0], 15),
                    ([198, 51, 100, 0], 24),
                    ([203, 0, 113, 0], 24),
                    ([224, 0, 0, 0], 4),
                    ([240, 0, 0, 0], 4),
                    ([255, 255, 255, 255], 32),
                ];
                !DENIED
                    .iter()
                    .any(|(network, prefix)| in_v4_range(address, *network, *prefix))
            }
            IpAddr::V6(address) => {
                // Only the ordinary global-unicast allocation is considered here.
                // Exclude special ranges inside it, including 6to4 encapsulation.
                in_v6_range(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3)
                    && !in_v6_range(address, Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23)
                    && !in_v6_range(address, Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 32)
                    && !in_v6_range(address, Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16)
                    && !in_v6_range(address, Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20)
            }
        }
    }

    /// Validate syntax before handing a tracker to the engine. The proxy must still
    /// resolve its host and check the concrete IP for every connection.
    pub fn validate_tracker_url(self, value: &str) -> Result<Url, TrackerUrlError> {
        if value.len() > 8192 {
            return Err(TrackerUrlError::TooLong);
        }
        let url = Url::parse(value).map_err(|_| TrackerUrlError::Invalid)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.port_or_known_default() == Some(0)
        {
            return Err(TrackerUrlError::Forbidden);
        }
        match url.host().ok_or(TrackerUrlError::Invalid)? {
            Host::Domain(domain) => {
                if domain == "localhost"
                    || domain.ends_with(".localhost")
                    || domain.ends_with(".local")
                    || domain.ends_with(".internal")
                {
                    return Err(TrackerUrlError::Forbidden);
                }
            }
            Host::Ipv4(ip) => {
                if !self.permits_ip(IpAddr::V4(ip)) {
                    return Err(TrackerUrlError::Forbidden);
                }
            }
            Host::Ipv6(ip) => {
                if !self.permits_ip(IpAddr::V6(ip)) {
                    return Err(TrackerUrlError::Forbidden);
                }
            }
        }
        Ok(url)
    }
}

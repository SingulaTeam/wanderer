//! Reaching the chain and the Control Planes while the guard holds.
//!
//! The guard blocks DNS whenever the tunnel is down, yet that is exactly when
//! the app must reach a Control Plane — and the chain, through the RPC node —
//! to open the next session. So their names are resolved once, while nothing
//! is blocked, and every connection to them goes to those addresses; the
//! guard admits them, and only them. TLS is still checked against the
//! published names, so a wrong address fails rather than reaching someone
//! else.
//!
//! Once the tunnel is up these connections travel through it like all the
//! rest: the addresses route into the TUN.

use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};

use serde::{Deserialize, Serialize};
use singula::StarId;
use singula::chain::{ControlPlane, front};
use wanderer_tunnel::{Endpoint, GuardPolicy};

/// How many addresses are kept per name: enough to survive one being down,
/// few enough that the guard stays narrow.
const ADDRESSES_PER_NAME: usize = 2;

/// Resolved addresses of the RPC node and the Control Planes.
///
/// Serializable, so the app can keep the last one and start from it when
/// names cannot be resolved — when it starts while the guard already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reach {
    /// The RPC node.
    pub rpc: Resolved,
    /// Every Control Plane whose name resolved.
    pub control_planes: Vec<ControlPlaneReach>,
}

/// A Control Plane's addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPlaneReach {
    /// Its Star id.
    #[serde(with = "star_id")]
    pub star: StarId,
    /// Where its published name resolved.
    #[serde(flatten)]
    pub at: Resolved,
    /// Its front's REALITY key, base64url, if it published one: its front
    /// listens at the same addresses, on the front's port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_key: Option<String>,
}

impl ControlPlaneReach {
    /// Where its front listens.
    fn front_endpoints(&self) -> impl Iterator<Item = Endpoint> + '_ {
        let addresses = if self.front_key.is_some() {
            self.at.addresses.as_slice()
        } else {
            &[]
        };
        addresses
            .iter()
            .map(|&ip| Endpoint(SocketAddrV4::new(ip, front::PORT)))
    }
}

/// One name's addresses, and the port they serve on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    /// IPv4 only, as the tunnel is.
    pub addresses: Vec<Ipv4Addr>,
    /// The port.
    pub port: u16,
}

impl Resolved {
    /// Resolves `host` at `port`, keeping a few IPv4 addresses.
    ///
    /// # Errors
    ///
    /// Fails when the name does not resolve to any IPv4 address.
    pub async fn lookup(host: &str, port: u16) -> std::io::Result<Self> {
        let mut addresses: Vec<Ipv4Addr> = tokio::net::lookup_host((host, port))
            .await?
            .filter_map(|a| match a.ip() {
                IpAddr::V4(ip) => Some(ip),
                IpAddr::V6(_) => None,
            })
            .collect();
        addresses.dedup();
        addresses.truncate(ADDRESSES_PER_NAME);
        if addresses.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{host} has no IPv4 address"),
            ));
        }
        Ok(Self { addresses, port })
    }

    /// The addresses, for the SDK's `at` constructors.
    #[must_use]
    pub fn ips(&self) -> Vec<IpAddr> {
        self.addresses.iter().copied().map(IpAddr::V4).collect()
    }

    fn endpoints(&self) -> impl Iterator<Item = Endpoint> + '_ {
        self.addresses
            .iter()
            .map(|&ip| Endpoint(SocketAddrV4::new(ip, self.port)))
    }
}

impl Reach {
    /// Resolves every Control Plane in `control_planes`; one that does not
    /// resolve is left out, as one that is down would be.
    pub async fn control_planes(control_planes: &[ControlPlane]) -> Vec<ControlPlaneReach> {
        let mut resolved = Vec::new();
        for cp in control_planes {
            let port = cp.port.map_or(singula::client::DEFAULT_PORT, u16::from);
            match Resolved::lookup(cp.host.trim(), port).await {
                Ok(at) => resolved.push(ControlPlaneReach {
                    star: cp.star,
                    at,
                    front_key: cp.front().map(|f| f.public_key_base64()),
                }),
                Err(e) => tracing::warn!(star = %cp.star, "Control Plane left out: {e}"),
            }
        }
        resolved
    }

    /// Every Control Plane's Star id.
    #[must_use]
    pub fn stars(&self) -> Vec<StarId> {
        self.control_planes.iter().map(|c| c.star).collect()
    }

    /// The addresses Control Plane `star` is reached at, its front's too.
    #[must_use]
    pub fn addresses(&self, star: StarId) -> &[Ipv4Addr] {
        self.control_planes
            .iter()
            .find(|c| c.star == star)
            .map_or(&[], |c| c.at.addresses.as_slice())
    }

    /// What the guard admits besides the tunnel: these addresses, and the
    /// local network if `allow_lan`.
    ///
    /// The RPC node comes first; Control Planes past the guard's limit are
    /// left out, as the app needs only one of them to answer.
    #[must_use]
    pub fn guard(&self, allow_lan: bool) -> GuardPolicy {
        let mut allowed: Vec<Endpoint> = self.rpc.endpoints().collect();
        for cp in &self.control_planes {
            allowed.extend(cp.at.endpoints());
            allowed.extend(cp.front_endpoints());
        }
        allowed.truncate(wanderer_tunnel::plan::MAX_ALLOWED_ENDPOINTS);
        GuardPolicy { allowed, allow_lan }
    }
}

/// A Star id as the number it is on chain.
mod star_id {
    use serde::{Deserialize as _, Deserializer, Serializer};
    use singula::StarId;

    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `with` passes a reference"
    )]
    pub(super) fn serialize<S: Serializer>(id: &StarId, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(id.0)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<StarId, D::Error> {
        u64::deserialize(d).map(StarId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(last: u8) -> Resolved {
        Resolved {
            addresses: vec![Ipv4Addr::new(203, 0, 113, last)],
            port: 443,
        }
    }

    #[test]
    fn the_guard_admits_the_rpc_first_and_stays_within_its_limit() {
        let reach = Reach {
            rpc: resolved(1),
            control_planes: (0..40)
                .map(|i| ControlPlaneReach {
                    star: StarId(i),
                    at: resolved(2),
                    front_key: None,
                })
                .collect(),
        };
        let guard = reach.guard(true);
        assert_eq!(guard.allowed[0].0, "203.0.113.1:443".parse().unwrap());
        assert_eq!(
            guard.allowed.len(),
            wanderer_tunnel::plan::MAX_ALLOWED_ENDPOINTS
        );
        assert!(guard.allow_lan);
        assert!(guard.validate().is_ok());
    }

    #[test]
    fn reads_back_what_it_kept() {
        let reach = Reach {
            rpc: resolved(1),
            control_planes: vec![ControlPlaneReach {
                star: StarId(7),
                at: resolved(2),
                front_key: Some("DbxXuk87LHpLVR_Nj7YANVZcg9VqeUiPFDm8KajFRww".into()),
            }],
        };
        let kept = serde_json::to_string(&reach).unwrap();
        assert_eq!(serde_json::from_str::<Reach>(&kept).unwrap(), reach);
        // What an older app kept still reads, with no front.
        let old = r#"{"rpc":{"addresses":["203.0.113.1"],"port":443},"control_planes":[{"star":7,"addresses":["203.0.113.2"],"port":443}]}"#;
        assert_eq!(
            serde_json::from_str::<Reach>(old).unwrap().control_planes[0].front_key,
            None
        );
    }

    #[test]
    fn the_guard_admits_a_front_only_where_one_was_published() {
        let cp = |front_key| ControlPlaneReach {
            star: StarId(7),
            at: Resolved {
                addresses: vec![Ipv4Addr::new(203, 0, 113, 2)],
                port: 9090,
            },
            front_key,
        };
        let allowed = |front_key| {
            Reach {
                rpc: resolved(1),
                control_planes: vec![cp(front_key)],
            }
            .guard(false)
            .allowed
        };
        let front: Endpoint = Endpoint("203.0.113.2:443".parse().unwrap());
        assert!(allowed(Some("k".into())).contains(&front));
        assert!(!allowed(None).contains(&front));
    }

    #[tokio::test]
    async fn keeps_ipv4_only() {
        let r = Resolved::lookup("127.0.0.1", 8545).await.unwrap();
        assert_eq!(
            (r.addresses.as_slice(), r.port),
            (&[Ipv4Addr::LOCALHOST][..], 8545)
        );
        assert!(Resolved::lookup("::1", 8545).await.is_err());
    }
}

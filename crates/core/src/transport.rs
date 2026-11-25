//! Which way to reach a session's node, in which order.
//!
//! A node serves each cover over raw TCP with Vision and over xHTTP, on the
//! cover's port — its own, or one shared by all behind a reverse proxy — and
//! the session works under any cover it serves. A cover whose port the node
//! does not name is not tried: an older node names only its choice's. The user
//! picks Vision, xHTTP or Auto; Auto tries what worked last first, then
//! Vision before xHTTP — Vision is faster where it is not blocked — on the
//! cover the node chose, then the other covers.

use std::net::{Ipv4Addr, SocketAddrV4};

use rand::seq::IndexedRandom as _;
use serde::{Deserialize, Serialize};
use singula::client::{Cover, TunnelSettings};
use wanderer_tunnel::{Hop, Reality, Resolver, Transport, TunnelPlan};

/// The transport the user chose in settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Whatever gets through.
    #[default]
    Auto,
    /// Raw TCP with Vision only.
    Vision,
    /// xHTTP only.
    Xhttp,
}

/// A transport, without its path: what the app remembers worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Raw TCP with Vision.
    Vision,
    /// xHTTP.
    Xhttp,
}

impl From<&Transport> for Kind {
    fn from(t: &Transport) -> Self {
        match t {
            Transport::Vision => Self::Vision,
            Transport::Xhttp { .. } => Self::Xhttp,
        }
    }
}

/// What a node offers a session: the parts of its tunnel settings that
/// decide how to reach it.
#[derive(Clone, PartialEq, Eq)]
pub struct Offer {
    /// The VLESS user id.
    pub uuid: String,
    /// Whether the node serves Vision over raw TCP.
    pub vision: bool,
    /// The node's REALITY public key.
    pub public_key: String,
    /// The TLS fingerprint the node asks for.
    pub fingerprint: String,
    /// Its covers, the one it chose for the session first.
    pub covers: Vec<CoverOffer>,
}

/// One cover a node serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverOffer {
    /// Its preset name.
    pub preset: String,
    /// `(server name, short id)` pairs.
    pub names: Vec<(String, String)>,
    /// Its xHTTP path, if served over xHTTP.
    pub xhttp_path: Option<String>,
    /// The port it is reached on.
    pub port: u16,
}

/// One way to try a node: a plan for the daemon, and the cover it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The cover's preset name, for display.
    pub cover: String,
    /// The plan.
    pub plan: TunnelPlan,
}

/// The flow a node sends when it serves Vision.
const VISION_FLOW: &str = "xtls-rprx-vision";
/// Used when the node names a fingerprint the daemon would refuse.
const FALLBACK_FINGERPRINT: &str = "chrome";

impl From<&TunnelSettings> for Offer {
    fn from(s: &TunnelSettings) -> Self {
        let cover = |c: &Cover, port: u16| CoverOffer {
            preset: c.preset.clone(),
            names: c.names.clone(),
            xhttp_path: c.xhttp_path.clone(),
            port,
        };
        // The session's cover list may or may not repeat the chosen one, and
        // the chosen one is reached on the session's port.
        let mut covers = vec![cover(&s.cover, s.cover.port.unwrap_or(s.port))];
        covers.extend(
            s.covers
                .iter()
                .filter(|c| c.preset != s.cover.preset)
                .filter_map(|c| Some(cover(c, c.port?))),
        );
        Self {
            uuid: s.uuid.clone(),
            vision: s.flow == VISION_FLOW,
            public_key: s.reality_public_key.clone(),
            fingerprint: s.fingerprint.clone(),
            covers,
        }
    }
}

impl Offer {
    /// Every way to try the node at `node`, best first.
    ///
    /// `worked` is the kind that carried traffic last time; under
    /// [`Choice::Auto`] it goes first.
    #[must_use]
    pub fn plans(&self, node: Ipv4Addr, choice: Choice, worked: Option<Kind>) -> Vec<Candidate> {
        let mut kinds = match choice {
            Choice::Auto => vec![Kind::Vision, Kind::Xhttp],
            Choice::Vision => vec![Kind::Vision],
            Choice::Xhttp => vec![Kind::Xhttp],
        };
        if choice == Choice::Auto && worked == Some(Kind::Xhttp) {
            kinds.reverse();
        }

        let mut plans = Vec::new();
        for cover in &self.covers {
            let node = SocketAddrV4::new(node, cover.port);
            for &kind in &kinds {
                let transport = match (kind, &cover.xhttp_path) {
                    (Kind::Vision, _) if self.vision => Transport::Vision,
                    (Kind::Xhttp, Some(path)) => Transport::Xhttp { path: path.clone() },
                    _ => continue,
                };
                if let Some(reality) = self.reality(cover) {
                    plans.push(Candidate {
                        cover: cover.preset.clone(),
                        plan: TunnelPlan {
                            node,
                            uuid: self.uuid.clone(),
                            reality,
                            transport,
                            onward: Vec::new(),
                            early_exits: Vec::new(),
                            resolver: Resolver::default(),
                        },
                    });
                }
            }
        }
        plans
    }

    /// The node at `node` as a chain's leg: raw TCP without a flow, under
    /// the cover it chose. The leg is inside the hop before it, so no cover
    /// shows on the wire and there is nothing to fall back between.
    #[must_use]
    pub fn hop(&self, node: Ipv4Addr) -> Option<Hop> {
        let cover = self.covers.first()?;
        let reality = self.reality(cover)?;
        Some(Hop {
            node: SocketAddrV4::new(node, cover.port),
            uuid: self.uuid.clone(),
            reality,
        })
    }

    /// One of the cover's server names, at random: each connection may
    /// present a different one, so no single name carries all of a node's
    /// traffic.
    fn reality(&self, cover: &CoverOffer) -> Option<Reality> {
        let (server_name, short_id) = cover.names.choose(&mut rand::rng())?;
        let known = wanderer_tunnel::plan::FINGERPRINTS.contains(&self.fingerprint.as_str());
        Some(Reality {
            public_key: self.public_key.clone(),
            short_id: short_id.clone(),
            server_name: server_name.clone(),
            fingerprint: if known {
                self.fingerprint.clone()
            } else {
                FALLBACK_FINGERPRINT.to_owned()
            },
        })
    }
}

impl std::fmt::Debug for Offer {
    // The uuid is the session's credential.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offer")
            .field("vision", &self.vision)
            .field("covers", &self.covers)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> Offer {
        let cover = |preset: &str, path: Option<&str>, port| CoverOffer {
            preset: preset.into(),
            names: vec![(format!("{preset}.example"), "ab".into())],
            xhttp_path: path.map(Into::into),
            port,
        };
        Offer {
            uuid: "7316e3b4-53eb-41da-90d9-23bbde8e1cc6".into(),
            vision: true,
            public_key: "Vfv6lG2gjLQSbkLJdcH72Eo7zW2LrNRhAyrvU_YPJGQ".into(),
            fingerprint: "chrome".into(),
            covers: vec![
                cover("ru", Some("/a"), 8443),
                cover("default", None, 443),
                cover("cn", Some("/c"), 2053),
            ],
        }
    }

    fn order(plans: &[Candidate]) -> Vec<(String, Kind)> {
        plans
            .iter()
            .map(|c| (c.cover.clone(), Kind::from(&c.plan.transport)))
            .collect()
    }

    const NODE: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 7);

    #[test]
    fn auto_tries_vision_then_xhttp_on_each_cover_in_turn() {
        let plans = offer().plans(NODE, Choice::Auto, None);
        let want = [
            ("ru", Kind::Vision),
            ("ru", Kind::Xhttp),
            ("default", Kind::Vision),
            ("cn", Kind::Vision),
            ("cn", Kind::Xhttp),
        ];
        assert_eq!(order(&plans), want.map(|(n, k)| (n.to_owned(), k)));
        assert!(plans.iter().all(|c| c.plan.validate().is_ok()));
        assert_eq!(plans[3].plan.reality.server_name, "cn.example");
        assert_eq!(
            plans.iter().map(|c| c.plan.node.port()).collect::<Vec<_>>(),
            [8443, 8443, 443, 2053, 2053],
            "each cover on its own port"
        );
    }

    #[test]
    fn auto_tries_what_worked_first() {
        let plans = offer().plans(NODE, Choice::Auto, Some(Kind::Xhttp));
        assert_eq!(
            order(&plans)[..2],
            [
                ("ru".to_owned(), Kind::Xhttp),
                ("ru".to_owned(), Kind::Vision)
            ]
        );
    }

    #[test]
    fn a_fixed_choice_keeps_to_its_transport() {
        let xhttp = offer().plans(NODE, Choice::Xhttp, None);
        assert_eq!(xhttp.len(), 2, "only the covers served over xHTTP");
        assert!(
            xhttp
                .iter()
                .all(|c| Kind::from(&c.plan.transport) == Kind::Xhttp)
        );

        let mut no_vision = offer();
        no_vision.vision = false;
        assert_eq!(no_vision.plans(NODE, Choice::Vision, None), Vec::new());
    }

    #[test]
    fn an_unknown_fingerprint_falls_back() {
        let mut o = offer();
        o.fingerprint = "hellochrome_auto".into();
        let plans = o.plans(NODE, Choice::Auto, None);
        assert_eq!(plans[0].plan.reality.fingerprint, FALLBACK_FINGERPRINT);
    }
}

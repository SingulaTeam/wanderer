//! Which nodes a chain runs through.
//!
//! The person asks for a route: a country, or any, for each hop, entry first.
//! Every Control Plane lists the nodes it holds, and a hop is a node named at
//! the Control Plane that holds it — an anonymous chain's leg is never routed
//! to another operator's Control Plane, and a named node is served or
//! refused, never swapped.
//!
//! Unless the person allows otherwise, no two hops share an operator: the
//! owner of a hop's node and the owner of the Control Plane that admits it
//! are both that hop's operators, since either sees its session. Two hops
//! with one operator are one hop to that operator. Only the client can check
//! this — it is the one party that sees every hop.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use rand::seq::SliceRandom as _;
use serde::Serialize;
use singula::client::{Country, Singula};
use singula::{Address, StarId};
use tokio::task::JoinSet;

/// How long a Control Plane has to list its nodes.
const LIST_TIMEOUT: Duration = Duration::from_secs(8);

/// The most hops a route may have.
pub const MAX_HOPS: usize = wanderer_tunnel::plan::MAX_HOPS;

/// What the person asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// A country for each hop, entry first; `None` for any.
    pub hops: Vec<Option<Country>>,
    /// Whether hops may share an operator.
    pub same_operator: bool,
}

/// A node on offer, with who runs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Offered {
    /// The Control Plane that holds it.
    pub cp: StarId,
    /// The owner of that Control Plane.
    pub cp_owner: Address,
    /// The node.
    pub node: StarId,
    /// Where it says it is.
    pub country: Option<Country>,
    /// How busy it is, from 0 to 1.
    pub load: f32,
    /// The owner of the node.
    pub node_owner: Address,
}

/// One hop of a chosen route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    /// The Control Plane to open the hop's session at.
    pub cp: StarId,
    /// The node to name.
    pub node: StarId,
}

/// Why no route could be built: the first hop that could not be filled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gap {
    /// The hop, counting from 1.
    pub hop: u8,
    /// Its country, if one was asked for.
    pub country: Option<String>,
    /// Whether there are free nodes there, all of operators the route
    /// already uses.
    pub only_same_operator: bool,
}

/// The nodes for `wanted`, the least loaded that fit; nodes in `avoid` failed
/// lately and are taken only when nothing else fits.
///
/// # Errors
///
/// Returns the [`Gap`] that stopped it.
pub fn choose<H: std::hash::BuildHasher>(
    wanted: &Wanted,
    offered: &[Offered],
    avoid: &HashSet<StarId, H>,
) -> Result<Vec<Pick>, Gap> {
    let mut order: Vec<&Offered> = offered.iter().collect();
    // Shuffled first, so nodes that rank the same take turns.
    order.shuffle(&mut rand::rng());
    order.sort_by(|a, b| {
        let failed = |o: &Offered| avoid.contains(&o.node);
        failed(a)
            .cmp(&failed(b))
            .then(rank(a.load).cmp(&rank(b.load)))
    });

    let mut route = Vec::new();
    let mut deepest = 0;
    if fill(wanted, &order, &mut route, &mut deepest) {
        return Ok(route
            .iter()
            .map(|o| Pick {
                cp: o.cp,
                node: o.node,
            })
            .collect());
    }
    let country = wanted.hops.get(deepest).copied().flatten();
    let in_country = |o: &&&Offered| country.is_none() || o.country == country;
    Err(Gap {
        hop: u8::try_from(deepest + 1).unwrap_or(u8::MAX),
        country: country.map(|c| c.as_str().to_owned()),
        only_same_operator: !wanted.same_operator && order.iter().any(|o| in_country(&o)),
    })
}

/// Load in steps of a tenth: closer than that is noise, and the shuffle
/// decides.
fn rank(load: f32) -> u8 {
    // Load is clamped to 0..=1 by the SDK.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "0..=10"
    )]
    let step = (load * 10.0).round() as u8;
    step
}

/// Fills the hops after `route` depth first; `deepest` is the furthest hop
/// that found nothing.
fn fill<'a>(
    wanted: &Wanted,
    order: &[&'a Offered],
    route: &mut Vec<&'a Offered>,
    deepest: &mut usize,
) -> bool {
    let hop = route.len();
    let Some(&country) = wanted.hops.get(hop) else {
        return true;
    };
    for &o in order {
        let fits = (country.is_none() || o.country == country)
            && route.iter().all(|r| r.node != o.node)
            && (wanted.same_operator || route.iter().all(|r| apart(r, o)));
        if !fits {
            continue;
        }
        route.push(o);
        if fill(wanted, order, route, deepest) {
            return true;
        }
        route.pop();
    }
    *deepest = (*deepest).max(hop);
    false
}

/// Whether two hops have no operator in common.
fn apart(a: &Offered, b: &Offered) -> bool {
    let theirs = [b.cp_owner, b.node_owner];
    !theirs.contains(&a.cp_owner) && !theirs.contains(&a.node_owner)
}

/// Stars' owners, read once: an owner is fixed at registration.
#[derive(Debug, Default)]
pub struct Owners(Mutex<HashMap<StarId, Address>>);

impl Owners {
    fn known(&self, star: StarId) -> Option<Address> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&star)
            .copied()
    }

    /// The owner of `star`, or `None` if it cannot be read.
    pub async fn of<R, S>(&self, singula: &Singula<R, S>, star: StarId) -> Option<Address>
    where
        R: singula::chain::Rpc + 'static,
        S: singula::client::store::Store + 'static,
    {
        if let Some(owner) = self.known(star) {
            return Some(owner);
        }
        let owner = singula.chain().star(star).await.ok()??.owner;
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(star, owner);
        Some(owner)
    }
}

/// Every node the Control Planes `cps` hold and can serve now, with their
/// owners. A Control Plane that does not answer in time, or a Star whose
/// owner cannot be read, is left out.
pub async fn survey<R, S>(singula: &Singula<R, S>, owners: &Owners, cps: &[StarId]) -> Vec<Offered>
where
    R: singula::chain::Rpc + 'static,
    S: singula::client::store::Store + 'static,
{
    let mut lists = JoinSet::new();
    for &cp in cps {
        let singula = singula.clone();
        lists.spawn(async move {
            let client = singula.control_plane(cp).await.ok()?;
            let nodes = tokio::time::timeout(LIST_TIMEOUT, client.list_nodes())
                .await
                .ok()?
                .ok()?;
            Some((cp, nodes))
        });
    }
    let mut offered = Vec::new();
    while let Some(listed) = lists.join_next().await {
        let Ok(Some((cp, nodes))) = listed else {
            continue;
        };
        let Some(cp_owner) = owners.of(singula, cp).await else {
            continue;
        };
        for node in nodes.into_iter().filter(|n| n.available) {
            let Some(node_owner) = owners.of(singula, node.star).await else {
                continue;
            };
            offered.push(Offered {
                cp,
                cp_owner,
                node: node.star,
                country: node.country,
                load: node.load,
                node_owner,
            });
        }
    }
    offered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_wraps, reason = "as `Wanted` holds it")]
    fn country(c: &str) -> Option<Country> {
        Some(c.parse().unwrap())
    }

    /// Node `node` in `c`, held by CP `cp`; owners by number.
    fn offered(cp: u64, cp_owner: u8, node: u64, node_owner: u8, c: &str, load: f32) -> Offered {
        Offered {
            cp: StarId(cp),
            cp_owner: Address([cp_owner; 20]),
            node: StarId(node),
            country: country(c),
            load,
            node_owner: Address([node_owner; 20]),
        }
    }

    fn wanted(hops: &[Option<&str>], same_operator: bool) -> Wanted {
        Wanted {
            hops: hops.iter().map(|c| c.and_then(country)).collect(),
            same_operator,
        }
    }

    fn nodes(route: &[Pick]) -> Vec<u64> {
        route.iter().map(|p| p.node.0).collect()
    }

    #[test]
    fn each_hop_in_its_country_with_its_own_operator() {
        let offered = [
            offered(1, 1, 10, 1, "DE", 0.1),
            offered(2, 2, 20, 2, "NL", 0.1),
            offered(2, 2, 21, 2, "DE", 0.0),
        ];
        let route = choose(
            &wanted(&[Some("DE"), Some("NL")], false),
            &offered,
            &HashSet::new(),
        );
        // 21 is less loaded, but shares operator 2 with the only Dutch node.
        assert_eq!(nodes(&route.unwrap()), [10, 20]);
    }

    #[test]
    fn a_control_planes_owner_is_an_operator_too() {
        // Different node owners, one Control Plane owner.
        let offered = [
            offered(1, 9, 10, 1, "DE", 0.0),
            offered(2, 9, 20, 2, "NL", 0.0),
        ];
        let gap = choose(&wanted(&[None, None], false), &offered, &HashSet::new()).unwrap_err();
        assert_eq!(gap.hop, 2);
        assert!(gap.only_same_operator);
        assert!(choose(&wanted(&[None, None], true), &offered, &HashSet::new()).is_ok());
    }

    #[test]
    fn never_the_same_node_twice() {
        let offered = [offered(1, 1, 10, 1, "DE", 0.0)];
        let gap = choose(&wanted(&[None, None], true), &offered, &HashSet::new()).unwrap_err();
        assert_eq!(gap.hop, 2);
    }

    #[test]
    fn says_which_hop_has_nothing() {
        let offered = [
            offered(1, 1, 10, 1, "DE", 0.0),
            offered(2, 2, 20, 2, "DE", 0.0),
        ];
        let gap = choose(
            &wanted(&[Some("DE"), None, Some("JP")], false),
            &offered,
            &HashSet::new(),
        )
        .unwrap_err();
        assert_eq!(gap.hop, 3);
        assert_eq!(gap.country.as_deref(), Some("JP"));
        assert!(!gap.only_same_operator, "there is nothing in Japan at all");
    }

    #[test]
    fn backtracks_when_a_greedy_entry_blocks_the_exit() {
        // The least loaded entry, 11, is the only Dutch node's operator.
        let offered = [
            offered(1, 1, 11, 2, "DE", 0.0),
            offered(1, 1, 10, 1, "DE", 0.5),
            offered(3, 3, 20, 2, "NL", 0.0),
        ];
        let route = choose(
            &wanted(&[Some("DE"), Some("NL")], false),
            &offered,
            &HashSet::new(),
        );
        assert_eq!(nodes(&route.unwrap()), [10, 20]);
    }

    #[test]
    fn failed_nodes_go_last_but_still_count() {
        let offered = [
            offered(1, 1, 10, 1, "DE", 0.0),
            offered(2, 2, 20, 2, "DE", 0.9),
        ];
        let avoid = HashSet::from([StarId(10)]);
        let route = choose(&wanted(&[None], false), &offered, &avoid).unwrap();
        assert_eq!(nodes(&route), [20]);
        let avoid = HashSet::from([StarId(10), StarId(20)]);
        assert!(choose(&wanted(&[None], false), &offered, &avoid).is_ok());
    }
}

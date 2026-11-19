//! Which of the store's listings each shelf of Customize offers in its Workshop. Every
//! client asks the store for `kinds` and shows what `holds` keeps, so a shelf shows the
//! same listings everywhere.

use super::{Kind, Listing};

/// A shelf of Customize that has a Workshop. Tools has none: the store lists no tools.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    /// MCP servers: the store has no kind of its own for them, so it is the plugins that
    /// bring one, as a listing's Connectors tab shows them.
    Connectors,
    Skills,
    #[default]
    Plugins,
    Agents,
}

impl Scope {
    pub const ALL: [Scope; 4] = [
        Scope::Connectors,
        Scope::Skills,
        Scope::Plugins,
        Scope::Agents,
    ];

    /// The kinds the store is asked for.
    pub fn kinds(self) -> Vec<Kind> {
        match self {
            Self::Connectors | Self::Plugins => vec![Kind::Plugin],
            Self::Skills => vec![Kind::Skill, Kind::Pack],
            Self::Agents => vec![Kind::Agent],
        }
    }

    /// Whether `listing` is offered on this shelf.
    pub fn holds(self, listing: &Listing) -> bool {
        self.kinds().contains(&listing.kind)
            && (self != Self::Connectors || !listing.connectors.is_empty())
    }

    /// What the shelf offers, in a lower case plural word.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Connectors => "connectors",
            Self::Skills => "skills",
            Self::Plugins => "plugins",
            Self::Agents => "agents",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Connector, ConnectorState, Runs};

    fn listing(kind: Kind, connectors: usize) -> Listing {
        let connector = Connector {
            name: "search".into(),
            runs: Runs::Url("https://example.com/mcp".into()),
            state: ConnectorState::NotAdded,
        };
        Listing {
            kind,
            connectors: vec![connector; connectors],
            ..Listing::default()
        }
    }

    #[test]
    fn each_shelf_holds_its_own_listings() {
        let plain = listing(Kind::Plugin, 0);
        let server = listing(Kind::Plugin, 1);
        let (skill, pack, agent) = (
            listing(Kind::Skill, 0),
            listing(Kind::Pack, 0),
            listing(Kind::Agent, 0),
        );
        let held = |scope: Scope| {
            [&plain, &server, &skill, &pack, &agent].map(|listing| scope.holds(listing))
        };
        assert_eq!(held(Scope::Connectors), [false, true, false, false, false]);
        assert_eq!(held(Scope::Plugins), [true, true, false, false, false]);
        assert_eq!(held(Scope::Skills), [false, false, true, true, false]);
        assert_eq!(held(Scope::Agents), [false, false, false, false, true]);
    }

    #[test]
    fn a_shelf_asks_the_store_only_for_what_it_holds() {
        for scope in Scope::ALL {
            for kind in [Kind::Plugin, Kind::Skill, Kind::Pack, Kind::Agent] {
                if scope.holds(&listing(kind, 1)) {
                    assert!(scope.kinds().contains(&kind), "{scope:?} {kind:?}");
                }
            }
        }
    }
}

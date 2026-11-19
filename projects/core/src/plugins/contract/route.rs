//! Which plugin a host goes to: the user's choice if that plugin serves the host, else
//! the highest tier, then the name. Every client shows the same answer.

use std::sync::Arc;

use dusk_protocol::plugin::Tier;

use super::Served;

/// The hosts `served` serves for `contract`.
pub fn hosts<'a>(served: &'a Served, contract: &str) -> &'a [String] {
    match contract {
        "forge" => served
            .provides
            .forge
            .as_ref()
            .map_or(&[][..], |forge| &forge.hosts),
        _ => &[],
    }
}

fn rank(tier: Tier) -> u8 {
    match tier {
        Tier::Official => 0,
        Tier::Verified => 1,
        Tier::Community => 2,
        Tier::Unverified => 3,
        Tier::Local => 4,
    }
}

/// The plugin of `served` that takes `host` of `contract`.
pub fn route<'a>(
    served: &'a [Arc<Served>],
    chosen: Option<&str>,
    contract: &str,
    host: &str,
) -> Option<&'a Arc<Served>> {
    let serves = |plugin: &&Arc<Served>| hosts(plugin, contract).iter().any(|name| name == host);
    chosen
        .and_then(|name| {
            served
                .iter()
                .filter(serves)
                .find(|plugin| plugin.plugin == name)
        })
        .or_else(|| {
            served
                .iter()
                .filter(serves)
                .min_by(|a, b| (rank(a.tier), &a.plugin).cmp(&(rank(b.tier), &b.plugin)))
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use dusk_protocol::plugin::{ForgeProvides, Permissions, Provides};

    use super::*;
    use crate::{
        init::paths::Paths,
        plugins::{Isolation, store::Record},
    };

    fn served(plugin: &str, tier: Tier, hosts: &[&str], isolation: &Isolation) -> Arc<Served> {
        let provides = Provides {
            command: Vec::new(),
            forge: Some(ForgeProvides {
                versions: vec![1],
                hosts: hosts.iter().map(|host| (*host).to_owned()).collect(),
            }),
            ..Provides::default()
        };
        Arc::new(Served::new(
            plugin.into(),
            tier,
            provides,
            Vec::new(),
            isolation.clone(),
        ))
    }

    fn name(found: Option<&Arc<Served>>) -> Option<&str> {
        found.map(|served| served.plugin.as_str())
    }

    #[test]
    fn the_choice_wins_then_the_tier_then_the_name() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/route-tests")
            .join(std::process::id().to_string());
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.join("config"), root.join("workspace"));
        let folder = paths.rules_root().join("plugins/p");
        let data = paths.config_root().join("plugin-data/p");
        for dir in [&folder, &data] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let Ok(isolation) = Isolation::new(
            &paths,
            &folder,
            &data,
            &Permissions::default(),
            Vec::new(),
            None,
        ) else {
            eprintln!("no sandbox on this system; skipped");
            return;
        };
        let all = [
            served("zed", Tier::Official, &["github.com"], &isolation),
            served(
                "alpha",
                Tier::Community,
                &["github.com", "ghe.example.com"],
                &isolation,
            ),
            served("beta", Tier::Community, &["github.com"], &isolation),
            served("loc", Tier::Local, &["gitea.example.com"], &isolation),
        ];
        // The tier, then the name.
        assert_eq!(name(route(&all, None, "forge", "github.com")), Some("zed"));
        assert_eq!(
            name(route(&all[1..], None, "forge", "github.com")),
            Some("alpha")
        );
        // The choice, when that plugin serves the host.
        assert_eq!(
            name(route(&all, Some("beta"), "forge", "github.com")),
            Some("beta")
        );
        // A choice of a plugin that does not serve it, or is off, is ignored.
        assert_eq!(
            name(route(&all, Some("loc"), "forge", "github.com")),
            Some("zed")
        );
        assert_eq!(
            name(route(&all, Some("gone"), "forge", "github.com")),
            Some("zed")
        );
        // A host nobody serves, or a contract nobody knows, goes to the built-in path.
        assert!(route(&all, None, "forge", "gitlab.com").is_none());
        assert!(route(&all, None, "mail", "github.com").is_none());

        let contracts = crate::plugins::contract::Contracts::default();
        contracts.set(
            all.iter()
                .map(|plugin| {
                    Served::new(
                        plugin.plugin.clone(),
                        plugin.tier,
                        plugin.provides.clone(),
                        Vec::new(),
                        isolation.clone(),
                    )
                })
                .collect(),
        );
        assert_eq!(
            contracts.serving("forge", "github.com"),
            ["alpha", "beta", "zed"]
        );
        assert_eq!(
            contracts
                .route("forge", "ghe.example.com", None)
                .map(|s| s.plugin.clone()),
            Some("alpha".to_owned())
        );

        // Tiers come from the store's record: not installed is local.
        assert_eq!(Record::default().tier(&paths, "zed"), Tier::Local);
        let _ = std::fs::remove_dir_all(&root);
    }
}

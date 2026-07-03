//! Fleet roster: each agent's identity — id (board owner / session name),
//! full display name, lane (the board role markers `@Coordinator`/`@Builder`/
//! `@QA`/`@Ops` and the skills role filter), a dev-team specialty, and an
//! optional per-agent model override from config (`agent_model_<id>` in
//! smartagent.conf) so lanes can run on different providers/models.

pub struct Profile {
    pub id: &'static str,
    pub full_name: &'static str,
    /// Board lane — the role vocabulary skills/tasks already file under.
    pub lane: &'static str,
    pub specialty: &'static str,
}

/// The software-dev team. Ids are the pioneers' full names (kebab-case);
/// lanes keep the established Coordinator/Builder/QA/Ops vocabulary.
pub const ROSTER: &[Profile] = &[
    Profile {
        id: "linus-torvalds",
        full_name: "Linus Torvalds",
        lane: "Coordinator",
        specialty: "Team Lead",
    },
    Profile {
        id: "ada-lovelace",
        full_name: "Ada Lovelace",
        lane: "Builder",
        specialty: "Backend Expert",
    },
    Profile {
        id: "dennis-ritchie",
        full_name: "Dennis Ritchie",
        lane: "Builder",
        specialty: "Systems Expert",
    },
    Profile {
        id: "steve-wozniak",
        full_name: "Steve Wozniak",
        lane: "Builder",
        specialty: "Frontend Expert",
    },
    Profile {
        id: "margaret-hamilton",
        full_name: "Margaret Hamilton",
        lane: "Builder",
        specialty: "Database Expert",
    },
    Profile {
        id: "grace-hopper",
        full_name: "Grace Hopper",
        lane: "QA",
        specialty: "QA Lead",
    },
    Profile {
        id: "alan-turing",
        full_name: "Alan Turing",
        lane: "QA",
        specialty: "Verification Expert",
    },
    Profile {
        id: "ken-thompson",
        full_name: "Ken Thompson",
        lane: "Ops",
        specialty: "Infrastructure Expert",
    },
    Profile {
        id: "jeeves",
        full_name: "Jeeves",
        lane: "Assistant",
        specialty: "Chat",
    },
];

pub fn profile(id: &str) -> Option<&'static Profile> {
    ROSTER.iter().find(|p| p.id == id)
}

/// Board-lane role for an agent — drives the `@Role` review markers and the
/// skills role filter. Legacy short names keep their old lanes so old board
/// rows and sessions still resolve.
pub(crate) fn role_of(name: &str) -> &'static str {
    if let Some(p) = profile(name) {
        return p.lane;
    }
    match name {
        "linus" | "main" => "Coordinator",
        "ada" | "dennis" | "woz" | "builder" => "Builder",
        "grace" | "turing" | "qa" => "QA",
        "ken" | "margaret" | "ops" => "Ops",
        "jeeves" => "Assistant",
        _ => "Agent",
    }
}

/// Human-facing role label for the sidebar/TSV: specialty when known.
pub(crate) fn display_role(name: &str) -> String {
    match profile(name) {
        Some(p) => p.specialty.to_string(),
        None => role_of(name).to_string(),
    }
}

/// Per-agent model override: `agent_model_<id>` (dashes → underscores) in
/// smartagent.conf, e.g. `agent_model_grace_hopper = codex/gpt-5.4-mini`.
/// None → the pi default model.
pub(crate) fn model_for(cfg: &semdb::config::Config, name: &str) -> Option<String> {
    let key = format!("agent_model_{}", name.replace('-', "_"));
    cfg.resolve(&key, "", None)
}

/// Identity line prepended to the agent's system prompt.
pub(crate) fn identity_prompt(name: &str) -> Option<String> {
    let p = profile(name)?;
    Some(format!(
        "You are {}, the fleet's {} ({} lane). Prefer tasks matching your specialty; route work outside it to the right teammate via the review column.",
        p.full_name, p.specialty, p.lane
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_name_ids_resolve_lanes() {
        assert_eq!(role_of("linus-torvalds"), "Coordinator");
        assert_eq!(role_of("margaret-hamilton"), "Builder");
        assert_eq!(role_of("alan-turing"), "QA");
        assert_eq!(role_of("ken-thompson"), "Ops");
    }

    #[test]
    fn legacy_short_names_keep_old_lanes() {
        assert_eq!(role_of("linus"), "Coordinator");
        assert_eq!(role_of("woz"), "Builder");
        assert_eq!(role_of("unknown"), "Agent");
    }

    #[test]
    fn display_role_prefers_specialty() {
        assert_eq!(display_role("steve-wozniak"), "Frontend Expert");
        assert_eq!(display_role("qa"), "QA");
    }

    #[test]
    fn identity_prompt_names_the_agent() {
        let p = identity_prompt("grace-hopper").unwrap();
        assert!(p.contains("Grace Hopper") && p.contains("QA Lead"));
        assert!(identity_prompt("nobody").is_none());
    }
}

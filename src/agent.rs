//! Agent mode: AI coding agents run socli with nobody at the terminal, and read its output as data.

use std::path::Path;
use std::sync::OnceLock;

/**
Each agent's name and the variables that mark it, after `AI_AGENT` and in vercel/detect-agent's order.
`CURSOR_TRACE_ID` and `REPL_ID` are left out on purpose: human terminals in Cursor and Replit carry them.
*/
const AGENTS: &[(&str, &[&str])] = &[
    ("cursor-cli", &["CURSOR_AGENT"]),
    ("gemini", &["GEMINI_CLI"]),
    ("codex", &["CODEX_SANDBOX", "CODEX_CI", "CODEX_THREAD_ID"]),
    ("antigravity", &["ANTIGRAVITY_AGENT"]),
    ("augment-cli", &["AUGMENT_AGENT"]),
    ("opencode", &["OPENCODE_CLIENT"]),
    ("claude", &["CLAUDECODE", "CLAUDE_CODE"]),
    (
        "github-copilot",
        &["COPILOT_MODEL", "COPILOT_ALLOW_ALL", "COPILOT_GITHUB_TOKEN"],
    ),
];

/// The agent running socli: `AI_AGENT`'s value, an agent's own variable, or devin's `/opt/.devin`.
pub fn detect(env: &dyn Fn(&str) -> Option<String>, devin: bool) -> Option<String> {
    let set = |key: &str| env(key).filter(|value| !value.is_empty());
    if let Some(name) = set("AI_AGENT") {
        return Some(name);
    }
    for (name, keys) in AGENTS {
        let cursor_exec = *name == "cursor-cli"
            && set("CURSOR_EXTENSION_HOST_ROLE").as_deref() == Some("agent-exec");
        if cursor_exec || keys.iter().any(|key| set(key).is_some()) {
            return Some((*name).to_string());
        }
    }
    devin.then(|| "devin".to_string())
}

/// Agent mode for this process: `SOCLI_AGENT=1` forces it, `SOCLI_AGENT=0` turns it off, else `detect`.
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| {
        active_from(
            &|key| std::env::var(key).ok(),
            Path::new("/opt/.devin").exists(),
        )
    })
}

fn active_from(env: &dyn Fn(&str) -> Option<String>, devin: bool) -> bool {
    match env("SOCLI_AGENT")
        .filter(|value| !value.is_empty())
        .as_deref()
    {
        Some("1") => true,
        Some("0") => false,
        _ => detect(env, devin).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn each_variable_names_its_agent() {
        for (key, value, agent) in [
            ("AI_AGENT", "my-agent", "my-agent"),
            ("CURSOR_AGENT", "1", "cursor-cli"),
            ("CURSOR_EXTENSION_HOST_ROLE", "agent-exec", "cursor-cli"),
            ("GEMINI_CLI", "1", "gemini"),
            ("CODEX_SANDBOX", "seatbelt", "codex"),
            ("CODEX_CI", "1", "codex"),
            ("CODEX_THREAD_ID", "t1", "codex"),
            ("ANTIGRAVITY_AGENT", "1", "antigravity"),
            ("AUGMENT_AGENT", "1", "augment-cli"),
            ("OPENCODE_CLIENT", "cli", "opencode"),
            ("CLAUDECODE", "1", "claude"),
            ("CLAUDE_CODE", "1", "claude"),
            ("COPILOT_MODEL", "gpt", "github-copilot"),
            ("COPILOT_ALLOW_ALL", "1", "github-copilot"),
            ("COPILOT_GITHUB_TOKEN", "t", "github-copilot"),
        ] {
            assert_eq!(
                detect(&with(&[(key, value)]), false).as_deref(),
                Some(agent),
                "{key}"
            );
        }
        assert_eq!(detect(&with(&[]), true).as_deref(), Some("devin"));
        assert_eq!(detect(&with(&[]), false), None);
    }

    #[test]
    fn ai_agent_comes_first_and_empty_values_are_unset() {
        let env = with(&[("CLAUDECODE", "1"), ("AI_AGENT", "amp")]);
        assert_eq!(detect(&env, false).as_deref(), Some("amp"));
        let env = with(&[("AI_AGENT", ""), ("CLAUDECODE", "1")]);
        assert_eq!(detect(&env, false).as_deref(), Some("claude"));
        assert_eq!(detect(&with(&[("CLAUDECODE", "")]), false), None);
        let env = with(&[("CURSOR_EXTENSION_HOST_ROLE", "renderer")]);
        assert_eq!(detect(&env, false), None);
    }

    #[test]
    fn human_terminals_in_cursor_and_replit_are_not_agents() {
        assert_eq!(detect(&with(&[("CURSOR_TRACE_ID", "abc")]), false), None);
        assert_eq!(detect(&with(&[("REPL_ID", "abc")]), false), None);
    }

    #[test]
    fn socli_agent_overrides_detection() {
        assert!(active_from(&with(&[("SOCLI_AGENT", "1")]), false));
        assert!(!active_from(
            &with(&[("SOCLI_AGENT", "0"), ("CLAUDECODE", "1")]),
            true
        ));
        assert!(active_from(
            &with(&[("SOCLI_AGENT", ""), ("CLAUDECODE", "1")]),
            false
        ));
        assert!(!active_from(&with(&[]), false));
    }
}

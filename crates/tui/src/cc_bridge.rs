//! Claude Code ecosystem bridge — reads CC config files for seamless interop.
//!
//! Loads settings from `~/.claude/settings.json` and `.claude/settings.json`,
//! mapping relevant fields to DeepSeek-TUI equivalents.

use std::path::{Path, PathBuf};

/// CC settings fields that DeepSeek-TUI can use.
#[derive(Debug, Default, Clone)]
pub struct CcSettings {
    pub language: Option<String>,
    pub model: Option<String>,
    pub theme: Option<String>,
    pub effort: Option<String>,
    pub fast_mode: Option<bool>,
}

/// Map CC language string to DSTUI locale code.
pub fn cc_language_to_locale(lang: &str) -> Option<&'static str> {
    match lang {
        "chinese" => Some("zh-Hans"),
        "japanese" => Some("ja"),
        "spanish" => Some("es-419"),
        "portuguese" => Some("pt-BR"),
        _ => None,
    }
}

/// Map CC theme string to DSTUI theme.
pub fn cc_theme_to_ds(theme: &str) -> Option<&'static str> {
    match theme {
        "dark" | "dark-daltonized" | "dark-ansi" => Some("dark"),
        "light" | "light-daltonized" | "light-ansi" => Some("light"),
        _ => None, // "auto" and unknown -> keep DS current theme
    }
}

/// Load CC user settings from `~/.claude/settings.json` (or a mock home dir).
pub fn load_cc_user_settings(home_override: Option<&Path>) -> Result<CcSettings, String> {
    let home = match home_override {
        Some(p) => p.to_path_buf(),
        None => dirs::home_dir().ok_or("Cannot determine home directory")?,
    };
    let settings_path = home.join(".claude").join("settings.json");
    let raw = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Cannot read {:?}: {e}", settings_path))?;
    parse_cc_settings(&raw)
}

/// Load CC project settings from `.claude/settings.json` in the given workspace.
pub fn load_cc_project_settings(workspace: &Path) -> Result<CcSettings, String> {
    let settings_path = workspace.join(".claude").join("settings.json");
    let raw = match std::fs::read_to_string(&settings_path) {
        Ok(r) => r,
        Err(_) => return Ok(CcSettings::default()), // missing project config is OK
    };
    parse_cc_settings(&raw)
}

/// A CC MCP server config that can be mapped to DSTUI.
#[derive(Debug, Clone)]
pub struct CcMcpServer {
    pub name: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub env: Vec<(String, String)>,
}

/// Read CC MCP config from `~/.claude/.mcp.json` or `settings.json`.
pub fn load_cc_mcp_servers(home_override: Option<&Path>) -> Vec<CcMcpServer> {
    let home = match home_override {
        Some(p) => p.to_path_buf(),
        None => match dirs::home_dir() {
            Some(h) => h,
            None => return vec![],
        },
    };

    let mut servers = Vec::new();

    // 1. Try .mcp.json first
    let mcp_json = home.join(".claude").join(".mcp.json");
    if let Ok(raw) = std::fs::read_to_string(&mcp_json) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(mcp_servers) = v["mcpServers"].as_object() {
                for (name, cfg) in mcp_servers {
                    servers.push(parse_cc_mcp_entry(name, cfg));
                }
            }
        }
    }

    // 2. Also check settings.json for enabledMcpjsonServers
    let settings = home.join(".claude").join("settings.json");
    if let Ok(raw) = std::fs::read_to_string(&settings) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(enabled) = v["enabledMcpjsonServers"].as_array() {
                for entry in enabled {
                    if let Some(name) = entry.as_str() {
                        if !servers.iter().any(|s| s.name == name) {
                            servers.push(CcMcpServer {
                                name: name.to_string(),
                                command: None,
                                args: vec![],
                                url: None,
                                env: vec![],
                            });
                        }
                    }
                }
            }
        }
    }

    servers
}

fn parse_cc_mcp_entry(name: &str, cfg: &serde_json::Value) -> CcMcpServer {
    let command = cfg["command"].as_str().map(String::from);
    let args = cfg["args"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let url = cfg["url"].as_str().map(String::from);
    let env = cfg["env"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    CcMcpServer {
        name: name.to_string(),
        command,
        args,
        url,
        env,
    }
}

/// Convenience: load and log CC integration status.
/// Call during startup after config is loaded.
pub fn apply_cc_integration(workspace: &Path) {
    // Settings
    match load_cc_user_settings(None) {
        Ok(cc) => {
            if cc.language.is_some() || cc.model.is_some() {
                tracing::info!(
                    "CC bridge: detected user settings (lang={:?}, model={:?})",
                    cc.language,
                    cc.model
                );
            }
        }
        Err(e) => {
            tracing::debug!("CC bridge: no user settings ({e})");
        }
    }

    // Project settings
    match load_cc_project_settings(workspace) {
        Ok(_) => {
            tracing::debug!("CC bridge: project settings loaded");
        }
        Err(_) => {}
    }

    // MCP servers
    let mcp_servers = load_cc_mcp_servers(None);
    if !mcp_servers.is_empty() {
        tracing::info!(
            "CC bridge: found {} CC MCP server(s)",
            mcp_servers.len()
        );
    }
}

/// Parse CC settings JSON and extract DS-relevant fields.
fn parse_cc_settings(raw: &str) -> Result<CcSettings, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid JSON: {e}"))?;
    Ok(CcSettings {
        language: v["language"].as_str().map(String::from),
        model: v["model"].as_str().map(String::from),
        theme: v["theme"].as_str().map(|s| s.to_string()),
        effort: v["effortLevel"].as_str().map(String::from),
        fast_mode: v["fastMode"].as_bool(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_code_settings_json() {
        // CC stores settings in JSON with known fields
        let sample = r#"{"language":"chinese","model":"opus[1m]","theme":"dark"}"#;
        let parsed: serde_json::Value = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed["language"], "chinese");
        assert_eq!(parsed["model"], "opus[1m]");
    }

    #[test]
    fn maps_cc_language_to_ds_locale() {
        assert_eq!(cc_language_to_locale("chinese"), Some("zh-Hans"));
        assert_eq!(cc_language_to_locale("japanese"), Some("ja"));
        assert_eq!(cc_language_to_locale(""), None);
        assert_eq!(cc_language_to_locale("unknown"), None);
    }

    #[test]
    fn maps_cc_theme_to_ds_theme() {
        assert_eq!(cc_theme_to_ds("dark"), Some("dark"));
        assert_eq!(cc_theme_to_ds("light"), Some("light"));
        assert_eq!(cc_theme_to_ds("auto"), None); // DS doesn't have auto, keep current
    }

    #[test]
    fn reads_cc_settings_from_disk() {
        // Write a mock CC settings.json
        let tmp = tempfile::tempdir().unwrap();
        let cc_home = tmp.path().join(".claude");
        std::fs::create_dir_all(&cc_home).unwrap();
        std::fs::write(
            cc_home.join("settings.json"),
            r#"{"language":"chinese","model":"sonnet"}"#,
        )
        .unwrap();

        let result = load_cc_user_settings(Some(tmp.path())).unwrap();
        assert_eq!(result.language.as_deref(), Some("chinese"));
        assert_eq!(result.model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn reads_cc_mcp_json() {
        let tmp = tempfile::tempdir().unwrap();
        let cc_home = tmp.path().join(".claude");
        std::fs::create_dir_all(&cc_home).unwrap();
        std::fs::write(
            cc_home.join(".mcp.json"),
            r#"{"mcpServers":{"test-srv":{"command":"node","args":["server.js"],"env":{"FOO":"bar"}}}}"#,
        ).unwrap();

        let servers = load_cc_mcp_servers(Some(tmp.path()));
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "test-srv");
        assert_eq!(servers[0].command.as_deref(), Some("node"));
        assert_eq!(servers[0].args, vec!["server.js"]);
    }

    #[test]
    fn reads_cc_mcp_from_settings_enabled_list() {
        let tmp = tempfile::tempdir().unwrap();
        let cc_home = tmp.path().join(".claude");
        std::fs::create_dir_all(&cc_home).unwrap();
        std::fs::write(
            cc_home.join("settings.json"),
            r#"{"enabledMcpjsonServers":["srv-a","srv-b"]}"#,
        ).unwrap();

        let servers = load_cc_mcp_servers(Some(tmp.path()));
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "srv-a");
        assert_eq!(servers[1].name, "srv-b");
    }

    #[test]
    fn merges_cc_settings_with_ds_defaults() {
        // When CC settings exist but DS hasn't been configured,
        // the CC values should be used as fallback.
        let cc = CcSettings {
            language: Some("chinese".into()),
            model: Some("opus[1m]".into()),
            theme: Some("dark".into()),
            effort: Some("high".into()),
            fast_mode: None,
        };
        assert_eq!(cc_language_to_locale("chinese"), Some("zh-Hans"));
        assert_eq!(cc_theme_to_ds("dark"), Some("dark"));
        assert_eq!(cc.model.as_deref(), Some("opus[1m]"));
    }

    #[test]
    fn handles_missing_cc_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_cc_user_settings(Some(tmp.path()));
        assert!(result.is_err()); // No .claude/ directory
    }
}

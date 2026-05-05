//! Centralized active config (config.yaml) mutations with atomic write-back.
//!
//! All writes to the active Mihomo config file must go through this store
//! so that the file is never partially written and concurrent writers do not
//! overwrite each other's changes.

use anyhow::Result;
use std::path::PathBuf;

use crate::ControlTowerPaths;

/// Port overrides read from settings.yaml.
/// These override the ports from the profile when generating config.yaml.
#[derive(Debug, Default, Clone)]
pub struct PortOverrides {
    pub mixed_port: Option<u16>,
    pub socks_port: Option<u16>,
    pub http_port: Option<u16>,
    pub external_controller: Option<(String, u16)>, // (host, port)
}

/// Store for the active Mihomo configuration file (config.yaml).
///
/// All mutation methods perform atomic write-back: content is first written
/// to a `.tmp` file next to the target, then atomically renamed over the
/// target, so readers never see a partial file.
#[derive(Debug, Clone)]
pub struct ActiveConfigStore {
    paths: ControlTowerPaths,
}

impl ActiveConfigStore {
    /// Create a new store backed by the given paths model.
    pub fn new(paths: ControlTowerPaths) -> Self {
        Self { paths }
    }

    /// Returns the path to the active config file (config.yaml).
    pub fn active_config_path(&self) -> &PathBuf {
        &self.paths.active_config_path
    }

    /// Replace the entire active config content with the contents of a profile file,
    /// then overlay any user-customized fields from the previous config (mode, port, etc.)
    /// so that activating a profile does not wipe out manual overrides.
    ///
    /// Network accessibility fields (`allow-lan`, `bind-address`) are always taken
    /// from the profile — they are NOT preserved from the old config.
    ///
    /// This is called when a user activates a subscription profile.
    ///
    /// `custom_rules` are prepended before the profile rules (higher priority).
    ///
    /// `port_overrides` from settings.yaml take highest priority over both
    /// the profile and the old config's user_overrides.
    ///
    /// Returns the number of profile rules (rules from the profile file itself,
    /// before custom rules are prepended). This should be stored in settings.yaml
    /// and passed back to `merge_rules_with_custom`.
    pub fn replace_from_profile(
        &self,
        profile_path: &PathBuf,
        custom_rules: &[String],
        port_overrides: PortOverrides,
    ) -> Result<usize> {
        let new_content = std::fs::read_to_string(profile_path)?;

        // Read existing config to extract user customizations (if any)
        let user_overrides = if self.paths.active_config_path.exists() {
            match std::fs::read_to_string(&self.paths.active_config_path) {
                Ok(c) => Self::extract_user_overrides(&c),
                Err(_) => serde_yaml_ng::Mapping::new(),
            }
        } else {
            serde_yaml_ng::Mapping::new()
        };

        // Parse the new profile content
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&new_content)?;

        // Count profile rules BEFORE prepending custom rules
        let profile_rules_count = yaml
            .get("rules")
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.len())
            .unwrap_or(0);

        // Apply port overrides from settings.yaml (highest priority)
        if let Some(map) = yaml.as_mapping_mut() {
            if let Some(p) = port_overrides.mixed_port {
                map.insert("mixed-port".into(), serde_yaml_ng::Value::Number(p.into()));
            }
            if let Some(p) = port_overrides.socks_port {
                map.insert("socks-port".into(), serde_yaml_ng::Value::Number(p.into()));
            }
            if let Some(p) = port_overrides.http_port {
                map.insert("http-port".into(), serde_yaml_ng::Value::Number(p.into()));
            }
            if let Some((host, port)) = port_overrides.external_controller {
                map.insert(
                    "external-controller".into(),
                    serde_yaml_ng::Value::String(format!("{}:{}", host, port)),
                );
            }
        }

        // Overlay user overrides onto the new config (except rules and ports — handled above)
        if let Some(map) = yaml.as_mapping_mut() {
            for (key, val) in user_overrides {
                if key == "rules" || key == "mixed-port" || key == "socks-port" || key == "http-port" || key == "external-controller" {
                    continue;
                }
                map.insert(key, val);
            }
        }

        // Prepend custom rules before profile rules (higher priority)
        if let Ok(rules) = Self::get_or_create_rules_array(&mut yaml) {
            for rule in custom_rules.iter().rev() {
                rules.insert(0, serde_yaml_ng::Value::String(rule.clone()));
            }
        }

        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)?;
        Ok(profile_rules_count)
    }

    /// Extract fields the user may have manually customized that should survive
    /// a profile switch.
    ///
    /// NOTE: `allow-lan` and `bind-address` are deliberately excluded — they
    /// control network accessibility and must come from the profile, otherwise
    /// a stale `allow-lan: false` in the old config would silently override the
    /// profile's `allow-lan: true`, blocking external proxy access.
    fn extract_user_overrides(content: &str) -> serde_yaml_ng::Mapping {
        let Ok(yaml) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(content) else {
            return serde_yaml_ng::Mapping::new();
        };
        let Some(map) = yaml.as_mapping() else {
            return serde_yaml_ng::Mapping::new();
        };
        let keys = [
            "mode",
            "mixed-port",
            "redir-port",
            "tproxy-port",
            "log-level",
            "dns",
            "socks-port",
            "http-port",
            "external-controller",
            "tun",
            // NOTE: "rule-providers" is NOT preserved here — it is loaded separately
            // from settings.yaml and injected via set_rule_providers after replace_from_profile.
            // NOTE: "rules" is NOT preserved here — rules come from the profile file
            // and custom user rules are appended separately via custom_rules parameter.
        ];
        let mut overrides = serde_yaml_ng::Mapping::new();
        for key in keys {
            if let Some(v) = map.get(key) {
                overrides.insert(key.into(), v.clone());
            }
        }
        overrides
    }

    /// Set the `mode` field in the active config, preserving all other fields.
    pub fn set_mode(&self, mode: &str) -> Result<()> {
        let content = std::fs::read_to_string(&self.paths.active_config_path)?;
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;
        if let Some(map) = yaml.as_mapping_mut() {
            map.insert("mode".into(), mode.into());
        }
        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Append a single rule to the rules array, creating the array if absent.
    pub fn append_rule(&self, rule: &str) -> Result<()> {
        let content = if self.paths.active_config_path.exists() {
            std::fs::read_to_string(&self.paths.active_config_path)?
        } else {
            // Create default config if file doesn't exist
            "mode: rule\n".to_string()
        };
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;

        // Get or create the rules array
        let rules = Self::get_or_create_rules_array(&mut yaml)?;
        rules.push(serde_yaml_ng::Value::String(rule.to_string()));

        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Prepend a single rule to the rules array (insert at top, highest priority).
    pub fn prepend_rule(&self, rule: &str) -> Result<()> {
        let content = std::fs::read_to_string(&self.paths.active_config_path)?;
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;

        let rules = Self::get_or_create_rules_array(&mut yaml)?;
        rules.insert(0, serde_yaml_ng::Value::String(rule.to_string()));

        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Remove a rule from the rules array by its string content.
    pub fn remove_rule_by_content(&self, rule_content: &str) -> Result<()> {
        let content = std::fs::read_to_string(&self.paths.active_config_path)?;
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;

        let rules = yaml
            .get_mut("rules")
            .and_then(|v| v.as_sequence_mut())
            .ok_or_else(|| anyhow::anyhow!("No rules found in config"))?;

        let original_len = rules.len();
        rules.retain(|r| !r.as_str().map_or(false, |s| s == rule_content));

        if rules.len() == original_len {
            anyhow::bail!("Rule not found: {}", rule_content);
        }

        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Remove a rule by 1-based index, returning the removed rule string.
    pub fn remove_rule(&self, index: usize) -> Result<String> {
        let content = std::fs::read_to_string(&self.paths.active_config_path)?;
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;

        let rules = yaml
            .get_mut("rules")
            .and_then(|v| v.as_sequence_mut())
            .ok_or_else(|| anyhow::anyhow!("No rules found in config"))?;

        if index == 0 || index > rules.len() {
            anyhow::bail!(
                "Invalid index {}. Valid range: 1-{}",
                index,
                rules.len()
            );
        }

        // Convert from 1-based to 0-based
        let removed = rules.remove(index - 1);
        let removed_str = removed
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| format!("{:?}", removed));

        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)?;
        Ok(removed_str)
    }

    /// Clear all rules from the active config atomically.
    pub fn clear_rules(&self) -> Result<()> {
        let content = if self.paths.active_config_path.exists() {
            std::fs::read_to_string(&self.paths.active_config_path)?
        } else {
            "mode: rule\n".to_string()
        };
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;
        if let Some(map) = yaml.as_mapping_mut() {
            map.insert("rules".into(), serde_yaml_ng::Value::Sequence(vec![]));
        } else {
            let mut new_map = serde_yaml_ng::Mapping::new();
            new_map.insert("mode".into(), yaml);
            new_map.insert("rules".into(), serde_yaml_ng::Value::Sequence(vec![]));
            yaml = serde_yaml_ng::Value::Mapping(new_map);
        }
        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Import multiple rules, appending them to the existing rules array.
    pub fn import_rules(&self, new_rules: Vec<String>) -> Result<()> {
        if new_rules.is_empty() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.paths.active_config_path)?;
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;

        let rules = Self::get_or_create_rules_array(&mut yaml)?;
        for rule in new_rules {
            rules.push(serde_yaml_ng::Value::String(rule));
        }

        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Merge profile rules with custom rules and write to active config.
    /// Custom rules are placed FIRST (higher priority in Mihomo's first-match-wins evaluation).
    /// Profile rules follow after custom rules.
    /// Used after add/delete custom rules to sync config.yaml.
    pub fn merge_rules_with_custom(
        &self,
        profile_rules_count: usize,
        custom_rules: &[String],
    ) -> Result<()> {
        let content = if self.paths.active_config_path.exists() {
            std::fs::read_to_string(&self.paths.active_config_path)?
        } else {
            "mode: rule\n".to_string()
        };
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;

        // Extract existing rules from config
        let existing_rules: Vec<String> = yaml
            .get("rules")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // existing_rules = [custom rules (0..custom_count)) + profile rules (custom_count..)]
        // custom_count = total - profile_rules_count
        // profile_rules are AFTER custom rules, so skip the custom portion
        let custom_count = existing_rules.len().saturating_sub(profile_rules_count);
        let profile_rules: Vec<String> = existing_rules
            .iter()
            .skip(custom_count)
            .cloned()
            .collect();

        // Build new rules array: custom_rules FIRST (high priority), then profile_rules
        let mut new_rules: Vec<serde_yaml_ng::Value> = custom_rules
            .iter()
            .map(|s| serde_yaml_ng::Value::String(s.clone()))
            .collect();
        for rule in &profile_rules {
            new_rules.push(serde_yaml_ng::Value::String(rule.clone()));
        }

        if let Some(map) = yaml.as_mapping_mut() {
            map.insert("rules".into(), serde_yaml_ng::Value::Sequence(new_rules));
        }

        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Atomically write `content` to `active_config_path` using a .tmp rename.
    fn write_atomically(&self, content: &str) -> Result<()> {
        let target = &self.paths.active_config_path;
        let temp = target.with_extension("yaml.tmp");
        std::fs::write(&temp, content)?;
        std::fs::rename(&temp, target)?;
        Ok(())
    }

    /// Get the rules array from yaml, creating it if absent.
    fn get_or_create_rules_array(yaml: &mut serde_yaml_ng::Value) -> Result<&mut serde_yaml_ng::Sequence> {
        if yaml.get("rules").is_none() {
            if let Some(map) = yaml.as_mapping_mut() {
                map.insert(
                    "rules".into(),
                    serde_yaml_ng::Value::Sequence(vec![]),
                );
            }
        }
        yaml.get_mut("rules")
            .and_then(|v| v.as_sequence_mut())
            .ok_or_else(|| anyhow::anyhow!("rules is not an array"))
    }

    /// Set the entire `rule-providers` section in the active config.yaml.
    /// Writes as a map keyed by provider name (Mihomo format), replacing any existing section.
    pub fn set_rule_providers(&self, providers: &[crate::profiles::RuleProviderConfig]) -> Result<()> {
        
        let content = if self.paths.active_config_path.exists() {
            std::fs::read_to_string(&self.paths.active_config_path)?
        } else {
            String::new()
        };
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
            .unwrap_or(serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new()));

        // Build a map keyed by provider name (Mihomo format)
        let rp_map: serde_yaml_ng::Mapping = providers
            .iter()
            .map(|p| {
                let key = serde_yaml_ng::Value::String(p.name.clone());
                let value = serde_yaml_ng::to_value(p)
                    .unwrap_or(serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new()));
                (key, value)
            })
            .collect();

        if let Some(map) = yaml.as_mapping_mut() {
            map.insert("rule-providers".into(), serde_yaml_ng::Value::Mapping(rp_map));
        }
        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }

    /// Remove a specific rule provider by name from the active config.yaml.
    pub fn remove_rule_provider(&self, name: &str) -> Result<()> {
        let content = std::fs::read_to_string(&self.paths.active_config_path)?;
        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)?;
        if let Some(map) = yaml.as_mapping_mut() {
            if let Some(rp) = map.get_mut("rule-providers").and_then(|v| v.as_mapping_mut()) {
                rp.remove(&serde_yaml_ng::Value::String(name.into()));
            }
        }
        self.write_atomically(&serde_yaml_ng::to_string(&yaml)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_paths() -> (tempfile::TempDir, ControlTowerPaths) {
        let temp = tempfile::tempdir().unwrap();
        let paths = ControlTowerPaths::from_settings(
            temp.path().join("settings.yaml"),
            Some(temp.path().to_path_buf()),
        );
        (temp, paths)
    }

    #[test]
    fn test_set_mode_preserves_existing_rules() {
        let (_temp, paths) = make_temp_paths();
        std::fs::write(
            &paths.active_config_path,
            "mode: rule\nrules:\n  - DOMAIN-SUFFIX,google.com,DIRECT\n  - MATCH,DIRECT\n",
        )
        .unwrap();

        let store = ActiveConfigStore::new(paths.clone());
        store.set_mode("global").unwrap();

        let content = std::fs::read_to_string(&paths.active_config_path).unwrap();
        assert!(content.contains("mode: global"));
        assert!(content.contains("DOMAIN-SUFFIX,google.com,DIRECT"));
        assert!(content.contains("MATCH,DIRECT"));
    }

    #[test]
    fn test_replace_from_profile_preserves_user_overrides() {
        let (_temp, paths) = make_temp_paths();
        let profile_path = paths.config_dir.join("profiles").join("p1.yaml");
        std::fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
        std::fs::write(&profile_path, "mode: rule\nproxies: []\n").unwrap();

        // Pre-existing content with user overrides
        std::fs::write(&paths.active_config_path, "mode: global\nmixed-port: 7890\nold: content\n").unwrap();

        let store = ActiveConfigStore::new(paths.clone());
        store.replace_from_profile(&profile_path, &[]).unwrap();

        let content = std::fs::read_to_string(&paths.active_config_path).unwrap();
        // User overrides (mode, mixed-port) should be preserved
        assert!(content.contains("mode: global"));
        assert!(content.contains("mixed-port: 7890"));
        // Profile content (proxies) should be present
        assert!(content.contains("proxies: []"));
        // Non-override fields from old config should NOT be present
        assert!(!content.contains("old: content"));
        // Ensure no .tmp file left behind
        assert!(!paths.active_config_path.with_extension("yaml.tmp").exists());
    }

    #[test]
    fn test_append_rule_creates_rules_if_absent() {
        let (_temp, paths) = make_temp_paths();
        std::fs::write(&paths.active_config_path, "mode: rule\n").unwrap();

        let store = ActiveConfigStore::new(paths.clone());
        store.append_rule("GEOIP,CN,DIRECT").unwrap();

        let content = std::fs::read_to_string(&paths.active_config_path).unwrap();
        assert!(content.contains("GEOIP,CN,DIRECT"));
    }

    #[test]
    fn test_remove_rule_by_index() {
        let (_temp, paths) = make_temp_paths();
        std::fs::write(
            &paths.active_config_path,
            "mode: rule\nrules:\n  - DOMAIN-SUFFIX,google.com,DIRECT\n  - GEOIP,CN,DIRECT\n",
        )
        .unwrap();

        let store = ActiveConfigStore::new(paths.clone());
        let removed = store.remove_rule(1).unwrap();

        assert_eq!(removed, "DOMAIN-SUFFIX,google.com,DIRECT");

        let content = std::fs::read_to_string(&paths.active_config_path).unwrap();
        assert!(!content.contains("google.com"));
        assert!(content.contains("GEOIP,CN,DIRECT"));
    }

    #[test]
    fn test_import_rules_append_only() {
        let (_temp, paths) = make_temp_paths();
        std::fs::write(
            &paths.active_config_path,
            "mode: rule\nrules:\n  - MATCH,DIRECT\n",
        )
        .unwrap();

        let store = ActiveConfigStore::new(paths.clone());
        store
            .import_rules(vec![
                "DOMAIN-SUFFIX,google.com,DIRECT".to_string(),
                "GEOIP,CN,DIRECT".to_string(),
            ])
            .unwrap();

        let content = std::fs::read_to_string(&paths.active_config_path).unwrap();
        assert!(content.contains("DOMAIN-SUFFIX,google.com,DIRECT"));
        assert!(content.contains("GEOIP,CN,DIRECT"));
        // Original MATCH rule still present
        assert!(content.contains("MATCH,DIRECT"));
    }

    #[test]
    fn test_atomic_write_leaves_no_tmp_file() {
        let (_temp, paths) = make_temp_paths();
        std::fs::write(&paths.active_config_path, "mode: rule\n").unwrap();

        let store = ActiveConfigStore::new(paths.clone());
        store.set_mode("global").unwrap();

        assert!(!paths.active_config_path.with_extension("yaml.tmp").exists());
    }
}

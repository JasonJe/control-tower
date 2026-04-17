//! Rule management module

use anyhow::Result;
use std::path::PathBuf;

use crate::config::get_config_dir;
use crate::RuleAction;

/// Rule types supported by Mihomo
const RULE_TYPES: &[&str] = &[
    "DOMAIN",
    "DOMAIN-SUFFIX",
    "DOMAIN-KEYWORD",
    "DOMAIN-WILDCARD",
    "GEOIP",
    "IP-CIDR",
    "IP-CIDR6",
    "PROCESS-NAME",
    "NETWORK",
    "RULE-SET",
    "MATCH",
];

/// Special proxy names
const SPECIAL_PROXIES: &[&str] = &[
    "DIRECT",
    "REJECT",
    "REJECT-DROP",
];

/// Truncate string to max_width characters, showing start and end with "..." in middle if truncated
fn truncate_str(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let char_count = chars.len();
    if char_count <= max_width {
        return s.to_string();
    }

    // Need at least 5 chars: start + "..." + end (3 dots = 3)
    if max_width < 5 {
        return chars[..max_width].iter().collect();
    }

    // Calculate: total = start_len + 3 (dots) + end_len
    // So: start_len + end_len = max_width - 3
    let remaining = max_width - 3;
    let start_len = remaining / 2;
    let end_len = remaining - start_len;

    // Start: first start_len characters
    let start: String = chars[..start_len].iter().collect();
    // End: last end_len characters
    let end: String = chars[char_count - end_len..].iter().collect();

    format!("{}...{}", start, end)
}

pub async fn handle(action: RuleAction) -> Result<()> {
    match action {
        RuleAction::List { limit } => list_rules(limit).await?,
        RuleAction::Add { rule } => add_rule(&rule).await?,
        RuleAction::Remove { index } => remove_rule(index).await?,
        RuleAction::Import { path } => import_rules(&path).await?,
        RuleAction::Export { path } => export_rules(path).await?,
    }
    Ok(())
}

/// List all current rules
async fn list_rules(limit: Option<usize>) -> Result<()> {
    let rules = parse_rules_from_config().await?;

    if rules.is_empty() {
        println!("No rules configured.");
        return Ok(());
    }

    let display_rules = if let Some(limit) = limit {
        &rules[..rules.len().min(limit)]
    } else {
        &rules
    };

    println!("Rules ({} total):", rules.len());
    println!("{}", "=".repeat(82));
    println!("{:<6}  {:<20}  {:<25}  {:<20}", "Index", "Type", "Value", "Proxy");
    println!("{}", "-".repeat(82));

    for (idx, rule) in display_rules.iter().enumerate() {
        let parts: Vec<&str> = rule.split(',').collect();
        if parts.len() >= 3 {
            let rule_type = parts[0];
            let value = parts[1..parts.len()-1].join(",");
            let proxy = parts[parts.len()-1];

            let type_str = truncate_str(rule_type, 18);
            let value_str = truncate_str(&value, 23);
            let proxy_str = truncate_str(proxy, 20);

            println!("{:<6}  {:<20}  {:<25}  {:<20}",
                format!("[{}]", idx + 1),
                type_str,
                value_str,
                proxy_str
            );
        } else if parts.len() == 2 {
            // MATCH rule (no value)
            println!("{:<6}  {:<20}  {:<25}  {:<20}",
                format!("[{}]", idx + 1),
                truncate_str(parts[0], 18),
                "-",
                truncate_str(parts[1], 20)
            );
        }
    }

    if let Some(limit) = limit {
        if rules.len() > limit {
            println!("\n... and {} more rules", rules.len() - limit);
        }
    }

    println!("\nTip: Use 'ctctl rule add' to add new rules");
    println!("     Use 'ctctl rule remove <index>' to delete a rule");

    Ok(())
}

/// Add a new rule
async fn add_rule(rule: &str) -> Result<()> {
    // Validate rule format
    let rule = rule.trim();

    // Parse rule to validate
    let parts: Vec<&str> = rule.split(',').collect();
    if parts.len() < 3 {
        anyhow::bail!(
            "Invalid rule format. Expected: TYPE,VALUE,PROXY\n\
             Example: DOMAIN-SUFFIX,google.com,香港 101\n\
             \n\
             Supported types: DOMAIN, DOMAIN-SUFFIX, DOMAIN-KEYWORD, GEOIP, IP-CIDR, IP-CIDR6, PROCESS-NAME, RULE-SET\n\
             Special proxies: DIRECT, REJECT, REJECT-DROP"
        );
    }

    let rule_type = parts[0].to_uppercase();
    if !RULE_TYPES.contains(&rule_type.as_str()) && rule_type != "RULE-SET" {
        anyhow::bail!(
            "Unknown rule type: {}\n\
             Supported types: DOMAIN, DOMAIN-SUFFIX, DOMAIN-KEYWORD, GEOIP, IP-CIDR, IP-CIDR6, PROCESS-NAME, RULE-SET",
            rule_type
        );
    }

    let proxy = parts[parts.len() - 1];
    if !SPECIAL_PROXIES.contains(&proxy) {
        // Check if it's a valid proxy name (non-empty)
        if proxy.is_empty() {
            anyhow::bail!("Proxy name cannot be empty");
        }
    }

    // Read current config
    let config_path = get_config_dir()?.join("config.yaml");
    if !config_path.exists() {
        anyhow::bail!("Config file not found at {:?}", config_path);
    }

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to read config: {}", e))?;

    // Parse YAML
    let mut config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    // Get or create rules array
    let rules = if let Some(r) = config.get_mut("rules") {
        if let Some(arr) = r.as_sequence_mut() {
            arr
        } else {
            anyhow::bail!("Rules in config is not an array");
        }
    } else {
        // Create rules section
        config.as_mapping_mut().unwrap().insert(
            serde_yaml_ng::Value::String("rules".to_string()),
            serde_yaml_ng::Value::Sequence(vec![]),
        );
        config.get_mut("rules").unwrap().as_sequence_mut().unwrap()
    };

    // Add the new rule
    rules.push(serde_yaml_ng::Value::String(rule.to_string()));

    // Write back
    let new_content = serde_yaml_ng::to_string(&config)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;

    std::fs::write(&config_path, new_content)
        .map_err(|e| anyhow::anyhow!("Failed to write config: {}", e))?;

    println!("Rule added successfully: {}", rule);
    println!("Use 'ctctl service restart' to apply changes.");

    Ok(())
}

/// Remove a rule by index
async fn remove_rule(index: usize) -> Result<()> {
    if index == 0 {
        anyhow::bail!("Invalid index. Use 1-based index from 'rule list'");
    }

    let config_path = get_config_dir()?.join("config.yaml");
    if !config_path.exists() {
        anyhow::bail!("Config file not found at {:?}", config_path);
    }

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to read config: {}", e))?;

    let mut config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    let rules = if let Some(r) = config.get_mut("rules") {
        if let Some(arr) = r.as_sequence_mut() {
            arr
        } else {
            anyhow::bail!("Rules in config is not an array");
        }
    } else {
        anyhow::bail!("No rules found in config");
    };

    if index > rules.len() {
        anyhow::bail!("Index {} out of range. Valid range: 1-{}", index, rules.len());
    }

    // Remove the rule (1-based to 0-based)
    let removed = rules.remove(index - 1);

    // Write back
    let new_content = serde_yaml_ng::to_string(&config)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;

    std::fs::write(&config_path, new_content)
        .map_err(|e| anyhow::anyhow!("Failed to write config: {}", e))?;

    println!("Removed rule: {}", removed.as_str().unwrap_or(&format!("{:?}", removed)));
    println!("Use 'ctctl service restart' to apply changes.");

    Ok(())
}

/// Import rules from a file
async fn import_rules(path: &str) -> Result<()> {
    let path = PathBuf::from(path);

    if !path.exists() {
        anyhow::bail!("File not found: {:?}", path);
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))?;

    // Try to parse as YAML array first
    let new_rules: Vec<String> = if let Ok(rules) = serde_yaml_ng::from_str::<Vec<String>>(&content) {
        rules
    } else if let Ok(rules) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content) {
        // Try to extract from a rules key
        if let Some(arr) = rules.get("rules").and_then(|v| v.as_sequence()) {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            // Treat each line as a rule
            content.lines()
                .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                .map(|l| l.trim().to_string())
                .collect()
        }
    } else {
        // Treat each line as a rule
        content.lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .map(|l| l.trim().to_string())
            .collect()
    };

    if new_rules.is_empty() {
        anyhow::bail!("No rules found in file");
    }

    // Read current config
    let config_path = get_config_dir()?.join("config.yaml");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to read config: {}", e))?;

    let mut config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    let rules = if let Some(r) = config.get_mut("rules") {
        if let Some(arr) = r.as_sequence_mut() {
            arr
        } else {
            anyhow::bail!("Rules in config is not an array");
        }
    } else {
        config.as_mapping_mut().unwrap().insert(
            serde_yaml_ng::Value::String("rules".to_string()),
            serde_yaml_ng::Value::Sequence(vec![]),
        );
        config.get_mut("rules").unwrap().as_sequence_mut().unwrap()
    };

    let count = new_rules.len();
    for rule in new_rules {
        rules.push(serde_yaml_ng::Value::String(rule));
    }

    // Write back
    let new_content = serde_yaml_ng::to_string(&config)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;

    std::fs::write(&config_path, new_content)
        .map_err(|e| anyhow::anyhow!("Failed to write config: {}", e))?;

    println!("Imported {} rules successfully.", count);
    println!("Use 'ctctl service restart' to apply changes.");

    Ok(())
}

/// Export rules to a file
async fn export_rules(path: Option<String>) -> Result<()> {
    let rules = parse_rules_from_config().await?;

    if rules.is_empty() {
        println!("No rules to export.");
        return Ok(());
    }

    let content: String = rules.iter()
        .map(|r| format!("- '{}'", r))
        .collect::<Vec<_>>()
        .join("\n");

    let yaml_content = format!("rules:\n{}", content);

    if let Some(path) = path {
        std::fs::write(&path, yaml_content)
            .map_err(|e| anyhow::anyhow!("Failed to write file: {}", e))?;
        println!("Rules exported to: {}", path);
    } else {
        println!("{}", yaml_content);
    }

    Ok(())
}

/// Parse rules from config file
async fn parse_rules_from_config() -> Result<Vec<String>> {
    let config_path = get_config_dir()?.join("config.yaml");

    if !config_path.exists() {
        return Ok(vec![]);
    }

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to read config: {}", e))?;

    let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    if let Some(rules) = config.get("rules").and_then(|v| v.as_sequence()) {
        Ok(rules.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
    } else {
        Ok(vec![])
    }
}

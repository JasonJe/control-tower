//! Offline rule matching for the "test a destination against the rules" tool.
//!
//! This mirrors the subset of Clash rules that can be evaluated without the
//! running kernel: domain patterns, CIDR blocks, ports and MATCH. Rules that
//! need providers or a geo database (RULE-SET, GEOIP, GEOSITE, ...) are
//! reported as skipped instead of silently falling through, so the UI can say
//! why a rule was not evaluated.

use std::net::IpAddr;

use serde::Serialize;

/// One rule considered during evaluation.
#[derive(Debug, Clone, Serialize)]
pub struct RuleEvaluation {
    /// 0-based index in the original rule list.
    pub index: usize,
    /// Original rule text.
    pub rule: String,
    /// Rule type, upper-cased (`DOMAIN-SUFFIX`, `MATCH`, ...).
    pub rule_type: String,
    /// Policy target of the rule.
    pub policy: String,
    /// The rule decided the destination.
    pub matched: bool,
    /// The rule could not be evaluated offline.
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Result of matching a destination against a rule list.
#[derive(Debug, Clone, Serialize)]
pub struct MatchResult {
    pub target: String,
    pub is_ip: bool,
    /// Winning rule, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winner: Option<RuleEvaluation>,
    /// Rules in order, up to and including the winner.
    pub evaluated: Vec<RuleEvaluation>,
    /// Number of rules that need providers/geo data or are unsupported.
    pub skipped: usize,
}

/// Normalize a user-supplied destination.
///
/// Accepts a bare host, `host:port`, an IP, or a URL. Returns the host plus the
/// port when one was present (the port is only used by port rules).
fn normalize(input: &str) -> (String, Option<u16>) {
    let mut target = input.trim();
    if let Some((_, rest)) = target.split_once("://") {
        target = rest;
    }
    // Drop path/query/fragment before looking at the host.
    if let Some((host, _)) = target.split_once('/') {
        target = host;
    }
    let target = target.trim().trim_end_matches('.').to_ascii_lowercase();

    // Bracketed IPv6, optionally followed by a port.
    if let Some(rest) = target.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok());
            return (host.to_string(), port);
        }
    }

    // `host:port` only when the host part is not itself an IPv6 literal.
    if target.parse::<IpAddr>().is_err() {
        if let Some((host, port)) = target.rsplit_once(':') {
            if !host.is_empty() && !host.contains(':') {
                if let Ok(port) = port.parse::<u16>() {
                    return (host.to_string(), Some(port));
                }
            }
        }
    }
    (target, None)
}

/// Match `target` against `rules` in order.
pub fn evaluate(rules: &[String], target: &str, port: Option<u16>) -> MatchResult {
    let (host, parsed_port) = normalize(target);
    let port = port.or(parsed_port);
    let ip: Option<IpAddr> = host.parse().ok();

    let mut result = MatchResult {
        target: host.clone(),
        is_ip: ip.is_some(),
        winner: None,
        evaluated: Vec::new(),
        skipped: 0,
    };

    for (index, rule) in rules.iter().enumerate() {
        let text = rule.trim();
        if text.is_empty() || text.starts_with('#') || text.starts_with("//") {
            continue;
        }

        let fields: Vec<&str> = text.split(',').map(str::trim).collect();
        let rule_type = fields[0].to_ascii_uppercase();
        // The policy is the last field for payload rules and the second field
        // for MATCH (`MATCH,DIRECT`).
        let policy = fields.last().copied().unwrap_or("").to_string();
        let payload = if rule_type == "MATCH" {
            None
        } else {
            fields.get(1).copied()
        };

        let outcome = match_rule(&rule_type, payload, &host, ip, port);

        let entry = RuleEvaluation {
            index,
            rule: text.to_string(),
            rule_type,
            policy,
            matched: outcome.is_match(),
            skipped: outcome.is_skipped(),
            note: outcome.note(),
        };
        if entry.skipped {
            result.skipped += 1;
        }
        let matched = entry.matched;
        result.evaluated.push(entry.clone());
        if matched {
            result.winner = Some(entry);
            break;
        }
    }

    result
}

enum Outcome {
    Match,
    NoMatch,
    Skipped(&'static str),
}

impl Outcome {
    fn is_match(&self) -> bool {
        matches!(self, Outcome::Match)
    }
    fn is_skipped(&self) -> bool {
        matches!(self, Outcome::Skipped(_))
    }
    fn note(&self) -> Option<String> {
        match self {
            Outcome::Skipped(reason) => Some((*reason).to_string()),
            _ => None,
        }
    }
}

fn match_rule(
    rule_type: &str,
    payload: Option<&str>,
    host: &str,
    ip: Option<IpAddr>,
    port: Option<u16>,
) -> Outcome {
    match rule_type {
        "MATCH" => Outcome::Match,
        "DOMAIN" => match payload {
            Some(value) if host.eq_ignore_ascii_case(value) => Outcome::Match,
            Some(_) => Outcome::NoMatch,
            None => Outcome::Skipped("rule has no payload"),
        },
        "DOMAIN-SUFFIX" => match payload {
            Some(value) => {
                let value = value.trim_start_matches('.').to_ascii_lowercase();
                if host == value || host.ends_with(&format!(".{}", value)) {
                    Outcome::Match
                } else {
                    Outcome::NoMatch
                }
            }
            None => Outcome::Skipped("rule has no payload"),
        },
        "DOMAIN-KEYWORD" => match payload {
            Some(value) => {
                if host.contains(&value.to_ascii_lowercase()) {
                    Outcome::Match
                } else {
                    Outcome::NoMatch
                }
            }
            None => Outcome::Skipped("rule has no payload"),
        },
        "DOMAIN-REGEX" | "DOMAIN-REGEXP" => match payload {
            Some(value) => match regex::Regex::new(value) {
                Ok(re) => {
                    if re.is_match(host) {
                        Outcome::Match
                    } else {
                        Outcome::NoMatch
                    }
                }
                Err(_) => Outcome::Skipped("invalid regular expression"),
            },
            None => Outcome::Skipped("rule has no payload"),
        },
        "IP-CIDR" | "IP-CIDR6" => {
            let Some(value) = payload else {
                return Outcome::Skipped("rule has no payload");
            };
            match ip {
                Some(ip) => match ip_in_cidr(ip, value) {
                    Some(true) => Outcome::Match,
                    Some(false) => Outcome::NoMatch,
                    None => Outcome::Skipped("invalid CIDR"),
                },
                None => Outcome::Skipped("destination is a domain; offline matching cannot resolve it"),
            }
        }
        "DST-PORT" | "DST-PORT-RANGE" => match (payload.and_then(|p| p.parse::<u16>().ok()), port) {
            (Some(rule_port), Some(target_port)) => {
                if rule_type == "DST-PORT-RANGE" {
                    Outcome::Skipped("port ranges are not supported offline")
                } else if rule_port == target_port {
                    Outcome::Match
                } else {
                    Outcome::NoMatch
                }
            }
            _ => Outcome::Skipped("destination port is required"),
        },
        "SRC-IP-CIDR" | "SRC-IP-CIDR6" | "SRC-PORT" | "SRC-PORT-RANGE" => {
            Outcome::Skipped("source address is not part of the test input")
        }
        "RULE-SET" => Outcome::Skipped("rule-set contents are not available offline"),
        "GEOSITE" | "GEOIP" | "SRC-GEOIP" | "SRC-GEOSITE" => {
            Outcome::Skipped("geo database lookup is not available offline")
        }
        "PROCESS-NAME" | "PROCESS-PATH" | "PROCESS-NAME-REGEX" | "PROCESS-PATH-REGEX" => {
            Outcome::Skipped("process rules need the originating process")
        }
        "SCRIPT" => Outcome::Skipped("script rules are not evaluated offline"),
        other if other.starts_with("AND") || other.starts_with("OR") || other.starts_with("NOT") => {
            Outcome::Skipped("logical rules are not evaluated offline")
        }
        other => {
            Outcome::Skipped(if other.is_empty() {
                "rule has no type"
            } else {
                "unsupported rule type"
            })
        }
    }
}

fn ip_in_cidr(ip: IpAddr, cidr: &str) -> Option<bool> {
    let (network, prefix) = cidr.trim().split_once('/')?;
    let prefix: u32 = prefix.trim().parse().ok()?;
    let network: IpAddr = network.trim().parse().ok()?;

    match (ip, network) {
        (IpAddr::V4(ip), IpAddr::V4(net)) => {
            if prefix > 32 {
                return None;
            }
            let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
            Some(u32::from(ip) & mask == u32::from(net) & mask)
        }
        (IpAddr::V6(ip), IpAddr::V6(net)) => {
            if prefix > 128 {
                return None;
            }
            let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
            Some(u128::from(ip) & mask == u128::from(net) & mask)
        }
        // An IPv4-mapped IPv6 destination can be compared against an IPv4 block.
        (IpAddr::V6(ip), IpAddr::V4(net)) => match ip.to_ipv4_mapped() {
            Some(ip) => ip_in_cidr(IpAddr::V4(ip), &format!("{}/{}", net, prefix)),
            None => Some(false),
        },
        (IpAddr::V4(_), IpAddr::V6(_)) => Some(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn matches_domain_suffix_before_match() {
        let eval = evaluate(
            &rules(&["DOMAIN,ads.example.com,REJECT", "DOMAIN-SUFFIX,example.com,PROXY", "MATCH,DIRECT"]),
            "https://www.example.com/path",
            None,
        );
        assert_eq!(eval.target, "www.example.com");
        assert!(!eval.is_ip);
        let winner = eval.winner.expect("suffix should win");
        assert_eq!(winner.index, 1);
        assert_eq!(winner.policy, "PROXY");
        assert_eq!(eval.evaluated.len(), 2);
    }

    #[test]
    fn suffix_requires_label_boundary() {
        let eval = evaluate(&rules(&["DOMAIN-SUFFIX,example.com,DIRECT"]), "notexample.com", None);
        assert!(eval.winner.is_none());
    }

    #[test]
    fn domain_keyword_and_regex() {
        let eval = evaluate(
            &rules(&["DOMAIN-KEYWORD,telegram,PROXY", "MATCH,DIRECT"]),
            "api.telegram.org",
            None,
        );
        assert_eq!(eval.winner.unwrap().policy, "PROXY");

        let eval = evaluate(
            &rules(&["DOMAIN-REGEX,^ads?[0-9]*\\.example\\.com$,REJECT", "MATCH,DIRECT"]),
            "ad12.example.com",
            None,
        );
        assert_eq!(eval.winner.unwrap().policy, "REJECT");
    }

    #[test]
    fn ipv4_cidr_and_fallback() {
        let eval = evaluate(
            &rules(&["IP-CIDR,10.0.0.0/8,REJECT,no-resolve", "IP-CIDR,1.1.1.0/24,PROXY", "MATCH,DIRECT"]),
            "1.1.1.1",
            None,
        );
        assert!(eval.is_ip);
        let winner = eval.winner.expect("second CIDR should match");
        assert_eq!(winner.policy, "PROXY");
        assert_eq!(eval.evaluated.len(), 2);
    }

    #[test]
    fn ipv6_cidr() {
        let eval = evaluate(
            &rules(&["IP-CIDR6,2001:db8::/32,PROXY", "MATCH,DIRECT"]),
            "2001:db8::1",
            None,
        );
        assert_eq!(eval.winner.unwrap().policy, "PROXY");
    }

    #[test]
    fn rule_set_is_skipped_not_matched() {
        let eval = evaluate(
            &rules(&["RULE-SET,ads,REJECT", "GEOIP,CN,DIRECT", "MATCH,PROXY"]),
            "example.org",
            None,
        );
        assert_eq!(eval.skipped, 2);
        let winner = eval.winner.expect("MATCH should win");
        assert_eq!(winner.index, 2);
        assert!(eval.evaluated[0].note.as_deref().unwrap().contains("rule-set"));
    }

    #[test]
    fn dst_port_matches_when_provided() {
        let eval = evaluate(&rules(&["DST-PORT,443,PROXY", "MATCH,DIRECT"]), "example.com:443", None);
        assert_eq!(eval.winner.unwrap().policy, "PROXY");
    }

    #[test]
    fn ip_rule_with_domain_is_skipped() {
        let eval = evaluate(&rules(&["IP-CIDR,10.0.0.0/8,DIRECT"]), "example.com", None);
        assert!(eval.winner.is_none());
        assert_eq!(eval.skipped, 1);
    }

    #[test]
    fn normalizes_urls_and_ipv6_hosts() {
        assert_eq!(normalize("HTTPS://Example.COM:8443/x"), ("example.com".into(), Some(8443)));
        assert_eq!(normalize("[2001:db8::1]:7890"), ("2001:db8::1".into(), Some(7890)));
        assert_eq!(normalize("2001:db8::1"), ("2001:db8::1".into(), None));
    }
}

//! Import proxy share links (`vmess://`, `vless://`, `trojan://`, `ss://`,
//! `hysteria2://`) and turn them into a local Clash/Mihomo profile.
//!
//! Users migrating from v2rayN / Clash Verge often only have share links, not
//! a subscription. Instead of implementing the protocols (Mihomo already
//! does), this module only translates the link format into a `config.yaml`
//! that Mihomo understands.

use std::collections::BTreeMap;

use base64::Engine;
use percent_encoding::percent_decode_str;
use serde_json::{json, Map, Value};

/// Result of importing a block of share links.
pub struct ShareLinkImport {
    /// Generated profile YAML (mode/proxies/proxy-groups/rules).
    pub yaml: String,
    /// Names of the proxies that were parsed successfully, in input order.
    pub names: Vec<String>,
    /// Human readable problems for lines that could not be imported.
    pub errors: Vec<String>,
}

/// Parse every non-empty line and build a profile from the links that parsed.
///
/// A single malformed line must not stop the import: users paste whole lists.
/// Errors are returned alongside the successful proxies so the UI can show
/// what was skipped.
pub fn import_share_links(input: &str) -> ShareLinkImport {
    let mut proxies: Vec<Value> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut used_names: BTreeMap<String, u32> = BTreeMap::new();

    for (index, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_link(line) {
            Ok(mut proxy) => {
                let base = proxy
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Proxy")
                    .to_string();
                // Proxy names must be unique inside a Clash config.
                let count = used_names.entry(base.clone()).or_insert(0);
                *count += 1;
                let name = if *count == 1 {
                    base
                } else {
                    format!("{} ({})", base, count)
                };
                proxy["name"] = Value::String(name.clone());
                proxies.push(proxy);
                names.push(name);
            }
            Err(e) => errors.push(format!("Line {}: {}", index + 1, e)),
        }
    }

    let mut group_members: Vec<Value> = names.iter().map(|n| Value::String(n.clone())).collect();
    group_members.push(Value::String("DIRECT".to_string()));

    let config = json!({
        "mode": "rule",
        "proxies": proxies,
        "proxy-groups": [{
            "name": "Proxy",
            "type": "select",
            "proxies": group_members,
        }],
        "rules": ["MATCH,Proxy"],
    });

    // serde_json values serialize to YAML losslessly.
    let yaml = serde_yaml_ng::to_string(&config)
        .unwrap_or_else(|_| "mode: rule\nproxies: []\n".to_string());

    ShareLinkImport {
        yaml,
        names,
        errors,
    }
}

fn parse_link(link: &str) -> Result<Value, String> {
    let (scheme, rest) = link
        .split_once("://")
        .ok_or_else(|| "not a share link (missing scheme)".to_string())?;
    let scheme = scheme.to_ascii_lowercase();
    match scheme.as_str() {
        "vmess" => parse_vmess(rest),
        "vless" => parse_vless(rest),
        "trojan" => parse_trojan(rest),
        "ss" => parse_ss(rest),
        "hysteria2" | "hy2" => parse_hysteria2(rest),
        other => Err(format!("unsupported scheme '{}'", other)),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn b64decode(input: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = input.trim().trim_end_matches('=').to_string();
    let padded = {
        let mut s = cleaned.clone();
        while s.len() % 4 != 0 {
            s.push('=');
        }
        s
    };
    for engine in [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(bytes) = engine.decode(padded.as_bytes()) {
            return Ok(bytes);
        }
    }
    let _ = cleaned;
    Err("invalid base64 payload".to_string())
}

fn b64decode_text(input: &str) -> Result<String, String> {
    let bytes = b64decode(input)?;
    String::from_utf8(bytes).map_err(|_| "base64 payload is not UTF-8".to_string())
}

fn decode(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

/// Split `host:port` handling IPv6 literals (`[::1]:443`).
fn split_host_port(input: &str) -> Result<(String, u16), String> {
    let input = input.trim();
    let (host, port) = if let Some(stripped) = input.strip_prefix('[') {
        let end = stripped
            .find(']')
            .ok_or_else(|| "unterminated IPv6 address".to_string())?;
        let host = &stripped[..end];
        let rest = stripped[end + 1..].strip_prefix(':').unwrap_or("");
        (host.to_string(), rest)
    } else {
        let (host, port) = input
            .rsplit_once(':')
            .ok_or_else(|| "missing port".to_string())?;
        (host.to_string(), port)
    };
    let port: u16 = port
        .parse()
        .map_err(|_| format!("invalid port '{}'", port))?;
    if host.is_empty() {
        return Err("missing host".to_string());
    }
    Ok((host, port))
}

fn query_map(query: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(key.to_ascii_lowercase(), decode(value));
    }
    map
}

/// Split `userinfo@host:port[/path][?query][#fragment]`.
struct UrlParts {
    userinfo: String,
    host: String,
    port: u16,
    query: BTreeMap<String, String>,
    name: String,
}

fn split_url(rest: &str) -> Result<UrlParts, String> {
    let (without_fragment, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, b),
        None => (rest, ""),
    };
    let (without_query, query) = match without_fragment.split_once('?') {
        Some((a, b)) => (a, b),
        None => (without_fragment, ""),
    };
    // Drop an optional `/` path separator; these schemes do not use a path.
    let authority = without_query.trim_end_matches('/');
    let (userinfo, hostport) = authority
        .rsplit_once('@')
        .ok_or_else(|| "missing credentials/userinfo".to_string())?;
    let (host, port) = split_host_port(hostport)?;
    Ok(UrlParts {
        userinfo: decode(userinfo),
        host,
        port,
        query: query_map(query),
        name: decode(fragment),
    })
}

fn network_opts(
    proxy: &mut Map<String, Value>,
    network: &str,
    path: &str,
    host: &str,
) {
    let network = network.trim().to_ascii_lowercase();
    match network.as_str() {
        "ws" | "websocket" => {
            proxy.insert("network".into(), json!("ws"));
            let mut opts = Map::new();
            opts.insert(
                "path".into(),
                json!(if path.is_empty() { "/" } else { path }),
            );
            if !host.is_empty() {
                opts.insert("headers".into(), json!({ "Host": host }));
            }
            proxy.insert("ws-opts".into(), Value::Object(opts));
        }
        "grpc" => {
            proxy.insert("network".into(), json!("grpc"));
            // v2ray links reuse `path` for the gRPC service name.
            let service = if !path.is_empty() { path } else { host };
            proxy.insert("grpc-opts".into(), json!({ "grpc-service-name": service }));
        }
        "h2" | "http" => {
            proxy.insert("network".into(), json!("h2"));
            let mut opts = Map::new();
            opts.insert("path".into(), json!(if path.is_empty() { "/" } else { path }));
            if !host.is_empty() {
                opts.insert("host".into(), json!([host]));
            }
            proxy.insert("h2-opts".into(), Value::Object(opts));
        }
        _ => {}
    }
}

fn apply_alpn(proxy: &mut Map<String, Value>, query: &BTreeMap<String, String>) {
    if let Some(alpn) = query.get("alpn") {
        let list: Vec<Value> = alpn
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| json!(s))
            .collect();
        if !list.is_empty() {
            proxy.insert("alpn".into(), Value::Array(list));
        }
    }
}

// ---------------------------------------------------------------------------
// vmess:// (base64-encoded JSON, v2rayN "vmess" format)
// ---------------------------------------------------------------------------

fn parse_vmess(rest: &str) -> Result<Value, String> {
    let text = b64decode_text(rest)?;
    let obj: Value = serde_json::from_str(&text)
        .map_err(|e| format!("vmess payload is not valid JSON: {}", e))?;
    let get = |key: &str| -> String {
        match obj.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        }
    };

    let server = get("add");
    if server.is_empty() {
        return Err("vmess link has no server address".to_string());
    }
    let port_str = get("port");
    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("vmess link has invalid port '{}'", port_str))?;
    let uuid = get("id");
    if uuid.is_empty() {
        return Err("vmess link has no uuid".to_string());
    }

    let mut proxy = Map::new();
    proxy.insert("type".into(), json!("vmess"));
    proxy.insert("server".into(), json!(server));
    proxy.insert("port".into(), json!(port));
    proxy.insert("uuid".into(), json!(uuid));
    let aid = get("aid");
    let alter_id: u32 = aid.parse().unwrap_or(0);
    proxy.insert("alterId".into(), json!(alter_id));
    let cipher = get("scy");
    proxy.insert(
        "cipher".into(),
        json!(if cipher.is_empty() { "auto" } else { &cipher }),
    );
    proxy.insert("udp".into(), json!(true));

    let sni = get("sni");
    let host = get("host");
    let tls = get("tls").to_ascii_lowercase();
    if tls == "tls" || tls == "reality" {
        proxy.insert("tls".into(), json!(true));
        let servername = if !sni.is_empty() { sni } else { host.clone() };
        if !servername.is_empty() {
            proxy.insert("servername".into(), json!(servername));
        }
        let alpn = get("alpn");
        if !alpn.is_empty() {
            let list: Vec<Value> = alpn
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| json!(s))
                .collect();
            proxy.insert("alpn".into(), Value::Array(list));
        }
        let fp = get("fp");
        if !fp.is_empty() {
            proxy.insert("client-fingerprint".into(), json!(fp));
        }
    }

    let network = get("net");
    let path = get("path");
    network_opts(&mut proxy, &network, &path, &host);

    let name = {
        let ps = get("ps");
        if ps.is_empty() {
            format!("{}:{}", proxy["server"].as_str().unwrap_or(""), port)
        } else {
            ps
        }
    };
    proxy.insert("name".into(), json!(name));
    Ok(Value::Object(proxy))
}

// ---------------------------------------------------------------------------
// vless://
// ---------------------------------------------------------------------------

fn parse_vless(rest: &str) -> Result<Value, String> {
    let parts = split_url(rest)?;
    if parts.userinfo.is_empty() {
        return Err("vless link has no uuid".to_string());
    }

    let mut proxy = Map::new();
    proxy.insert("type".into(), json!("vless"));
    proxy.insert("server".into(), json!(parts.host));
    proxy.insert("port".into(), json!(parts.port));
    proxy.insert("uuid".into(), json!(parts.userinfo));
    proxy.insert("udp".into(), json!(true));

    let security = parts
        .query
        .get("security")
        .cloned()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let sni = parts.query.get("sni").cloned().unwrap_or_default();
    let host = parts.query.get("host").cloned().unwrap_or_default();
    if security == "tls" || security == "reality" {
        proxy.insert("tls".into(), json!(true));
        let servername = if !sni.is_empty() { sni } else { host.clone() };
        if !servername.is_empty() {
            proxy.insert("servername".into(), json!(servername));
        }
    }
    if security == "reality" {
        if let Some(pbk) = parts.query.get("pbk") {
            if !pbk.is_empty() {
                let mut opts = Map::new();
                opts.insert("public-key".into(), json!(pbk));
                if let Some(sid) = parts.query.get("sid") {
                    if !sid.is_empty() {
                        opts.insert("short-id".into(), json!(sid));
                    }
                }
                proxy.insert("reality-opts".into(), Value::Object(opts));
            }
        }
    }
    if let Some(flow) = parts.query.get("flow") {
        if !flow.is_empty() {
            proxy.insert("flow".into(), json!(flow));
        }
    }
    if let Some(fp) = parts.query.get("fp") {
        if !fp.is_empty() {
            proxy.insert("client-fingerprint".into(), json!(fp));
        }
    }
    if let Some(insecure) = parts.query.get("allowinsecure") {
        if insecure == "1" || insecure.eq_ignore_ascii_case("true") {
            proxy.insert("skip-cert-verify".into(), json!(true));
        }
    }
    apply_alpn(&mut proxy, &parts.query);

    let network = parts.query.get("type").cloned().unwrap_or_default();
    let path = parts.query.get("path").cloned().unwrap_or_default();
    let service = parts
        .query
        .get("servicename")
        .cloned()
        .unwrap_or(path);
    network_opts(&mut proxy, &network, &service, &host);

    let name = if parts.name.is_empty() {
        format!("{}:{}", parts.host, parts.port)
    } else {
        parts.name
    };
    proxy.insert("name".into(), json!(name));
    Ok(Value::Object(proxy))
}

// ---------------------------------------------------------------------------
// trojan://
// ---------------------------------------------------------------------------

fn parse_trojan(rest: &str) -> Result<Value, String> {
    let parts = split_url(rest)?;
    if parts.userinfo.is_empty() {
        return Err("trojan link has no password".to_string());
    }

    let mut proxy = Map::new();
    proxy.insert("type".into(), json!("trojan"));
    proxy.insert("server".into(), json!(parts.host));
    proxy.insert("port".into(), json!(parts.port));
    proxy.insert("password".into(), json!(parts.userinfo));
    proxy.insert("udp".into(), json!(true));

    let sni = parts.query.get("sni").cloned().unwrap_or_default();
    let host = parts.query.get("host").cloned().unwrap_or_default();
    let servername = if !sni.is_empty() { sni } else { host.clone() };
    if !servername.is_empty() {
        proxy.insert("sni".into(), json!(servername));
    }
    if let Some(insecure) = parts.query.get("allowinsecure") {
        if insecure == "1" || insecure.eq_ignore_ascii_case("true") {
            proxy.insert("skip-cert-verify".into(), json!(true));
        }
    }
    apply_alpn(&mut proxy, &parts.query);

    let network = parts.query.get("type").cloned().unwrap_or_default();
    let path = parts.query.get("path").cloned().unwrap_or_default();
    network_opts(&mut proxy, &network, &path, &host);

    let name = if parts.name.is_empty() {
        format!("{}:{}", parts.host, parts.port)
    } else {
        parts.name
    };
    proxy.insert("name".into(), json!(name));
    Ok(Value::Object(proxy))
}

// ---------------------------------------------------------------------------
// ss:// (SIP002 and legacy base64)
// ---------------------------------------------------------------------------

fn parse_ss(rest: &str) -> Result<Value, String> {
    let (body, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, b),
        None => (rest, ""),
    };
    let (body, query) = match body.split_once('?') {
        Some((a, b)) => (a, b),
        None => (body, ""),
    };
    let query = query_map(query);

    // SIP002: ss://base64(method:password)@host:port
    // Legacy:  ss://base64(method:password@host:port)
    let (userinfo, hostport) = if let Some((user, host)) = body.rsplit_once('@') {
        let decoded_user = if user.contains(':') {
            decode(user)
        } else {
            b64decode_text(user).unwrap_or_else(|_| decode(user))
        };
        (decoded_user, host.to_string())
    } else {
        let decoded = b64decode_text(body)?;
        let (user, host) = decoded
            .rsplit_once('@')
            .ok_or_else(|| "ss link has no host".to_string())?;
        (user.to_string(), host.to_string())
    };

    let (method, password) = userinfo
        .split_once(':')
        .ok_or_else(|| "ss link has no method/password".to_string())?;
    if method.is_empty() {
        return Err("ss link has no encryption method".to_string());
    }
    let (host, port) = split_host_port(&hostport)?;

    let mut proxy = Map::new();
    proxy.insert("type".into(), json!("ss"));
    proxy.insert("server".into(), json!(host));
    proxy.insert("port".into(), json!(port));
    proxy.insert("cipher".into(), json!(method));
    proxy.insert("password".into(), json!(password));
    proxy.insert("udp".into(), json!(true));

    // Optional SIP002 plugin (v2ray-plugin / obfs-local).
    if let Some(plugin) = query.get("plugin") {
        if let Some((plugin_name, opts)) = parse_ss_plugin(plugin) {
            proxy.insert("plugin".into(), json!(plugin_name));
            proxy.insert("plugin-opts".into(), Value::Object(opts));
        }
    }

    let name = if fragment.is_empty() {
        format!("{}:{}", proxy["server"].as_str().unwrap_or(""), port)
    } else {
        decode(fragment)
    };
    proxy.insert("name".into(), json!(name));
    Ok(Value::Object(proxy))
}

/// Translate a SIP002 plugin string (`v2ray-plugin;tls;host=x`) into
/// Mihomo's `plugin` + `plugin-opts` pair.
fn parse_ss_plugin(raw: &str) -> Option<(String, Map<String, Value>)> {
    let mut parts = raw.split(';');
    let name = parts.next()?.trim();
    let mut opts = Map::new();
    match name {
        "obfs-local" | "simple-obfs" | "obfs" => {
            for part in parts {
                if let Some(mode) = part.strip_prefix("obfs=") {
                    opts.insert("mode".into(), json!(mode));
                } else if let Some(host) = part.strip_prefix("obfs-host=") {
                    opts.insert("host".into(), json!(host));
                }
            }
            Some(("obfs".to_string(), opts))
        }
        "v2ray-plugin" => {
            opts.insert("mode".into(), json!("websocket"));
            for part in parts {
                if part.eq_ignore_ascii_case("tls") {
                    opts.insert("tls".into(), json!(true));
                } else if let Some(host) = part.strip_prefix("host=") {
                    opts.insert("host".into(), json!(decode(host)));
                } else if let Some(path) = part.strip_prefix("path=") {
                    opts.insert("path".into(), json!(decode(path)));
                }
            }
            Some(("v2ray-plugin".to_string(), opts))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// hysteria2:// / hy2://
// ---------------------------------------------------------------------------

fn parse_hysteria2(rest: &str) -> Result<Value, String> {
    let parts = split_url(rest)?;

    let mut proxy = Map::new();
    proxy.insert("type".into(), json!("hysteria2"));
    proxy.insert("server".into(), json!(parts.host));
    proxy.insert("port".into(), json!(parts.port));
    if !parts.userinfo.is_empty() {
        proxy.insert("password".into(), json!(parts.userinfo));
    }
    let sni = parts.query.get("sni").cloned().unwrap_or_default();
    let host = parts.query.get("host").cloned().unwrap_or_default();
    let servername = if !sni.is_empty() { sni } else { host };
    if !servername.is_empty() {
        proxy.insert("sni".into(), json!(servername));
    }
    if let Some(insecure) = parts.query.get("insecure") {
        if insecure == "1" || insecure.eq_ignore_ascii_case("true") {
            proxy.insert("skip-cert-verify".into(), json!(true));
        }
    }
    if let Some(obfs) = parts.query.get("obfs") {
        if !obfs.is_empty() {
            proxy.insert("obfs".into(), json!(obfs));
            if let Some(password) = parts.query.get("obfs-password") {
                proxy.insert("obfs-password".into(), json!(password));
            }
        }
    }
    if let Some(up) = parts.query.get("up") {
        if !up.is_empty() {
            proxy.insert("up".into(), json!(up));
        }
    }
    if let Some(down) = parts.query.get("down") {
        if !down.is_empty() {
            proxy.insert("down".into(), json!(down));
        }
    }
    // `insecure` is also commonly spelled `allowInsecure`.
    if let Some(insecure) = parts.query.get("allowinsecure") {
        if insecure == "1" || insecure.eq_ignore_ascii_case("true") {
            proxy.insert("skip-cert-verify".into(), json!(true));
        }
    }

    let name = if parts.name.is_empty() {
        format!("{}:{}", parts.host, parts.port)
    } else {
        parts.name
    };
    proxy.insert("name".into(), json!(name));
    Ok(Value::Object(proxy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_profile_with_unique_names() {
        let links = "\
vmess://eyJ2IjoiMiIsInBzIjoiVGVzdCIsImFkZCI6ImV4YW1wbGUuY29tIiwicG9ydCI6IjQ0MyIsImlkIjoiMTExMTExMTEtMTExMS0xMTExLTExMTEtMTExMTExMTExMTExIiwiYWlkIjoiMCIsIm5ldCI6IndzIiwiaG9zdCI6ImV4YW1wbGUuY29tIiwicGF0aCI6Ii93cyIsInRscyI6InRscyJ9
vless://22222222-2222-2222-2222-222222222222@example.org:443?encryption=none&security=tls&type=ws&host=example.org&path=%2Fvless&sni=example.org#VLESS%20Node
";
        let result = import_share_links(links);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.names, vec!["Test", "VLESS Node"]);
        assert!(result.yaml.contains("type: vmess"));
        assert!(result.yaml.contains("type: vless"));
        assert!(result.yaml.contains("MATCH,Proxy"));
        assert!(result.yaml.contains("ws-opts"));
    }

    #[test]
    fn parses_trojan_and_hysteria2() {
        let trojan = parse_link("trojan://secret@a.example:443?sni=a.example&type=ws&path=%2Ft#TJ")
            .unwrap();
        assert_eq!(trojan["type"], "trojan");
        assert_eq!(trojan["password"], "secret");
        assert_eq!(trojan["sni"], "a.example");
        assert_eq!(trojan["network"], "ws");

        let hy2 = parse_link("hysteria2://pw@b.example:8443?sni=b.example&insecure=1#HY2")
            .unwrap();
        assert_eq!(hy2["type"], "hysteria2");
        assert_eq!(hy2["port"], 8443);
        assert_eq!(hy2["skip-cert-verify"], true);
    }

    #[test]
    fn parses_ss_sip002_and_legacy() {
        // base64("aes-256-gcm:pass") = YWVzLTI1Ni1nY206cGFzcw==
        let sip = parse_link("ss://YWVzLTI1Ni1nY206cGFzcw==@1.2.3.4:8388#SS").unwrap();
        assert_eq!(sip["cipher"], "aes-256-gcm");
        assert_eq!(sip["password"], "pass");
        assert_eq!(sip["server"], "1.2.3.4");

        // base64("chacha20-ietf-poly1305:pw@5.6.7.8:443")
        let legacy_encoded =
            base64::engine::general_purpose::STANDARD.encode("chacha20-ietf-poly1305:pw@5.6.7.8:443");
        let legacy = parse_link(&format!("ss://{}#Legacy", legacy_encoded)).unwrap();
        assert_eq!(legacy["cipher"], "chacha20-ietf-poly1305");
        assert_eq!(legacy["server"], "5.6.7.8");
        assert_eq!(legacy["name"], "Legacy");
    }

    #[test]
    fn reports_bad_lines_without_aborting() {
        let result = import_share_links("bogus://x\nvmess://not-base64\n");
        assert!(result.names.is_empty());
        assert_eq!(result.errors.len(), 2);
        assert!(result.yaml.contains("proxies: []"));
    }

    #[test]
    fn split_host_port_supports_ipv6() {
        assert_eq!(
            split_host_port("[2001:db8::1]:8443").unwrap(),
            ("2001:db8::1".to_string(), 8443)
        );
        assert!(split_host_port("example.com").is_err());
    }
}

use anyhow::{anyhow, Result};
use reqwest::blocking::Response;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::PanelClient;
use crate::session::load_session;

const BASE: &str = "api/v2/hosts/firewall";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FireInfo {
    #[serde(default)]
    num: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    port: String,
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    strategy: String,
    #[serde(default)]
    used_status: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    target_ip: String,
    #[serde(default)]
    target_port: String,
    #[serde(default)]
    interface: String,
}

fn check_code(resp: &Value) -> Result<()> {
    if resp["code"].as_i64() != Some(200) {
        return Err(anyhow!(
            "面板返回错误 (code={}): {}",
            resp["code"].as_i64().unwrap_or(-1),
            resp["message"].as_str().unwrap_or("")
        ));
    }
    Ok(())
}

fn response_json(resp: Response, context: &str) -> Result<Value> {
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| anyhow!("{context}: HTTP {status}, 响应不是 JSON: {e}; {text}"))?;
    if !status.is_success() {
        return Err(anyhow!(
            "{context}: HTTP {status}: {}",
            value["message"].as_str().unwrap_or(&text)
        ));
    }
    check_code(&value)?;
    Ok(value)
}

fn post_json(client: &PanelClient, path: &str, body: &Value, context: &str) -> Result<Value> {
    response_json(client.post_json(path, body)?, context)
}

fn client_for(profile: &str) -> Result<PanelClient> {
    let session = load_session(profile)?;
    PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)
}

fn supports_unified_api(client: &PanelClient) -> bool {
    let Ok(resp) = client.get(&format!("{BASE}/settings")) else {
        return false;
    };
    if !resp.status().is_success() {
        return false;
    }
    resp.json::<Value>()
        .ok()
        .is_some_and(|v| v["code"].as_i64() == Some(200))
}

fn firewall_base(client: &PanelClient) -> Result<Value> {
    post_json(
        client,
        &format!("{BASE}/base"),
        &json!({"name": "base"}),
        "查询防火墙状态失败",
    )
}

fn firewall_provider(client: &PanelClient) -> Result<String> {
    let resp = firewall_base(client)?;
    let data = &resp["data"];
    let provider = data["backend"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| data["name"].as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match provider.as_str() {
        "iptables" | "nftables" | "firewalld" | "ufw" => Ok(provider),
        _ => Err(anyhow!("无法识别当前防火墙后端: {provider}")),
    }
}

pub fn status(profile: &str) -> Result<()> {
    let client = client_for(profile)?;
    let resp = firewall_base(&client)?;
    let d = &resp["data"];
    println!("防火墙: {}", d["name"].as_str().unwrap_or("-"));
    if let Some(backend) = d["backend"].as_str().filter(|v| !v.is_empty()) {
        println!("后端: {backend}");
    }
    println!(
        "状态: {}",
        if d["isActive"].as_bool() == Some(true) {
            "运行中"
        } else {
            "未运行"
        }
    );
    println!("版本: {}", d["version"].as_str().unwrap_or("-"));
    let ping = match d["pingStatus"].as_str() {
        Some("Enable") => "已禁止",
        Some("Disable") => "允许",
        _ => "未知",
    };
    println!("Ping: {ping}");
    println!(
        "初始化: {}",
        if d["isInit"].as_bool() == Some(true) {
            "是"
        } else {
            "否"
        }
    );
    if let Some(err) = d["message"].as_str().filter(|v| !v.is_empty()) {
        println!("消息: {err}");
    }
    if supports_unified_api(&client) {
        let resp = response_json(
            client.get(&format!("{BASE}/settings"))?,
            "查询防火墙后端设置失败",
        )?;
        let s = &resp["data"];
        let selected = |group: &Value| -> String {
            let sel = group["selected"].as_str().unwrap_or("-");
            let cur = group["current"].as_str().unwrap_or("");
            if !cur.is_empty() && cur != sel {
                format!("{sel}(当前 {cur})")
            } else {
                sel.to_string()
            }
        };
        println!("后端选择: 系统 {}", selected(&s["system"]));
        if let Some(forwarding) = s["forwarding"].as_object() {
            if !forwarding.is_empty() {
                println!("          转发 {}", selected(&s["forwarding"]));
            }
        }
        if let Some(docker) = s["docker"].as_object() {
            if !docker.is_empty() {
                println!("          Docker {}", selected(&s["docker"]));
            }
        }
    }
    Ok(())
}

pub fn operate(profile: &str, operation: &str) -> Result<()> {
    let client = client_for(profile)?;
    let op = match operation {
        "start" => "start",
        "stop" => "stop",
        "restart" => "restart",
        "allow-ping" => "disableBanPing",
        "ban-ping" => "enableBanPing",
        _ => return Err(anyhow!("不支持的操作 {operation}")),
    };
    post_json(
        &client,
        &format!("{BASE}/operate"),
        &json!({"operation": op, "withDockerRestart": false}),
        "操作防火墙失败",
    )?;
    let desc = match operation {
        "start" => "防火墙已启动",
        "stop" => "防火墙已停止",
        "restart" => "防火墙已重启",
        "allow-ping" => "已允许 Ping",
        "ban-ping" => "已禁止 Ping",
        _ => "操作完成",
    };
    println!("{desc}");
    Ok(())
}

pub struct ListOptions {
    pub r#type: String,
    pub info: String,
    pub strategy: String,
}

fn stable_list(client: &PanelClient, opts: &ListOptions) -> Result<()> {
    let body = json!({
        "page": 1,
        "pageSize": 500,
        "strategy": opts.strategy,
        "info": opts.info,
        "type": opts.r#type,
    });
    let resp = post_json(
        client,
        &format!("{BASE}/search"),
        &body,
        "查询防火墙规则失败",
    )?;
    let items: Vec<FireInfo> = serde_json::from_value(resp["data"]["items"].clone())
        .map_err(|e| anyhow!("解析防火墙规则失败: {e}"))?;
    print_stable_items(&opts.r#type, &items)?;
    println!(
        "共 {} 条规则",
        resp["data"]["total"].as_i64().unwrap_or(items.len() as i64)
    );
    Ok(())
}

fn print_stable_items(kind: &str, items: &[FireInfo]) -> Result<()> {
    match kind {
        "port" => {
            println!(
                "{:<10} {:<10} {:<10} {:<20} {:<10} 备注",
                "端口", "协议", "策略", "来源", "使用中"
            );
            for it in items {
                println!(
                    "{:<10} {:<10} {:<10} {:<20} {:<10} {}",
                    it.port,
                    it.protocol,
                    it.strategy,
                    it.address,
                    if it.used_status == "in_used" || it.used_status == "used" {
                        "是"
                    } else {
                        "否"
                    },
                    it.description
                );
            }
        }
        "forward" => {
            println!(
                "{:<6} {:<10} {:<24} {:<10} 网卡",
                "编号", "端口", "目标", "协议"
            );
            for it in items {
                println!(
                    "{:<6} {:<10} {:<24} {:<10} {}",
                    it.num,
                    it.port,
                    format!("{}:{}", it.target_ip, it.target_port),
                    it.protocol,
                    if it.interface.is_empty() {
                        "*"
                    } else {
                        &it.interface
                    }
                );
            }
        }
        "address" => {
            println!("{:<24} {:<10} 备注", "地址", "策略");
            for it in items {
                println!("{:<24} {:<10} {}", it.address, it.strategy, it.description);
            }
        }
        other => {
            return Err(anyhow!(
                "不支持的规则类型 {other}(可选 port / address / forward)"
            ))
        }
    }
    Ok(())
}

fn scopes(provider: &str) -> Vec<Value> {
    match provider {
        "iptables" | "nftables" => ["ipv4", "ipv6"]
            .into_iter()
            .flat_map(|family| {
                ["1PANEL_BASIC_BEFORE", "1PANEL_BASIC", "1PANEL_BASIC_AFTER"]
                    .into_iter()
                    .map(move |chain| {
                        json!({
                            "provider": provider,
                            "family": family,
                            "table": "filter",
                            "chain": chain,
                            "direction": "input"
                        })
                    })
            })
            .collect(),
        "firewalld" => vec![json!({
            "provider": provider,
            "family": "inet",
            "zone": "public",
            "direction": "input"
        })],
        "ufw" => ["ipv4", "ipv6"]
            .into_iter()
            .map(|family| {
                json!({
                    "provider": provider,
                    "family": family,
                    "chain": "incoming",
                    "direction": "input"
                })
            })
            .collect(),
        _ => vec![],
    }
}

fn unified_inventory(client: &PanelClient, provider: &str, info: &str) -> Result<Vec<Value>> {
    let body = json!({
        "page": 1,
        "pageSize": 500,
        "scopes": scopes(provider),
        "info": info,
        "all": true
    });
    let resp = post_json(
        client,
        &format!("{BASE}/rules/search"),
        &body,
        "查询新版防火墙规则失败",
    )?;
    Ok(resp["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn field<'a>(rule: &'a Value, name: &str) -> &'a str {
    rule[name].as_str().unwrap_or("")
}

fn matches_filter(rule: &Value, item: &Value, opts: &ListOptions) -> bool {
    let kind_match = match opts.r#type.as_str() {
        "port" => !field(rule, "destinationPort").is_empty(),
        "address" => {
            !field(rule, "sourceAddress").is_empty() && field(rule, "destinationPort").is_empty()
        }
        _ => false,
    };
    let strategy_match =
        opts.strategy.is_empty() || field(rule, "action").eq_ignore_ascii_case(&opts.strategy);
    let haystack = format!(
        "{} {} {} {} {} {}",
        field(rule, "sourceAddress"),
        field(rule, "destinationAddress"),
        field(rule, "sourcePort"),
        field(rule, "destinationPort"),
        field(rule, "description"),
        item["state"].as_str().unwrap_or("")
    )
    .to_ascii_lowercase();
    kind_match
        && strategy_match
        && (opts.info.is_empty() || haystack.contains(&opts.info.to_ascii_lowercase()))
}

fn unified_list(client: &PanelClient, opts: &ListOptions) -> Result<()> {
    if opts.r#type == "forward" {
        let resp = post_json(
            client,
            &format!("{BASE}/forward/search"),
            &json!({"page": 1, "pageSize": 500, "info": opts.info, "status": "", "strategy": opts.strategy}),
            "查询新版端口转发失败",
        )?;
        let items: Vec<FireInfo> = serde_json::from_value(resp["data"]["items"].clone())
            .map_err(|e| anyhow!("解析端口转发规则失败: {e}"))?;
        print_stable_items("forward", &items)?;
        println!(
            "共 {} 条规则",
            resp["data"]["total"].as_i64().unwrap_or(items.len() as i64)
        );
        return Ok(());
    }
    if opts.r#type != "port" && opts.r#type != "address" {
        return Err(anyhow!(
            "不支持的规则类型 {}(可选 port / address / forward)",
            opts.r#type
        ));
    }
    let provider = firewall_provider(client)?;
    let items = unified_inventory(client, &provider, &opts.info)?;
    let filtered: Vec<&Value> = items
        .iter()
        .filter(|item| matches_filter(&item["rule"], item, opts))
        .collect();
    if opts.r#type == "port" {
        println!(
            "{:<12} {:<10} {:<10} {:<24} {:<12} 备注",
            "端口", "协议", "策略", "来源", "状态"
        );
        for item in &filtered {
            let rule = &item["rule"];
            println!(
                "{:<12} {:<10} {:<10} {:<24} {:<12} {}",
                field(rule, "destinationPort"),
                field(rule, "protocol"),
                field(rule, "action"),
                if field(rule, "sourceAddress").is_empty() {
                    "Anywhere"
                } else {
                    field(rule, "sourceAddress")
                },
                item["state"].as_str().unwrap_or("-"),
                field(rule, "description")
            );
        }
    } else {
        println!("{:<24} {:<10} {:<12} 备注", "地址", "策略", "状态");
        for item in &filtered {
            let rule = &item["rule"];
            println!(
                "{:<24} {:<10} {:<12} {}",
                field(rule, "sourceAddress"),
                field(rule, "action"),
                item["state"].as_str().unwrap_or("-"),
                field(rule, "description")
            );
        }
    }
    println!(
        "共 {} 条规则(新版统一规则 API,后端 {provider})",
        filtered.len()
    );
    Ok(())
}

pub fn list(profile: &str, opts: &ListOptions) -> Result<()> {
    let client = client_for(profile)?;
    let mut normalized = ListOptions {
        r#type: if opts.r#type.is_empty() {
            "port".into()
        } else {
            opts.r#type.clone()
        },
        info: opts.info.clone(),
        strategy: opts.strategy.clone(),
    };
    normalized.r#type.make_ascii_lowercase();
    if supports_unified_api(&client) {
        unified_list(&client, &normalized)
    } else {
        stable_list(&client, &normalized)
    }
}

pub struct PortOptions {
    pub protocol: String,
    pub strategy: String,
    pub source: String,
    pub desc: String,
}

fn validate_strategy(strategy: &str) -> Result<String> {
    match strategy.to_ascii_lowercase().as_str() {
        "accept" | "allow" => Ok("accept".into()),
        "drop" | "deny" | "block" => Ok("drop".into()),
        _ => Err(anyhow!("不支持的策略 {strategy}(可选 accept / drop)")),
    }
}

fn protocols(protocol: &str) -> Result<Vec<&str>> {
    match protocol.to_ascii_lowercase().as_str() {
        "tcp" => Ok(vec!["tcp"]),
        "udp" => Ok(vec!["udp"]),
        "tcp/udp" | "udp/tcp" => Ok(vec!["tcp", "udp"]),
        _ => Err(anyhow!("不支持的协议 {protocol}(可选 tcp / udp / tcp/udp)")),
    }
}

fn address_family(address: &str) -> &'static str {
    if address.contains(':') {
        "ipv6"
    } else {
        "ipv4"
    }
}

fn rule_scope(provider: &str, family: &str) -> Value {
    match provider {
        "iptables" | "nftables" => json!({
            "provider": provider,
            "family": family,
            "table": "filter",
            "chain": "1PANEL_BASIC",
            "direction": "input"
        }),
        "firewalld" => json!({
            "provider": provider,
            "family": family,
            "zone": "public",
            "direction": "input"
        }),
        "ufw" => json!({
            "provider": provider,
            "family": family,
            "chain": "incoming",
            "direction": "input"
        }),
        _ => Value::Null,
    }
}

fn port_rules(provider: &str, port: &str, opts: &PortOptions) -> Result<Vec<Value>> {
    let action = validate_strategy(&opts.strategy)?;
    let family = if opts.source.is_empty() && provider == "firewalld" {
        "inet"
    } else {
        address_family(&opts.source)
    };
    Ok(protocols(&opts.protocol)?
        .into_iter()
        .map(|protocol| {
            json!({
                "scope": rule_scope(provider, family),
                "protocol": protocol,
                "sourceAddress": opts.source,
                "destinationPort": port,
                "action": action,
                "description": opts.desc
            })
        })
        .collect())
}

fn ip_rules(provider: &str, address: &str, opts: &IpOptions) -> Result<Vec<Value>> {
    Ok(vec![json!({
        "scope": rule_scope(provider, address_family(address)),
        "protocol": "all",
        "sourceAddress": address,
        "action": validate_strategy(&opts.strategy)?,
        "description": opts.desc
    })])
}

fn unified_create(client: &PanelClient, rules: Vec<Value>, label: &str) -> Result<()> {
    let check = post_json(
        client,
        &format!("{BASE}/rules/check"),
        &json!({"items": rules.into_iter().map(|rule| json!({"rule": rule})).collect::<Vec<_>>() }),
        "检查新版防火墙规则失败",
    )?;
    let plans = check["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut creates = Vec::new();
    let mut existing = 0usize;
    for plan in plans {
        let decision = plan["decision"].as_str().unwrap_or("");
        let classification = plan["classification"].as_str().unwrap_or("");
        if decision == "blocked" {
            return Err(anyhow!(
                "规则检查被阻止: {} ({classification})",
                plan["reason"].as_str().unwrap_or("未知原因")
            ));
        }
        if decision == "no_change"
            || classification == "exact_managed"
            || classification == "exact_external"
        {
            existing += 1;
            continue;
        }
        let can_create = plan["allowedActions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|v| v.as_str() == Some("create")));
        if !can_create {
            return Err(anyhow!("规则检查未允许 create 操作: {classification}"));
        }
        creates.push(json!({
            "rule": plan["requestedRule"].clone(),
            "checkFlag": plan["checkFlag"].clone(),
            "action": "create",
            "sourceKind": "user"
        }));
    }
    if creates.is_empty() {
        println!("{label} 已存在,无需重复添加({existing} 条)");
        return Ok(());
    }
    let resp = post_json(
        client,
        &format!("{BASE}/rules"),
        &json!({"items": creates}),
        "创建新版防火墙规则失败",
    )?;
    let failed = resp["data"]["failed"].as_u64().unwrap_or(0);
    if failed > 0 {
        return Err(anyhow!(
            "有 {failed} 条规则创建失败: {}",
            resp["data"]["errors"]
        ));
    }
    println!(
        "{label} 已添加: {} 条{}",
        resp["data"]["succeeded"].as_u64().unwrap_or(0),
        if existing > 0 {
            format!(", {existing} 条已存在")
        } else {
            String::new()
        }
    );
    Ok(())
}

fn stable_port(
    client: &PanelClient,
    operation: &str,
    port: &str,
    opts: &PortOptions,
) -> Result<()> {
    post_json(
        client,
        &format!("{BASE}/port"),
        &json!({
            "operation": operation,
            "address": opts.source,
            "port": port,
            "protocol": opts.protocol,
            "strategy": opts.strategy,
            "description": opts.desc
        }),
        "操作端口规则失败",
    )?;
    println!(
        "端口规则已{}: {port}/{}(策略 {})",
        if operation == "add" {
            "添加"
        } else {
            "删除"
        },
        opts.protocol,
        opts.strategy
    );
    Ok(())
}

fn address_eq(actual: &str, expected: &str) -> bool {
    let actual = actual.split('/').next().unwrap_or(actual);
    let expected = expected.split('/').next().unwrap_or(expected);
    actual.eq_ignore_ascii_case(expected)
}

fn protocol_eq(actual: &str, expected: &str) -> bool {
    expected
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(actual))
}

fn unified_delete(client: &PanelClient, uuids: Vec<String>, label: &str) -> Result<()> {
    if uuids.is_empty() {
        return Err(anyhow!(
            "未找到可删除的托管规则: {label};外部或受保护规则不能由 CLI 删除"
        ));
    }
    let resp = post_json(
        client,
        &format!("{BASE}/rules/delete"),
        &json!({"uuids": uuids}),
        "删除新版防火墙规则失败",
    )?;
    let failed = resp["data"]["failed"].as_u64().unwrap_or(0);
    if failed > 0 {
        return Err(anyhow!(
            "有 {failed} 条规则删除失败: {}",
            resp["data"]["errors"]
        ));
    }
    println!(
        "{label} 已删除: {} 条",
        resp["data"]["succeeded"].as_u64().unwrap_or(0)
    );
    Ok(())
}

pub fn port_add(profile: &str, port: &str, opts: &PortOptions) -> Result<()> {
    let client = client_for(profile)?;
    if supports_unified_api(&client) {
        let provider = firewall_provider(&client)?;
        unified_create(
            &client,
            port_rules(&provider, port, opts)?,
            &format!("端口规则 {port}/{}", opts.protocol),
        )
    } else {
        stable_port(&client, "add", port, opts)
    }
}

pub fn port_del(profile: &str, port: &str, opts: &PortOptions) -> Result<()> {
    let client = client_for(profile)?;
    if !supports_unified_api(&client) {
        return stable_port(&client, "remove", port, opts);
    }
    let provider = firewall_provider(&client)?;
    let items = unified_inventory(&client, &provider, port)?;
    let wanted_action = validate_strategy(&opts.strategy)?;
    let uuids = items
        .iter()
        .filter(|item| {
            let rule = &item["rule"];
            field(rule, "destinationPort") == port
                && protocol_eq(field(rule, "protocol"), &opts.protocol)
                && address_eq(field(rule, "sourceAddress"), &opts.source)
                && field(rule, "action") == wanted_action
        })
        .filter_map(|item| item["desired"]["uuid"].as_str().map(str::to_string))
        .collect();
    unified_delete(
        &client,
        uuids,
        &format!("端口规则 {port}/{}", opts.protocol),
    )
}

pub struct IpOptions {
    pub strategy: String,
    pub desc: String,
}

fn stable_ip(client: &PanelClient, operation: &str, address: &str, opts: &IpOptions) -> Result<()> {
    post_json(
        client,
        &format!("{BASE}/ip"),
        &json!({
            "operation": operation,
            "address": address,
            "strategy": opts.strategy,
            "description": opts.desc
        }),
        "操作 IP 规则失败",
    )?;
    println!(
        "IP 规则已{}: {address}(策略 {})",
        if operation == "add" {
            "添加"
        } else {
            "删除"
        },
        opts.strategy
    );
    Ok(())
}

pub fn ip_add(profile: &str, address: &str, opts: &IpOptions) -> Result<()> {
    let client = client_for(profile)?;
    if supports_unified_api(&client) {
        let provider = firewall_provider(&client)?;
        unified_create(
            &client,
            ip_rules(&provider, address, opts)?,
            &format!("IP 规则 {address}"),
        )
    } else {
        stable_ip(&client, "add", address, opts)
    }
}

pub fn ip_del(profile: &str, address: &str, opts: &IpOptions) -> Result<()> {
    let client = client_for(profile)?;
    if !supports_unified_api(&client) {
        return stable_ip(&client, "remove", address, opts);
    }
    let provider = firewall_provider(&client)?;
    let items = unified_inventory(&client, &provider, address)?;
    let wanted_action = validate_strategy(&opts.strategy)?;
    let uuids = items
        .iter()
        .filter(|item| {
            let rule = &item["rule"];
            field(rule, "destinationPort").is_empty()
                && address_eq(field(rule, "sourceAddress"), address)
                && field(rule, "action") == wanted_action
        })
        .filter_map(|item| item["desired"]["uuid"].as_str().map(str::to_string))
        .collect();
    unified_delete(&client, uuids, &format!("IP 规则 {address}"))
}

pub struct ForwardOptions {
    pub protocol: String,
    pub target: String,
    pub interface: String,
    pub num: String,
}

fn parse_target(target: &str) -> Result<(String, String)> {
    let (ip, port) = target
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("目标格式应为 IP:端口,如 10.0.0.2:8080"))?;
    Ok((ip.to_string(), port.to_string()))
}

fn operate_forward(
    client: &PanelClient,
    unified: bool,
    operation: &str,
    port: &str,
    opts: &ForwardOptions,
) -> Result<()> {
    let (target_ip, target_port) = parse_target(&opts.target)?;
    let body = json!({
        "rules": [{
            "operation": operation,
            "num": opts.num,
            "protocol": opts.protocol,
            "port": port,
            "targetIP": target_ip,
            "targetPort": target_port,
            "interface": opts.interface
        }],
        "forceDelete": false
    });
    let path = if unified {
        format!("{BASE}/forward/operate")
    } else {
        format!("{BASE}/forward")
    };
    post_json(client, &path, &body, "操作端口转发失败")?;
    println!(
        "端口转发已{}: {port}/{} -> {}:{}",
        if operation == "add" {
            "添加"
        } else {
            "删除"
        },
        opts.protocol,
        target_ip,
        target_port
    );
    Ok(())
}

pub fn forward_add(profile: &str, port: &str, opts: &ForwardOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_forward(&client, supports_unified_api(&client), "add", port, opts)
}

pub fn forward_del(profile: &str, port: &str, opts: &ForwardOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_forward(&client, supports_unified_api(&client), "remove", port, opts)
}

pub fn batch_add(profile: &str, ports: &str, opts: &PortOptions) -> Result<()> {
    let client = client_for(profile)?;
    if supports_unified_api(&client) {
        let provider = firewall_provider(&client)?;
        let mut rules = Vec::new();
        for port in ports.split(',').map(str::trim).filter(|v| !v.is_empty()) {
            rules.extend(port_rules(&provider, port, opts)?);
        }
        if rules.is_empty() {
            return Err(anyhow!("至少提供一个端口"));
        }
        return unified_create(
            &client,
            rules,
            &format!("批量端口规则 {ports}/{}", opts.protocol),
        );
    }
    let rules: Vec<Value> = ports
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|port| {
            json!({
                "operation": "add",
                "address": opts.source,
                "port": port,
                "protocol": opts.protocol,
                "strategy": opts.strategy,
                "description": opts.desc
            })
        })
        .collect();
    post_json(
        &client,
        &format!("{BASE}/batch"),
        &json!({"type": "port", "rules": rules}),
        "批量添加端口规则失败",
    )?;
    println!("已批量添加端口规则: {ports}/{}", opts.protocol);
    Ok(())
}

// ---------- Docker 端口守护(新版统一防火墙 API) ----------

fn unified_api_required(client: &PanelClient) -> Result<()> {
    if !supports_unified_api(client) {
        return Err(anyhow!(
            "当前面板不支持新版统一防火墙 API(未检测到 /hosts/firewall/settings);\
             Docker 端口守护需要新版 1Panel"
        ));
    }
    Ok(())
}

fn docker_base(client: &PanelClient) -> Result<Value> {
    response_json(
        client.get(&format!("{BASE}/docker/ports"))?,
        "查询 Docker 端口守护状态失败",
    )
}

fn bool_zh(v: Option<bool>) -> &'static str {
    if v == Some(true) {
        "是"
    } else {
        "否"
    }
}

pub fn docker_status(profile: &str) -> Result<()> {
    let client = client_for(profile)?;
    unified_api_required(&client)?;
    let resp = docker_base(&client)?;
    let base = &resp["data"]["base"];
    println!(
        "Docker 端口守护: {}",
        base["name"].as_str().unwrap_or("-")
    );
    println!("后端: {}", base["backend"].as_str().unwrap_or("-"));
    println!("版本: {}", base["version"].as_str().unwrap_or("-"));
    println!(
        "存在: {}, 初始化: {}, 绑定: {}",
        bool_zh(base["isExist"].as_bool()),
        bool_zh(base["initialized"].as_bool()),
        bool_zh(base["bound"].as_bool())
    );
    for (key, label) in [("ipv4", "IPv4"), ("ipv6", "IPv6")] {
        let fam = &base[key];
        let reason = fam["reason"].as_str().unwrap_or("");
        let detail = if !reason.is_empty() {
            format!("({reason})")
        } else {
            format!(
                "状态 {} 初始化 {} 绑定 {} 生效 {}",
                fam["state"].as_str().unwrap_or("-"),
                bool_zh(fam["initialized"].as_bool()),
                bool_zh(fam["bound"].as_bool()),
                bool_zh(fam["effective"].as_bool())
            )
        };
        println!("{label}: {detail}");
    }
    if let Some(msg) = base["message"].as_str().filter(|v| !v.is_empty()) {
        println!("消息: {msg}");
    }
    let containers = resp["data"]["containers"].as_array().map(|v| v.len()).unwrap_or(0);
    let orphans = resp["data"]["orphanPolicies"].as_array().map(|v| v.len()).unwrap_or(0);
    println!("容器: {containers} 个, 孤立策略: {orphans} 条");
    Ok(())
}

fn endpoint_line(e: &Value) -> String {
    let family = e["family"].as_str().unwrap_or("ipv4");
    let host_ip = e["hostIP"].as_str().unwrap_or("");
    let host_port = e["hostPort"].as_u64().unwrap_or(0);
    let protocol = e["protocol"].as_str().unwrap_or("");
    let port_display = if host_ip.is_empty() {
        format!("{host_port}/{protocol}")
    } else if family == "ipv6" {
        format!("[{host_ip}]:{host_port}/{protocol}")
    } else {
        format!("{host_ip}:{host_port}/{protocol}")
    };
    port_display
}

fn docker_mode_zh(mode: &str) -> String {
    match mode {
        "allow_sources" => "仅允许来源".to_string(),
        "deny_sources" => "拒绝来源".to_string(),
        "deny_all" => "拒绝所有".to_string(),
        other => other.to_string(),
    }
}

pub fn docker_ports(profile: &str) -> Result<()> {
    let client = client_for(profile)?;
    unified_api_required(&client)?;
    let resp = response_json(
        client.get(&format!("{BASE}/docker/endpoints"))?,
        "查询 Docker 已发布端口失败",
    )?;
    let containers = resp["data"].as_array().cloned().unwrap_or_default();
    let mut total = 0usize;
    for c in &containers {
        let name = c["name"].as_str().unwrap_or("-");
        let app = c["application"].as_str().unwrap_or("");
        let compose = c["compose"].as_str().unwrap_or("");
        let owner = if !app.is_empty() {
            format!("应用 {app}")
        } else if !compose.is_empty() {
            format!("Compose {compose}")
        } else {
            "容器".to_string()
        };
        println!("\n{name} ({owner}):");
        let endpoints = c["endpoints"].as_array().cloned().unwrap_or_default();
        total += endpoints.len();
        for e in &endpoints {
            let mode = e["mode"].as_str().unwrap_or("-");
            let sources = e["sources"]
                .as_array()
                .map(|s| {
                    s.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            let sources = if sources.is_empty() {
                "-".to_string()
            } else {
                sources
            };
            let uuid = e["policyUUID"].as_str().unwrap_or("");
            println!(
                "  {:<28} {:<14} {:<8} {}",
                endpoint_line(e),
                docker_mode_zh(mode),
                if e["effective"].as_bool() == Some(true) {
                    "生效"
                } else {
                    "未生效"
                },
                if uuid.is_empty() { String::new() } else { format!("uuid={uuid}") }
            );
            if !sources.is_empty() || !e["description"].as_str().unwrap_or("").is_empty() {
                let desc = e["description"].as_str().unwrap_or("");
                if !sources.is_empty() {
                    println!("    来源: {sources}");
                }
                if !desc.is_empty() {
                    println!("    备注: {desc}");
                }
            }
        }
    }
    println!("\n共 {total} 个已发布端口({} 个容器)", containers.len());
    Ok(())
}

pub fn docker_sync(profile: &str) -> Result<()> {
    let client = client_for(profile)?;
    unified_api_required(&client)?;
    post_json(
        &client,
        &format!("{BASE}/docker/sync"),
        &json!({}),
        "同步 Docker 端口防护规则失败",
    )?;
    println!("Docker 端口防护规则已同步");
    Ok(())
}

pub fn docker_operate(profile: &str, operation: &str) -> Result<()> {
    let op = match operation {
        "init" | "initialize" => "initialize",
        "bind" => "bind",
        "unbind" => "unbind",
        _ => {
            return Err(anyhow!(
                "不支持的操作 {operation}(可选 init / bind / unbind)"
            ))
        }
    };
    let client = client_for(profile)?;
    unified_api_required(&client)?;
    post_json(
        &client,
        &format!("{BASE}/docker/operate"),
        &json!({"operation": op, "taskID": ""}),
        "操作 Docker 端口守护失败",
    )?;
    println!(
        "Docker 端口守护已{}",
        match op {
            "initialize" => "初始化",
            "bind" => "绑定",
            _ => "解绑",
        }
    );
    Ok(())
}

fn parse_docker_port(port: &str) -> Result<(String, u64)> {
    let port = port.trim();
    if let Ok(num) = port.parse::<u64>() {
        return Ok((String::new(), num));
    }
    if let Some((ip, num)) = port.rsplit_once(':') {
        let ip = ip.trim().trim_start_matches('[').trim_end_matches(']');
        if !ip.is_empty() {
            let num: u64 = num
                .parse()
                .map_err(|_| anyhow!("端口格式错误: {port}(应为数字或 IP:端口)"))?;
            return Ok((ip.to_string(), num));
        }
    }
    Err(anyhow!(
        "端口格式错误: {port}(应为 8080 或 0.0.0.0:8080)"
    ))
}

fn docker_published_endpoints(
    client: &PanelClient,
    port: &str,
    protocol: &str,
) -> Result<Vec<Value>> {
    let (host_ip, host_port) = parse_docker_port(port)?;
    let protos = protocols(protocol)?;
    let resp = response_json(
        client.get(&format!("{BASE}/docker/endpoints"))?,
        "查询 Docker 已发布端口失败",
    )?;
    let containers = resp["data"].as_array().cloned().unwrap_or_default();
    let mut endpoints: Vec<Value> = Vec::new();
    for c in &containers {
        for e in c["endpoints"].as_array().cloned().unwrap_or_default() {
            let ip_ok = host_ip.is_empty()
                || e["hostIP"].as_str().unwrap_or("").eq_ignore_ascii_case(&host_ip);
            let port_ok = e["hostPort"].as_u64() == Some(host_port);
            let proto_ok = protos
                .iter()
                .any(|p| e["protocol"].as_str() == Some(p));
            if ip_ok && port_ok && proto_ok {
                endpoints.push(json!({
                    "family": e["family"].as_str().unwrap_or("ipv4"),
                    "hostIP": e["hostIP"].as_str().unwrap_or(""),
                    "hostPort": e["hostPort"].as_u64().unwrap_or(0),
                    "protocol": e["protocol"].as_str().unwrap_or("tcp"),
                }));
            }
        }
    }
    if endpoints.is_empty() {
        return Err(anyhow!(
            "未找到已发布的端口 {port}/{};可先运行 `firewall docker ports` 查看实际端口",
            protocol
        ));
    }
    Ok(endpoints)
}

pub fn docker_apply(
    profile: &str,
    port: &str,
    protocol: &str,
    sources: &str,
    desc: &str,
    mode: &str,
) -> Result<()> {
    let client = client_for(profile)?;
    unified_api_required(&client)?;
    let endpoints = docker_published_endpoints(&client, port, protocol)?;
    let source_list: Vec<String> = sources
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let real_mode = match mode {
        "allow_sources" => {
            if source_list.is_empty() {
                return Err(anyhow!("放行操作需要 --sources 指定允许的来源 IP"));
            }
            "allow_sources"
        }
        "deny_sources" => {
            if source_list.is_empty() {
                return Err(anyhow!("拒绝指定来源需要 --sources;拒绝所有来源请省略 --sources"));
            }
            "deny_sources"
        }
        _ => "deny_all",
    };
    let body = json!({
        "endpoints": endpoints,
        "mode": real_mode,
        "sources": source_list,
        "description": desc,
    });
    post_json(
        &client,
        &format!("{BASE}/docker/policies/batch"),
        &body,
        "更新 Docker 端口防护策略失败",
    )?;
    println!(
        "已更新 {} 条 Docker 端口防护策略: {}",
        endpoints.len(),
        docker_mode_zh(real_mode)
    );
    if !source_list.is_empty() {
        println!("来源: {}", source_list.join(", "));
    }
    Ok(())
}

pub fn docker_policy_del(profile: &str, uuid: &str) -> Result<()> {
    if uuid.trim().is_empty() {
        return Err(anyhow!("请通过 --uuid 指定策略 UUID(firewall docker ports 中查看)"));
    }
    let client = client_for(profile)?;
    unified_api_required(&client)?;
    post_json(
        &client,
        &format!("{BASE}/docker/policies/delete/batch"),
        &json!({"uuids": [uuid]}),
        "删除 Docker 端口防护策略失败",
    )?;
    println!("已删除 Docker 端口防护策略: {uuid}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_new_firewall_scopes() {
        assert_eq!(scopes("iptables").len(), 6);
        assert_eq!(scopes("firewalld").len(), 1);
        assert_eq!(scopes("ufw").len(), 2);
    }

    #[test]
    fn expands_tcp_udp_port_rule() {
        let opts = PortOptions {
            protocol: "tcp/udp".into(),
            strategy: "accept".into(),
            source: String::new(),
            desc: "test".into(),
        };
        let rules = port_rules("iptables", "8080", &opts).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["scope"]["chain"], "1PANEL_BASIC");
    }

    #[test]
    fn matches_normalized_host_addresses() {
        assert!(address_eq("1.2.3.4/32", "1.2.3.4"));
        assert!(address_eq("2001:db8::1/128", "2001:db8::1"));
    }
}

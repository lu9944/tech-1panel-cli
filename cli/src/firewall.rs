use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::PanelClient;
use crate::session::load_session;

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

fn client_for(profile: &str) -> Result<PanelClient> {
    let session = load_session(profile)?;
    PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)
}

pub fn status(profile: &str) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({"name": "base"});
    let resp: Value = client
        .post_json("api/v2/hosts/firewall/base", &body)?
        .json()
        .map_err(|e| anyhow!("查询防火墙状态失败: {e}"))?;
    check_code(&resp)?;
    let d = &resp["data"];
    println!("防火墙: {}", d["name"].as_str().unwrap_or("-"));
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
        _ => {
            return Err(anyhow!(
                "不支持的操作 {operation}(可选 start / stop / restart / allow-ping / ban-ping)"
            ));
        }
    };
    let body = json!({"operation": op, "withDockerRestart": false});
    let resp: Value = client
        .post_json("api/v2/hosts/firewall/operate", &body)?
        .json()
        .map_err(|e| anyhow!("操作防火墙失败: {e}"))?;
    check_code(&resp)?;
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

pub fn list(profile: &str, opts: &ListOptions) -> Result<()> {
    let client = client_for(profile)?;
    let r#type = if opts.r#type.is_empty() {
        "port".to_string()
    } else {
        opts.r#type.clone()
    };
    let body = json!({
        "page": 1,
        "pageSize": 500,
        "strategy": opts.strategy,
        "info": opts.info,
        "type": r#type,
    });
    let resp: Value = client
        .post_json("api/v2/hosts/firewall/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询防火墙规则失败: {e}"))?;
    check_code(&resp)?;
    let items: Vec<FireInfo> = match resp["data"]["items"].as_array() {
        Some(arr) => serde_json::from_value(serde_json::Value::Array(arr.clone()))
            .map_err(|e| anyhow!("解析防火墙规则失败: {e}"))?,
        None => vec![],
    };
    match r#type.as_str() {
        "port" => {
            println!(
                "{:<10} {:<10} {:<10} {:<10} {:<10} 备注",
                "端口", "协议", "策略", "来源", "使用中"
            );
            for it in &items {
                println!(
                    "{:<10} {:<10} {:<10} {:<10} {:<10} {}",
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
            println!("{:<6} {:<10} {:<20} {:<10} 网卡", "编号", "端口", "目标", "协议");
            for it in &items {
                println!(
                    "{:<6} {:<10} {:<20} {:<10} {}",
                    it.num,
                    it.port,
                    format!("{}:{}", it.target_ip, it.target_port),
                    it.protocol,
                    if it.interface.is_empty() { "*" } else { &it.interface }
                );
            }
        }
        "address" => {
            println!("{:<20} {:<10} 备注", "地址", "策略");
            for it in &items {
                println!("{:<20} {:<10} {}", it.address, it.strategy, it.description);
            }
        }
        other => {
            return Err(anyhow!("不支持的规则类型 {other}(可选 port / address / forward)"));
        }
    }
    let total = resp["data"]["total"].as_i64().unwrap_or(items.len() as i64);
    println!("共 {total} 条规则");
    Ok(())
}

pub struct PortOptions {
    pub protocol: String,
    pub strategy: String,
    pub source: String,
    pub desc: String,
}

fn operate_port(client: &PanelClient, operation: &str, port: &str, opts: &PortOptions) -> Result<()> {
    let body = json!({
        "operation": operation,
        "address": opts.source,
        "port": port,
        "protocol": opts.protocol,
        "strategy": opts.strategy,
        "description": opts.desc,
    });
    let resp: Value = client
        .post_json("api/v2/hosts/firewall/port", &body)?
        .json()
        .map_err(|e| anyhow!("操作端口规则失败: {e}"))?;
    check_code(&resp)?;
    println!(
        "端口规则已{}: {port}/{}(策略 {})",
        if operation == "add" { "添加" } else { "删除" },
        opts.protocol,
        opts.strategy
    );
    Ok(())
}

pub fn port_add(profile: &str, port: &str, opts: &PortOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_port(&client, "add", port, opts)
}

pub fn port_del(profile: &str, port: &str, opts: &PortOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_port(&client, "remove", port, opts)
}

pub struct IpOptions {
    pub strategy: String,
    pub desc: String,
}

fn operate_ip(client: &PanelClient, operation: &str, address: &str, opts: &IpOptions) -> Result<()> {
    let body = json!({
        "operation": operation,
        "address": address,
        "strategy": opts.strategy,
        "description": opts.desc,
    });
    let resp: Value = client
        .post_json("api/v2/hosts/firewall/ip", &body)?
        .json()
        .map_err(|e| anyhow!("操作 IP 规则失败: {e}"))?;
    check_code(&resp)?;
    println!(
        "IP 规则已{}: {address}(策略 {})",
        if operation == "add" { "添加" } else { "删除" },
        opts.strategy
    );
    Ok(())
}

pub fn ip_add(profile: &str, address: &str, opts: &IpOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_ip(&client, "add", address, opts)
}

pub fn ip_del(profile: &str, address: &str, opts: &IpOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_ip(&client, "remove", address, opts)
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
            "interface": opts.interface,
        }],
        "forceDelete": false,
    });
    let resp: Value = client
        .post_json("api/v2/hosts/firewall/forward", &body)?
        .json()
        .map_err(|e| anyhow!("操作端口转发失败: {e}"))?;
    check_code(&resp)?;
    println!(
        "端口转发已{}: {port}/{} -> {}:{}",
        if operation == "add" { "添加" } else { "删除" },
        opts.protocol,
        target_ip,
        target_port
    );
    Ok(())
}

pub fn forward_add(profile: &str, port: &str, opts: &ForwardOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_forward(&client, "add", port, opts)
}

pub fn forward_del(profile: &str, port: &str, opts: &ForwardOptions) -> Result<()> {
    let client = client_for(profile)?;
    operate_forward(&client, "remove", port, opts)
}

pub fn batch_add(profile: &str, ports: &str, opts: &PortOptions) -> Result<()> {
    let client = client_for(profile)?;
    let rules: Vec<Value> = ports
        .split(',')
        .map(|p| {
            json!({
                "operation": "add",
                "address": opts.source,
                "port": p.trim(),
                "protocol": opts.protocol,
                "strategy": opts.strategy,
                "description": opts.desc,
            })
        })
        .collect();
    let body = json!({"type": "port", "rules": rules});
    let resp: Value = client
        .post_json("api/v2/hosts/firewall/batch", &body)?
        .json()
        .map_err(|e| anyhow!("批量添加端口规则失败: {e}"))?;
    check_code(&resp)?;
    println!("已批量添加端口规则: {ports}/{}", opts.protocol);
    Ok(())
}

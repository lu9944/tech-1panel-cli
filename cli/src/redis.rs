use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::client::PanelClient;
use crate::session::load_session;

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

pub fn list(profile: &str) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({"page": 1, "pageSize": 100, "name": "", "type": "", "tags": []});
    let resp: Value = client
        .post_json("api/v2/apps/installed/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询 Redis 实例失败: {e}"))?;
    check_code(&resp)?;
    let items = resp["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    println!("{:<8} {:<16} {:<12} {:<10}", "ID", "名称", "状态", "版本");
    for it in &items {
        if it["appKey"].as_str() != Some("redis") {
            continue;
        }
        println!(
            "{:<8} {:<16} {:<12} {:<10}",
            it["id"].as_u64().unwrap_or(0),
            it["name"].as_str().unwrap_or(""),
            it["status"].as_str().unwrap_or(""),
            it["version"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

pub fn info(profile: &str, name: &str) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({"type": "redis", "name": name});
    let resp: Value = client
        .post_json("api/v2/databases/redis/status", &body)?
        .json()
        .map_err(|e| anyhow!("查询 Redis 状态失败: {e}"))?;
    check_code(&resp)?;
    let st = &resp["data"];
    let fmt_bytes = |v: &str| -> String {
        let bytes: u64 = v.parse().unwrap_or(0);
        if bytes >= 1 << 30 {
            format!("{:.2} GB", bytes as f64 / (1 << 30) as f64)
        } else if bytes >= 1 << 20 {
            format!("{:.2} MB", bytes as f64 / (1 << 20) as f64)
        } else if bytes >= 1 << 10 {
            format!("{:.2} KB", bytes as f64 / (1 << 10) as f64)
        } else {
            format!("{bytes} B")
        }
    };
    println!("Redis 实例: {name}");
    println!("端口: {}", st["tcp_port"].as_str().unwrap_or(""));
    println!("运行天数: {}", st["uptime_in_days"].as_str().unwrap_or(""));
    println!(
        "当前连接数: {}",
        st["connected_clients"].as_str().unwrap_or("")
    );
    println!(
        "内存使用: {}",
        fmt_bytes(st["used_memory"].as_str().unwrap_or(""))
    );
    println!(
        "累计命令数: {}",
        st["total_commands_processed"].as_str().unwrap_or("")
    );

    let resp: Value = client
        .post_json("api/v2/databases/redis/conf", &body)?
        .json()
        .map_err(|e| anyhow!("查询 Redis 配置失败: {e}"))?;
    check_code(&resp)?;
    let conf = &resp["data"];
    println!("\n配置:");
    println!("  端口: {}", conf["port"].as_i64().unwrap_or(0));
    println!("  容器: {}", conf["containerName"].as_str().unwrap_or(""));
    println!("  timeout: {}", conf["timeout"].as_str().unwrap_or(""));
    println!(
        "  maxclients: {}",
        conf["maxclients"].as_str().unwrap_or("")
    );
    println!("  maxmemory: {}", conf["maxmemory"].as_str().unwrap_or(""));
    println!(
        "  requirepass: {}",
        conf["requirepass"].as_str().unwrap_or("")
    );
    Ok(())
}

pub struct ConfigOptions {
    pub set: Vec<(String, String)>,
}

pub fn config(profile: &str, name: &str, opts: &ConfigOptions) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({"type": "redis", "name": name});
    let resp: Value = client
        .post_json("api/v2/databases/redis/conf", &body)?
        .json()
        .map_err(|e| anyhow!("查询 Redis 配置失败: {e}"))?;
    check_code(&resp)?;
    let conf = &resp["data"];

    if opts.set.is_empty() {
        println!("Redis 配置({name}):");
        println!("  timeout    = {}", conf["timeout"].as_str().unwrap_or(""));
        println!(
            "  maxclients = {}",
            conf["maxclients"].as_str().unwrap_or("")
        );
        println!(
            "  maxmemory  = {}",
            conf["maxmemory"].as_str().unwrap_or("")
        );
        println!(
            "  requirepass= {}",
            conf["requirepass"].as_str().unwrap_or("")
        );
        return Ok(());
    }

    let mut update = json!({
        "database": name,
        "dbType": "redis",
        "timeout": conf["timeout"].as_str().unwrap_or(""),
        "maxclients": conf["maxclients"].as_str().unwrap_or(""),
        "maxmemory": conf["maxmemory"].as_str().unwrap_or(""),
    });
    for (k, v) in &opts.set {
        match k.as_str() {
            "timeout" | "maxclients" | "maxmemory" => {
                update[k] = json!(v);
            }
            _ => {
                return Err(anyhow!(
                    "不支持的配置项 {k},仅支持 timeout / maxclients / maxmemory"
                ));
            }
        }
    }
    let resp: Value = client
        .post_json("api/v2/databases/redis/conf/update", &update)?
        .json()
        .map_err(|e| anyhow!("更新 Redis 配置失败: {e}"))?;
    check_code(&resp)?;
    println!("Redis 配置已更新({name}):");
    for (k, v) in &opts.set {
        println!("  {k} = {v}");
    }
    Ok(())
}

pub fn passwd(profile: &str, name: &str, password: &str) -> Result<()> {
    crate::apps::password(profile, name, password)
}

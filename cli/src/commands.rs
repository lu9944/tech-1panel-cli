use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::client::PanelClient;
use crate::session::{delete_session, load_session};

#[derive(Debug)]
pub enum ApiMethod {
    Get,
    Post,
    Put,
    Delete,
}
pub fn status(profile: &str) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let resp: Value = client
        .get("api/v2/core/auth/current")?
        .json()
        .map_err(|e| anyhow!("解析面板响应失败: {e}"))?;
    let code = resp["code"].as_i64().unwrap_or(-1);
    if code == 200 {
        let name = resp["data"]["name"].as_str().unwrap_or(&session.username);
        let role = resp["data"]["role"].as_str().unwrap_or("");
        let panel = if session.panel_name.is_empty() {
            session.panel_url.clone()
        } else {
            format!("{} ({})", session.panel_name, session.panel_url)
        };
        println!("已登录: {name} 角色: {role} 面板: {panel}");
        Ok(())
    } else {
        println!(
            "会话已失效 (code={code}, message={})",
            resp["message"].as_str().unwrap_or("")
        );
        println!("请重新运行 login 命令登录。");
        Ok(())
    }
}

pub fn info(profile: &str) -> Result<()> {
    let session = load_session(profile)?;
    println!("profile     : {}", session.profile);
    println!("面板地址    : {}", session.panel_url);
    println!(
        "安全入口    : {}",
        if session.entrance.is_empty() {
            "(未启用)".to_string()
        } else {
            session.entrance
        }
    );
    println!("用户名      : {}", session.username);
    println!("面板名称    : {}", session.panel_name);
    println!("登录时间    : {}", session.logged_in_at);
    let names: Vec<&str> = session.cookies.iter().map(|c| c.name.as_str()).collect();
    println!("凭据 cookies: {} (值已隐藏)", names.join(", "));
    println!("凭据文件    : {}", crate::session::session_path(profile)?.display());
    Ok(())
}

pub fn logout(profile: &str) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let _ = client.post("api/v2/core/auth/logout");
    delete_session(profile)?;
    println!("已退出登录,本地保存的凭据已清除。");
    Ok(())
}

pub fn api_call(profile: &str, method: ApiMethod, path: &str, body: Option<&str>) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let full = normalize_api_path(path);

    let json_body = match body {
        Some(b) if !b.trim().is_empty() => {
            serde_json::from_str::<Value>(b).map_err(|e| anyhow!("--body 不是合法的 JSON: {e}"))?
        }
        _ => Value::Null,
    };

    let resp = match method {
        ApiMethod::Get => client.get(&full)?,
        ApiMethod::Post => client.post_json(&full, &json_body)?,
        ApiMethod::Put => client.put_json(&full, &json_body)?,
        ApiMethod::Delete => client.delete(&full)?,
    };
    let status = resp.status();
    if std::env::var("PANEL_CLI_DEBUG").is_ok() {
        eprintln!("[debug] {method:?} {full} -> {status}");
        for c in client.cookies() {
            eprintln!("[debug] cookie {}={}", c.name, c.value.chars().take(12).collect::<String>());
        }
    }
    let text = resp.text().unwrap_or_default();

    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!("{text}");
    }
    if !status.is_success() {
        eprintln!("HTTP 状态码: {status}");
    }
    Ok(())
}

fn normalize_api_path(path: &str) -> String {
    let p = path.trim_start_matches('/');
    if p.starts_with("api/v2/") {
        p.to_string()
    } else {
        format!("api/v2/{p}")
    }
}

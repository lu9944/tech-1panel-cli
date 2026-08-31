use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::PanelClient;
use crate::session::load_session;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MysqlDB {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    permission: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MysqlUser {
    #[serde(default)]
    username: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    description: String,
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

fn find_mysql_db(client: &PanelClient, instance: &str, name: &str) -> Result<MysqlDB> {
    let body = json!({
        "info": name,
        "database": instance,
        "page": 1,
        "pageSize": 100,
        "orderBy": "name",
        "order": "ascending",
    });
    let resp: Value = client
        .post_json("api/v2/databases/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询数据库失败: {e}"))?;
    check_code(&resp)?;
    let items: Vec<MysqlDB> = serde_json::from_value(resp["data"]["items"].clone())
        .map_err(|e| anyhow!("解析数据库列表失败: {e}"))?;
    items
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| anyhow!("在实例 {instance} 上未找到数据库 {name}"))
}

pub fn list(profile: &str, r#type: &str, instance: Option<&str>) -> Result<()> {
    let client = client_for(profile)?;
    let r#type = if r#type.is_empty() { "mysql" } else { r#type };
    let instance = instance.unwrap_or(r#type);

    match r#type {
        "mysql" | "mariadb" => {
            let body = json!({
                "info": "",
                "database": instance,
                "page": 1,
                "pageSize": 100,
                "orderBy": "name",
                "order": "ascending",
            });
            let resp: Value = client
                .post_json("api/v2/databases/search", &body)?
                .json()
                .map_err(|e| anyhow!("查询数据库失败: {e}"))?;
            check_code(&resp)?;
            let items: Vec<MysqlDB> = serde_json::from_value(resp["data"]["items"].clone())
                .map_err(|e| anyhow!("解析数据库列表失败: {e}"))?;
            let total = resp["data"]["total"].as_i64().unwrap_or(0);
            println!(
                "{:<8} {:<24} {:<16} {:<12} 备注",
                "ID", "名称", "用户", "权限"
            );
            for d in &items {
                let remark = if d.description.is_empty() {
                    "-".to_string()
                } else {
                    d.description.clone()
                };
                println!(
                    "{:<8} {:<24} {:<16} {:<12} {}",
                    d.id, d.name, d.username, d.permission, remark
                );
            }
            println!("共 {total} 个数据库(实例 {instance})");
        }
        other => {
            return Err(anyhow!(
                "不支持的数据库类型: {other}(Redis 请使用 redis 命令)"
            ));
        }
    }
    Ok(())
}

pub struct CreateOptions {
    pub user: Option<String>,
    pub password: Option<String>,
    pub instance: String,
    pub format: String,
    pub permission: String,
    pub remark: Option<String>,
}

pub fn create(profile: &str, name: &str, opts: &CreateOptions) -> Result<()> {
    let client = client_for(profile)?;
    let password = match &opts.password {
        Some(p) => BASE64.encode(p.as_bytes()),
        None => String::new(),
    };
    let body = json!({
        "name": name,
        "from": "local",
        "database": opts.instance,
        "format": opts.format,
        "username": opts.user.clone().unwrap_or_default(),
        "password": password,
        "permission": opts.permission,
        "description": opts.remark.clone().unwrap_or_default(),
    });
    let resp: Value = client
        .post_json("api/v2/databases", &body)?
        .json()
        .map_err(|e| anyhow!("创建数据库失败: {e}"))?;
    check_code(&resp)?;
    println!("数据库已创建: {name}(实例 {})", opts.instance);
    Ok(())
}

pub struct DeleteOptions {
    pub instance: String,
    pub db_type: String,
    pub force: bool,
}

pub fn delete(profile: &str, name: &str, opts: &DeleteOptions) -> Result<()> {
    let client = client_for(profile)?;
    let db = find_mysql_db(&client, &opts.instance, name)?;

    let check_body = json!({"id": db.id, "type": opts.db_type, "database": opts.instance});
    let resp: Value = client
        .post_json("api/v2/databases/del/check", &check_body)?
        .json()
        .map_err(|e| anyhow!("检查数据库占用失败: {e}"))?;
    check_code(&resp)?;
    let resources = resp["data"].as_array().cloned().unwrap_or_default();
    if !resources.is_empty() && !opts.force {
        let names: Vec<String> = resources
            .iter()
            .map(|r| {
                format!(
                    "{}[{}]",
                    r["name"].as_str().unwrap_or(""),
                    r["type"].as_str().unwrap_or("")
                )
            })
            .collect();
        return Err(anyhow!(
            "数据库 {name} 正被以下资源使用: {};如确认删除请加 --force",
            names.join(", ")
        ));
    }

    let body = json!({
        "id": db.id,
        "type": opts.db_type,
        "database": opts.instance,
        "forceDelete": opts.force,
        "deleteBackup": false,
    });
    let resp: Value = client
        .post_json("api/v2/databases/del", &body)?
        .json()
        .map_err(|e| anyhow!("删除数据库失败: {e}"))?;
    check_code(&resp)?;
    println!("数据库已删除: {name}");
    Ok(())
}

pub fn users(profile: &str, instance: &str) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({"database": instance});
    let resp: Value = client
        .post_json("api/v2/databases/users/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询用户失败: {e}"))?;
    check_code(&resp)?;
    let data = &resp["data"];
    let items: Vec<MysqlUser> = if data.get("items").is_some() {
        serde_json::from_value(data["items"].clone())?
    } else {
        serde_json::from_value(data.clone())?
    };
    println!("{:<20} {:<16} 备注", "用户", "主机");
    for u in &items {
        let remark = if u.description.is_empty() {
            "-".to_string()
        } else {
            u.description.clone()
        };
        println!("{:<20} {:<16} {}", u.username, u.host, remark);
    }
    println!("共 {} 个用户", items.len());
    Ok(())
}

pub struct UserOptions {
    pub instance: String,
    pub host: String,
    pub dbs: Vec<String>,
    pub remark: Option<String>,
}

pub fn user_add(profile: &str, username: &str, password: &str, opts: &UserOptions) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({
        "database": opts.instance,
        "username": username,
        "password": BASE64.encode(password.as_bytes()),
        "host": opts.host,
        "description": opts.remark.clone().unwrap_or_default(),
        "dbs": opts.dbs,
    });
    let resp: Value = client
        .post_json("api/v2/databases/users", &body)?
        .json()
        .map_err(|e| anyhow!("创建用户失败: {e}"))?;
    check_code(&resp)?;
    println!(
        "用户已创建: {username}@{}(实例 {})",
        opts.host, opts.instance
    );
    Ok(())
}

pub fn user_del(profile: &str, username: &str, opts: &UserOptions) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({
        "database": opts.instance,
        "username": username,
        "host": opts.host,
    });
    let resp: Value = client
        .post_json("api/v2/databases/users/del", &body)?
        .json()
        .map_err(|e| anyhow!("删除用户失败: {e}"))?;
    check_code(&resp)?;
    println!(
        "用户已删除: {username}@{}(实例 {})",
        opts.host, opts.instance
    );
    Ok(())
}

pub fn user_passwd(
    profile: &str,
    username: &str,
    password: &str,
    opts: &UserOptions,
) -> Result<()> {
    let client = client_for(profile)?;
    let body = json!({
        "database": opts.instance,
        "username": username,
        "host": opts.host,
        "password": BASE64.encode(password.as_bytes()),
    });
    let resp: Value = client
        .post_json("api/v2/databases/users/password", &body)?
        .json()
        .map_err(|e| anyhow!("修改用户密码失败: {e}"))?;
    check_code(&resp)?;
    println!(
        "用户密码已修改: {username}@{}(实例 {})",
        opts.host, opts.instance
    );
    Ok(())
}

pub fn root_passwd(profile: &str, instance: &str, password: &str) -> Result<()> {
    crate::apps::password(profile, instance, password)
}

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::thread;
use std::time::{Duration, Instant};

use crate::client::PanelClient;
use crate::session::load_session;

#[derive(Debug, Deserialize)]
struct AppItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    installed: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    r#type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppTag {
    #[serde(default)]
    name: String,
    #[serde(default)]
    key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledItem {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    app_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppConfigParam {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    edit: bool,
    #[serde(default)]
    label_zh: String,
    #[serde(default)]
    r#type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppConfigResp {
    #[serde(default)]
    params: Vec<AppConfigParam>,
    #[serde(default)]
    container_name: String,
    #[serde(default)]
    allow_port: bool,
    #[serde(default)]
    raw_compose: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledInfo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    container: String,
    #[serde(default)]
    compose_path: String,
    #[serde(default)]
    app_key: String,
    #[serde(default)]
    env: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DatabaseConn {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    container_name: String,
    #[serde(default)]
    port: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    installed: bool,
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppDetail {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    params: Params,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Params {
    #[serde(default)]
    form_fields: Vec<FormField>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FormField {
    #[serde(default)]
    env_key: String,
    #[serde(default)]
    default: Value,
}

pub struct InstallOptions {
    pub version: Option<String>,
    pub params: Vec<(String, String)>,
    pub wait: bool,
    pub timeout: i64,
}

pub struct AppsQuery {
    pub page: i64,
    pub page_size: i64,
    pub name: Option<String>,
    pub r#type: Option<String>,
    pub tag: Option<String>,
    pub installed: bool,
    pub all: bool,
}

pub fn list(profile: &str, q: &AppsQuery) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;

    let tags = match &q.tag {
        Some(t) if !t.is_empty() => vec![t.clone()],
        _ => vec![],
    };

    let mut page = q.page.max(1);
    let page_size = q.page_size.clamp(1, 1000);
    let fetch_all = q.all || q.installed;

    let (items, total) = {
        let mut items: Vec<AppItem> = Vec::new();
        let mut total;
        loop {
            let body = json!({
                "page": page,
                "pageSize": page_size,
                "name": q.name.clone().unwrap_or_default(),
                "tags": tags,
                "type": q.r#type.clone().unwrap_or_default(),
                "recommend": false,
                "resource": "",
                "showCurrentArch": true,
            });
            let resp: Value = client
                .post_json("api/v2/apps/search", &body)?
                .json()
                .map_err(|e| anyhow!("解析响应失败: {e}"))?;
            if resp["code"].as_i64() != Some(200) {
                return Err(anyhow!(
                    "面板返回错误 (code={}): {}",
                    resp["code"].as_i64().unwrap_or(-1),
                    resp["message"].as_str().unwrap_or("")
                ));
            }
            total = resp["data"]["total"].as_i64().unwrap_or(0);
            let batch: Vec<AppItem> = serde_json::from_value(resp["data"]["items"].clone())
                .map_err(|e| anyhow!("解析应用列表失败: {e}"))?;
            let fetched = items.len() as i64 + batch.len() as i64;
            items.extend(batch);
            if !fetch_all || fetched >= total || page * page_size >= total {
                break;
            }
            page += 1;
        }
        (items, total)
    };

    let (items, total) = if q.installed {
        let filtered: Vec<AppItem> = items.into_iter().filter(|i| i.installed).collect();
        let n = filtered.len() as i64;
        (filtered, n)
    } else {
        (items, total)
    };

    print_table(&items, total);
    Ok(())
}

pub fn tags(profile: &str) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let resp: Value = client
        .get("api/v2/apps/tags")?
        .json()
        .map_err(|e| anyhow!("解析响应失败: {e}"))?;
    if resp["code"].as_i64() != Some(200) {
        return Err(anyhow!(
            "面板返回错误 (code={}): {}",
            resp["code"].as_i64().unwrap_or(-1),
            resp["message"].as_str().unwrap_or("")
        ));
    }
    let tags: Vec<AppTag> = serde_json::from_value(resp["data"].clone())
        .map_err(|e| anyhow!("解析应用分类失败: {e}"))?;
    for t in tags {
        println!("{:<16} {}", t.key, t.name);
    }
    Ok(())
}

fn print_table(items: &[AppItem], total: i64) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for it in items {
        rows.push(vec![
            it.r#type.clone(),
            it.name.clone(),
            it.key.clone(),
            if it.installed { "是" } else { "否" }.to_string(),
            it.tags.join(", "),
        ]);
    }
    let w = |idx: usize| rows.iter().map(|r| r[idx].chars().count()).max().unwrap_or(0);
    let (wt, wn, wk, wi, wtags) = (w(0), w(1), w(2), w(3), w(4));
    let sep = |widths: &[usize]| {
        println!(
            "{}",
            widths
                .iter()
                .map(|x| "-".repeat(x + 2))
                .collect::<Vec<_>>()
                .join("+")
        );
    };
    println!(
        "{:<wt$}  {:<wn$}  {:<wk$}  {:<wi$}  {:<wtags$}",
        "类型", "名称", "key", "已安装", "标签"
    );
    sep(&[wt, wn, wk, wi, wtags]);
    for r in &rows {
        println!(
            "{:<wt$}  {:<wn$}  {:<wk$}  {:<wi$}  {:<wtags$}",
            r[0], r[1], r[2], r[3], r[4]
        );
    }
    sep(&[wt, wn, wk, wi, wtags]);
    println!("共 {} 个应用(展示 {} 条)", total, rows.len());
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

fn find_installed(client: &PanelClient, name: &str) -> Result<InstalledItem> {
    let body = json!({"page": 1, "pageSize": 100, "name": name, "type": "", "tags": []});
    let resp: Value = client
        .post_json("api/v2/apps/installed/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询已安装应用失败: {e}"))?;
    check_code(&resp)?;
    let items: Vec<InstalledItem> = serde_json::from_value(resp["data"]["items"].clone())
        .map_err(|e| anyhow!("解析已安装应用失败: {e}"))?;
    items
        .into_iter()
        .find(|it| it.name == name)
        .ok_or_else(|| anyhow!("未安装应用 {name},可先运行 apps install {name} 进行安装"))
}

fn get_install_info(client: &PanelClient, id: u64) -> Result<InstalledInfo> {
    let resp: Value = client
        .get(&format!("api/v2/apps/installed/info/{id}"))?
        .json()
        .map_err(|e| anyhow!("查询应用详情失败: {e}"))?;
    check_code(&resp)?;
    serde_json::from_value(resp["data"].clone()).map_err(|e| anyhow!("解析应用详情失败: {e}"))
}

fn get_conn_info(client: &PanelClient, app_key: &str, name: &str) -> Result<DatabaseConn> {
    let body = json!({"type": app_key, "name": name});
    let resp: Value = client
        .post_json("api/v2/apps/installed/conninfo", &body)?
        .json()
        .map_err(|e| anyhow!("查询连接信息失败: {e}"))?;
    check_code(&resp)?;
    serde_json::from_value(resp["data"].clone()).map_err(|e| anyhow!("解析连接信息失败: {e}"))
}

pub fn info(profile: &str, name: &str) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;

    let installed = find_installed(&client, name)?;
    let detail = get_install_info(&client, installed.id)?;

    println!("应用:   {}", detail.name);
    println!("key:    {}", detail.app_key);
    println!("版本:   {}", detail.version);
    println!("状态:   {}", detail.status);
    if !detail.container.is_empty() {
        println!("容器:   {}", detail.container);
    }
    if !detail.compose_path.is_empty() {
        println!("compose: {}", detail.compose_path);
    }

    let is_db = matches!(
        detail.app_key.as_str(),
        "mysql" | "mariadb" | "redis" | "mongodb" | "postgresql" | "sqlite" | "clickhouse"
    );
    if is_db {
        let conn = get_conn_info(&client, &detail.app_key, name)?;
        if !conn.container_name.is_empty() || conn.port > 0 {
            let host = session
                .panel_url
                .split("://")
                .nth(1)
                .and_then(|s| s.split(':').next())
                .unwrap_or("127.0.0.1");
            let username = if conn.username.is_empty()
                && matches!(detail.app_key.as_str(), "mysql" | "mariadb")
            {
                "root".to_string()
            } else {
                conn.username
            };
            println!();
            println!("连接信息:");
            println!("  主机:  {host}");
            println!("  端口:  {}", conn.port);
            println!("  用户:  {username}");
            println!("  密码:  {}", conn.password);
            println!("  容器:  {}", conn.container_name);
        }
    }

    if !detail.env.is_empty() {
        println!();
        println!("环境变量:");
        let mut keys: Vec<&String> = detail.env.keys().collect();
        keys.sort();
        for k in keys {
            println!("  {k} = {}", fmt_value(&detail.env[k]));
        }
    }
    Ok(())
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn get_app(client: &PanelClient, key: &str) -> Result<AppInfo> {
    let resp: Value = client
        .get(&format!("api/v2/apps/{key}"))?
        .json()
        .map_err(|e| anyhow!("解析应用信息失败: {e}"))?;
    check_code(&resp)?;
    let app: AppInfo = serde_json::from_value(resp["data"].clone())
        .map_err(|e| anyhow!("解析应用信息失败: {e}"))?;
    if app.id == 0 {
        return Err(anyhow!("未找到应用: {key}"));
    }
    Ok(app)
}

fn get_app_detail(client: &PanelClient, app_id: u64, version: &str) -> Result<AppDetail> {
    let resp: Value = client
        .get(&format!("api/v2/apps/detail/{app_id}/{version}/app"))?
        .json()
        .map_err(|e| anyhow!("解析应用版本信息失败: {e}"))?;
    check_code(&resp)?;
    let detail: AppDetail = serde_json::from_value(resp["data"].clone())
        .map_err(|e| anyhow!("解析应用版本信息失败: {e}"))?;
    if detail.id == 0 {
        return Err(anyhow!("未找到应用版本信息"));
    }
    Ok(detail)
}

fn parse_value(raw: &str) -> Value {
    if let Ok(i) = raw.parse::<i64>() {
        return json!(i);
    }
    if let Ok(f) = raw.parse::<f64>() {
        return json!(f);
    }
    json!(raw)
}

fn new_uuid() -> String {
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = |s: &[u8]| hex::encode(s);
    format!(
        "{}-{}-{}-{}-{}",
        h(&b[0..4]),
        h(&b[4..6]),
        h(&b[6..8]),
        h(&b[8..10]),
        h(&b[10..16])
    )
}

pub fn install(profile: &str, name: &str, opts: &InstallOptions) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;

    let app = get_app(&client, name)?;
    if app.installed {
        return Err(anyhow!("应用 {name} 已安装"));
    }
    if app.versions.is_empty() {
        return Err(anyhow!("应用 {name} 没有可用版本"));
    }
    let version = opts
        .version
        .clone()
        .unwrap_or_else(|| app.versions[0].clone());

    let detail = get_app_detail(&client, app.id, &version)?;

    let mut params = Map::new();
    for f in &detail.params.form_fields {
        if !f.env_key.is_empty() {
            params.insert(f.env_key.clone(), f.default.clone());
        }
    }
    for (k, v) in &opts.params {
        params.insert(k.clone(), parse_value(v));
    }

    let task_id = new_uuid();
    let body = json!({
        "appDetailId": detail.id,
        "params": params,
        "name": name,
        "taskID": task_id,
        "advanced": true,
        "allowPort": true,
        "pullImage": true,
        "cpuQuota": 0,
        "memoryLimit": 0,
        "memoryUnit": "M",
        "containerName": "",
        "editCompose": false,
        "dockerCompose": "",
        "gpuConfig": false,
        "specifyIP": "",
    });

    println!("开始安装 {name} (版本 {version}) ...");
    let resp: Value = client
        .post_json("api/v2/apps/install", &body)?
        .json()
        .map_err(|e| anyhow!("解析安装响应失败: {e}"))?;
    check_code(&resp)?;

    println!("安装任务已提交, taskID={task_id}");
    if opts.wait {
        wait_task(&client, &task_id, opts.timeout)
    } else {
        println!("可通过面板\"已安装应用\"页面或运行 apps status 查看安装进度");
        Ok(())
    }
}

fn wait_task(client: &PanelClient, task_id: &str, timeout: i64) -> Result<()> {
    let timeout = if timeout <= 0 { 600 } else { timeout };
    let deadline = Instant::now() + Duration::from_secs(timeout as u64);
    loop {
        let body = json!({
            "page": 1,
            "pageSize": 10,
            "type": "",
            "status": "",
            "taskID": task_id,
        });
        let resp: Value = client
            .post_json("api/v2/logs/tasks/search", &body)?
            .json()
            .map_err(|e| anyhow!("查询任务状态失败: {e}"))?;
        if let Some(items) = resp["data"]["items"].as_array() {
            if let Some(task) = items.first() {
                let status = task["status"].as_str().unwrap_or("");
                let step = task["currentStep"].as_str().unwrap_or("");
                match status {
                    "Success" => {
                        println!("\n安装成功!");
                        return Ok(());
                    }
                    "Failed" | "Error" => {
                        let err = task["errorMsg"].as_str().unwrap_or("未知错误");
                        println!("\n安装失败: {err}");
                        return Err(anyhow!("安装失败: {err}"));
                    }
                    _ => {
                        print!("\r安装进行中... 状态: {status} {step:<40}");
                        use std::io::Write;
                        std::io::stdout().flush()?;
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            println!("\n等待安装超时(超过 {timeout} 秒),请到面板查看安装进度");
            return Err(anyhow!("等待安装超时"));
        }
        thread::sleep(Duration::from_secs(2));
    }
}

pub struct ConfigOptions {
    pub set: Vec<(String, String)>,
    pub show_compose: bool,
}

pub fn config(profile: &str, name: &str, opts: &ConfigOptions) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let installed = find_installed(&client, name)?;

    if opts.set.is_empty() {
        let resp: Value = client
            .get(&format!("api/v2/apps/installed/params/{}", installed.id))?
            .json()
            .map_err(|e| anyhow!("读取应用配置失败: {e}"))?;
        check_code(&resp)?;
        let cfg: AppConfigResp = serde_json::from_value(resp["data"].clone())
            .map_err(|e| anyhow!("解析应用配置失败: {e}"))?;
        println!("{name} 配置(共 {} 项):", cfg.params.len());
        for p in &cfg.params {
            let edit = if p.edit { "是" } else { "否" };
            let label = if p.label_zh.is_empty() {
                String::new()
            } else {
                format!(" # {}", p.label_zh)
            };
            println!(
                "  {} = {}{}  [类型:{} 可编辑:{}]",
                p.key,
                fmt_value(&p.value),
                label,
                p.r#type,
                edit
            );
        }
        if !cfg.container_name.is_empty() {
            println!("容器: {}", cfg.container_name);
        }
        println!(
            "端口自动分配: {}",
            if cfg.allow_port { "开启" } else { "关闭" }
        );
        if opts.show_compose && !cfg.raw_compose.is_empty() {
            println!("\n--- docker-compose ---");
            println!("{}", cfg.raw_compose);
        }
        return Ok(());
    }

    let mut params = Map::new();
    for (k, v) in &opts.set {
        params.insert(k.clone(), parse_value(v));
    }
    let body = json!({
        "installId": installed.id,
        "params": params,
        "advanced": false,
    });
    let resp: Value = client
        .post_json("api/v2/apps/installed/params/update", &body)?
        .json()
        .map_err(|e| anyhow!("解析修改配置响应失败: {e}"))?;
    check_code(&resp)?;
    println!("{name} 配置已提交,容器正在重建以应用新配置...");
    println!("稍后可运行 apps info {name} 查看生效情况");
    Ok(())
}

pub fn password(profile: &str, name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(anyhow!("密码不能为空"));
    }
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let installed = find_installed(&client, name)?;

    let encoded = BASE64.encode(value.as_bytes());
    match installed.app_key.as_str() {
        "mysql" | "mariadb" => {
            let body = json!({
                "id": 0,
                "from": "local",
                "type": installed.app_key,
                "database": name,
                "value": encoded,
            });
            let resp: Value = client
                .post_json("api/v2/databases/change/password", &body)?
                .json()
                .map_err(|e| anyhow!("解析修改密码响应失败: {e}"))?;
            check_code(&resp)?;
            println!("{name} ({}) root 密码已修改", installed.app_key);
        }
        "redis" => {
            let body = json!({"database": name, "value": encoded});
            let resp: Value = client
                .post_json("api/v2/databases/redis/password", &body)?
                .json()
                .map_err(|e| anyhow!("解析修改密码响应失败: {e}"))?;
            check_code(&resp)?;
            println!("{name} (redis) 密码已修改");
        }
        other => {
            return Err(anyhow!(
                "暂不支持修改 {other} 的密码,仅支持 mysql / mariadb / redis"
            ));
        }
    }
    println!("新密码: {value}");
    println!("可通过 apps info {name} 查看最新的连接信息");
    Ok(())
}

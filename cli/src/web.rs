use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

use crate::client::PanelClient;
use crate::session::load_session;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebsiteItem {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    primary_domain: String,
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    site_path: String,
    #[serde(default)]
    protocol: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpsStatus {
    #[serde(default)]
    enable: bool,
    #[serde(default)]
    http_config: String,
    #[serde(default)]
    ssl: Option<SslInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SslInfo {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    primary_domain: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    expire_date: String,
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

fn find_website(client: &PanelClient, domain: &str) -> Result<WebsiteItem> {
    for page in 1..=20 {
        let body = json!({
            "page": page,
            "pageSize": 100,
            "name": "",
            "orderBy": "primary_domain",
            "order": "ascending",
            "type": "",
        });
        let resp: Value = client
            .post_json("api/v2/websites/search", &body)?
            .json()
            .map_err(|e| anyhow!("查询网站列表失败: {e}"))?;
        check_code(&resp)?;
        let items: Vec<WebsiteItem> = serde_json::from_value(resp["data"]["items"].clone())
            .map_err(|e| anyhow!("解析网站列表失败: {e}"))?;
        for it in items {
            if it.primary_domain == domain {
                return Ok(it);
            }
        }
        if (resp["data"]["total"].as_i64().unwrap_or(0) as usize) <= page * 100 {
            break;
        }
    }
    Err(anyhow!("未找到域名 {domain} 对应的网站"))
}

fn get_website(client: &PanelClient, domain: &str) -> Result<WebsiteItem> {
    find_website(client, domain)
}

pub fn list(profile: &str) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut total = 0i64;
    for page in 1..=20 {
        let body = json!({
            "page": page,
            "pageSize": 100,
            "name": "",
            "orderBy": "primary_domain",
            "order": "ascending",
            "type": "",
        });
        let resp: Value = client
            .post_json("api/v2/websites/search", &body)?
            .json()
            .map_err(|e| anyhow!("查询网站列表失败: {e}"))?;
        check_code(&resp)?;
        total = resp["data"]["total"].as_i64().unwrap_or(0);
        let items: Vec<WebsiteItem> = serde_json::from_value(resp["data"]["items"].clone())
            .map_err(|e| anyhow!("解析网站列表失败: {e}"))?;
        for it in &items {
            rows.push(vec![
                it.primary_domain.clone(),
                it.r#type.clone(),
                it.status.clone(),
                if it.protocol == "HTTPS" { "是" } else { "否" }.to_string(),
                it.site_path.clone(),
            ]);
        }
        if (total as usize) <= page * 100 {
            break;
        }
    }
    let w = |i: usize| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0);
    let (wd, wt, ws, wh, wp) = (w(0), w(1), w(2), w(3), w(4));
    println!("{:<wd$}  {:<wt$}  {:<ws$}  {:<wh$}  {:<wp$}", "域名", "类型", "状态", "HTTPS", "网站目录");
    println!(
        "{}",
        [wd, wt, ws, wh, wp]
            .iter()
            .map(|x| "-".repeat(x + 2))
            .collect::<Vec<_>>()
            .join("+")
    );
    for r in &rows {
        println!("{:<wd$}  {:<wt$}  {:<ws$}  {:<wh$}  {:<wp$}", r[0], r[1], r[2], r[3], r[4]);
    }
    println!("共 {total} 个网站");
    Ok(())
}

pub struct ConfigOptions {
    pub file: Option<String>,
}

pub fn config(profile: &str, domain: &str, opts: &ConfigOptions) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let website = get_website(&client, domain)?;

    if let Some(file) = &opts.file {
        let content =
            fs::read_to_string(file).map_err(|e| anyhow!("读取文件失败 {file}: {e}"))?;
        let body = json!({"id": website.id, "content": content});
        let resp: Value = client
            .post_json("api/v2/websites/nginx/update", &body)?
            .json()
            .map_err(|e| anyhow!("解析修改配置响应失败: {e}"))?;
        check_code(&resp)?;
        println!("{domain} 的 nginx 配置已更新并生效");
        return Ok(());
    }

    let resp: Value = client
        .get(&format!("api/v2/websites/{}/config/openresty", website.id))?
        .json()
        .map_err(|e| anyhow!("读取 nginx 配置失败: {e}"))?;
    check_code(&resp)?;
    let content = resp["data"]["content"].as_str().unwrap_or("");
    println!("--- {domain} nginx 配置 ---");
    println!("{content}");
    Ok(())
}

pub struct HttpsOptions {
    pub enable: Option<bool>,
    pub ssl_id: Option<u64>,
    pub http_config: Option<String>,
    pub cert_file: Option<String>,
    pub key_file: Option<String>,
}

pub fn https(profile: &str, domain: &str, opts: &HttpsOptions) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let website = get_website(&client, domain)?;

    if opts.enable.is_none() && opts.ssl_id.is_none() && opts.cert_file.is_none() {
        let resp: Value = client
            .get(&format!("api/v2/websites/{}/https", website.id))?
            .json()
            .map_err(|e| anyhow!("查询 https 配置失败: {e}"))?;
        check_code(&resp)?;
        let st: HttpsStatus = serde_json::from_value(resp["data"].clone())
            .map_err(|e| anyhow!("解析 https 配置失败: {e}"))?;
        println!("{domain} HTTPS: {}", if st.enable { "已开启" } else { "未开启" });
        if let Some(ssl) = &st.ssl {
            println!("证书 id: {} ({}，状态: {})", ssl.id, ssl.primary_domain, ssl.status);
            println!("到期: {}", ssl.expire_date);
        }
        println!("HTTP 跳转: {}", st.http_config);
        return Ok(());
    }

    let enable = opts.enable.unwrap_or(true);
    let http_config = opts
        .http_config
        .clone()
        .unwrap_or_else(|| "HTTPToHTTPS".to_string());
    let mut body = json!({
        "websiteId": website.id,
        "enable": enable,
        "httpConfig": http_config,
        "type": "existed",
    });
    if enable {
        if let (Some(cert), Some(key)) = (&opts.cert_file, &opts.key_file) {
            let certificate =
                fs::read_to_string(cert).map_err(|e| anyhow!("读取证书文件失败 {cert}: {e}"))?;
            let private_key =
                fs::read_to_string(key).map_err(|e| anyhow!("读取私钥文件失败 {key}: {e}"))?;
            body["type"] = json!("manual");
            body["importType"] = json!("paste");
            body["certificate"] = json!(certificate);
            body["privateKey"] = json!(private_key);
        } else {
            let ssl_id = opts
                .ssl_id
                .ok_or_else(|| anyhow!("启用 HTTPS 需要提供 --ssl-id(可先运行 web ssl 申请证书)或 --cert/--key 上传证书"))?;
            body["type"] = json!("existed");
            body["websiteSSLId"] = json!(ssl_id);
        }
    }
    let resp: Value = client
        .post_json(&format!("api/v2/websites/{}/https", website.id), &body)?
        .json()
        .map_err(|e| anyhow!("更新 https 配置失败: {e}"))?;
    check_code(&resp)?;
    println!(
        "{domain} HTTPS 已{}",
        if enable { "开启" } else { "关闭" }
    );
    Ok(())
}

pub struct SslOptions {
    pub email: String,
    pub provider: String,
    pub key_type: String,
    pub auto_renew: bool,
}

pub fn ssl(profile: &str, domain: &str, opts: &SslOptions) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;

    let acme_account_id = find_or_create_acme(&client, &opts.email)?;

    let body = json!({
        "primaryDomain": domain,
        "provider": opts.provider,
        "acmeAccountId": acme_account_id,
        "keyType": opts.key_type,
        "autoRenew": opts.auto_renew,
    });
    let resp: Value = client
        .post_json("api/v2/websites/ssl", &body)?
        .json()
        .map_err(|e| anyhow!("创建证书请求失败: {e}"))?;
    check_code(&resp)?;
    let ssl_id = resp["data"]["id"].as_u64().unwrap_or(0);
    if ssl_id == 0 {
        return Err(anyhow!("创建证书失败: 未返回证书 ID"));
    }
    println!("证书已创建: id={ssl_id} domain={domain}");

    let body = json!({"ID": ssl_id, "skipDNSCheck": false});
    let resp: Value = client
        .post_json("api/v2/websites/ssl/obtain", &body)?
        .json()
        .map_err(|e| anyhow!("申请证书失败: {e}"))?;
    check_code(&resp)?;
    println!("证书申请任务已提交(provider: {}, 可能需要几分钟)", opts.provider);
    println!("申请完成后运行以下命令启用 HTTPS:");
    println!("  1panel-cli web https {domain} --enable --ssl-id {ssl_id}");
    Ok(())
}

fn find_or_create_acme(client: &PanelClient, email: &str) -> Result<u64> {
    let body = json!({"page": 1, "pageSize": 50});
    let resp: Value = client
        .post_json("api/v2/websites/acme/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询 ACME 账号失败: {e}"))?;
    check_code(&resp)?;
    if let Some(items) = resp["data"]["items"].as_array() {
        for it in items {
            if it["email"].as_str() == Some(email) {
                if let Some(id) = it["id"].as_u64() {
                    println!("使用已有 ACME 账号: {email} (id={id})");
                    return Ok(id);
                }
            }
        }
    }
    let body = json!({
        "email": email,
        "type": "letsencrypt",
        "keyType": "RSA2048",
    });
    let resp: Value = client
        .post_json("api/v2/websites/acme", &body)?
        .json()
        .map_err(|e| anyhow!("创建 ACME 账号失败: {e}"))?;
    check_code(&resp)?;
    let id = resp["data"]["id"]
        .as_u64()
        .ok_or_else(|| anyhow!("创建 ACME 账号失败: 未返回账号 ID"))?;
    println!("已创建 ACME 账号: {email} (id={id})");
    Ok(id)
}

pub fn upload(profile: &str, domain: &str, file: &str, to: &str) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let website = get_website(&client, domain)?;

    let local = Path::new(file);
    if !local.exists() {
        return Err(anyhow!("本地文件不存在: {file}"));
    }
    let dest = join_site_path(&website.site_path, to);
    let resp = client.upload_file(&dest, local, true)?;
    let text = resp.text().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if v["code"].as_i64() != Some(200) {
        return Err(anyhow!(
            "上传失败 (code={}): {}",
            v["code"].as_i64().unwrap_or(-1),
            v["message"].as_str().unwrap_or(&text)
        ));
    }
    println!("已上传 {} 到 {dest}", local.file_name().unwrap_or_default().to_string_lossy());
    Ok(())
}

pub fn extract(
    profile: &str,
    domain: &str,
    archive: &str,
    to: &str,
    archive_type: &str,
) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let website = get_website(&client, domain)?;

    let archive_path = if archive.starts_with('/') {
        archive.to_string()
    } else {
        format!("{}/{}", website.site_path.trim_end_matches('/'), archive)
    };
    let dst = join_site_path(&website.site_path, to);
    let ctype = if archive_type.is_empty() {
        detect_archive_type(archive)?
    } else {
        archive_type.to_string()
    };

    let body = json!({
        "path": archive_path,
        "dst": dst,
        "type": ctype,
        "secret": "",
        "taskID": format!("{:x}-{:x}-4{:03}-8{:03}-{:012x}",
            rand::random::<u32>(), rand::random::<u16>(), rand::random::<u16>() & 0xfff,
            rand::random::<u16>() & 0xfff, rand::random::<u64>()),
    });
    let resp: Value = client
        .post_json("api/v2/files/decompress", &body)?
        .json()
        .map_err(|e| anyhow!("解析解压响应失败: {e}"))?;
    check_code(&resp)?;
    println!("已提交解压任务: {archive_path} -> {dst} (类型 {ctype})");
    Ok(())
}

fn join_site_path(site_path: &str, sub: &str) -> String {
    let base = site_path.trim_end_matches('/');
    let sub = sub.trim_matches('/');
    if sub.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{sub}")
    }
}

fn detect_archive_type(name: &str) -> Result<String> {
    let lower = name.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Ok("tar.gz".to_string())
    } else if lower.ends_with(".tar.bz2") {
        Ok("tar.bz2".to_string())
    } else if lower.ends_with(".tar.xz") {
        Ok("tar.xz".to_string())
    } else if lower.ends_with(".tar") {
        Ok("tar".to_string())
    } else if lower.ends_with(".zip") {
        Ok("zip".to_string())
    } else if lower.ends_with(".gz") {
        Ok("gz".to_string())
    } else if lower.ends_with(".bz2") {
        Ok("bz2".to_string())
    } else if lower.ends_with(".xz") {
        Ok("xz".to_string())
    } else if lower.ends_with(".7z") {
        Ok("7z".to_string())
    } else if lower.ends_with(".rar") {
        Ok("rar".to_string())
    } else {
        Err(anyhow!("无法从文件名识别压缩类型,请用 --type 指定(zip / tar.gz / tar / gz ...)"))
    }
}

pub struct CreateOptions {
    pub r#type: String,
    pub alias: Option<String>,
    pub remark: Option<String>,
}

fn new_uuid() -> String {
    let mut b = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut b);
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

fn find_openresty_install(client: &PanelClient) -> Result<u64> {
    let body = json!({"page": 1, "pageSize": 100, "name": "openresty", "type": "", "tags": []});
    let resp: Value = client
        .post_json("api/v2/apps/installed/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询 openresty 安装信息失败: {e}"))?;
    check_code(&resp)?;
    let items = resp["data"]["items"].as_array().cloned().unwrap_or_default();
    for it in &items {
        if it["appKey"].as_str() == Some("openresty") {
            return it["id"]
                .as_u64()
                .ok_or_else(|| anyhow!("解析 openresty 安装 ID 失败"));
        }
    }
    Err(anyhow!("未找到已安装的 openresty,请先运行 apps install openresty"))
}

fn find_default_website_group(client: &PanelClient) -> Result<u64> {
    let body = json!({"type": "website"});
    let resp: Value = client
        .post_json("api/v2/core/groups/search", &body)?
        .json()
        .map_err(|e| anyhow!("查询网站分组失败: {e}"))?;
    check_code(&resp)?;
    let groups = resp["data"].as_array().cloned().unwrap_or_default();
    for g in &groups {
        if g["isDefault"].as_bool() == Some(true) {
            return g["id"]
                .as_u64()
                .ok_or_else(|| anyhow!("解析默认分组 ID 失败"));
        }
    }
    if let Some(g) = groups.first() {
        return g["id"]
            .as_u64()
            .ok_or_else(|| anyhow!("解析分组 ID 失败"));
    }
    Err(anyhow!("面板没有可用的网站分组"))
}

pub fn create(profile: &str, domain: &str, opts: &CreateOptions) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;

    let openresty_id = find_openresty_install(&client)?;
    let group_id = find_default_website_group(&client)?;
    let alias = opts
        .alias
        .clone()
        .unwrap_or_else(|| {
            domain
                .split('.')
                .next()
                .unwrap_or("site")
                .to_string()
        });

    let body = json!({
        "primaryDomain": domain,
        "type": opts.r#type,
        "alias": alias,
        "remark": opts.remark.clone().unwrap_or_default(),
        "appType": "installed",
        "appInstallId": openresty_id,
        "webSiteGroupId": group_id,
        "domains": [{"domain": domain, "port": 80, "ssl": false}],
        "taskID": new_uuid(),
        "IPV6": false,
    });
    let resp: Value = client
        .post_json("api/v2/websites", &body)?
        .json()
        .map_err(|e| anyhow!("解析创建网站响应失败: {e}"))?;
    check_code(&resp)?;
    println!("网站已创建: {domain} (alias={alias}, 类型={})", opts.r#type);
    println!("网站目录: /opt/1panel/www/sites/{alias}");
    Ok(())
}

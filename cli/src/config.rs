use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

pub const ENV_URL: &str = "PANEL_URL";
pub const ENV_ENTRANCE: &str = "PANEL_ENTRANCE";
pub const ENV_USERNAME: &str = "PANEL_USERNAME";
pub const ENV_PASSWORD: &str = "PANEL_PASSWORD";
pub const ENV_MFA: &str = "PANEL_MFA_CODE";
pub const ENV_CAPTCHA: &str = "PANEL_CAPTCHA";
pub const ENV_CAPTCHA_ID: &str = "PANEL_CAPTCHA_ID";
pub const ENV_LANGUAGE: &str = "PANEL_LANGUAGE";
pub const ENV_INSECURE: &str = "PANEL_INSECURE";
pub const ENV_NODE: &str = "PANEL_NODE";
pub const ENV_LINUX_SSH_USER: &str = "LINUX_SSH_USER";
pub const ENV_LINUX_SSH_PWD: &str = "LINUX_SSH_PWD";

pub fn set_node_override(node: Option<&str>) {
    if let Some(node) = node.filter(|v| !v.trim().is_empty()) {
        env::set_var(ENV_NODE, node);
    }
}

/// 读取面板所在主机的 SSH 凭据(仅 exec --sync-ssh 自动配置本地连接时使用)
pub fn linux_ssh_creds() -> (Option<String>, Option<String>) {
    (env_get(ENV_LINUX_SSH_USER), env_get(ENV_LINUX_SSH_PWD))
}

#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    pub url: Option<String>,
    pub entrance: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub mfa_code: Option<String>,
    pub captcha: Option<String>,
    pub captcha_id: Option<String>,
    pub language: Option<String>,
    pub insecure: Option<bool>,
    pub linux_ssh_user: Option<String>,
    pub linux_ssh_pwd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PanelConfig {
    pub url: String,
    pub entrance: String,
    pub username: String,
    pub password: String,
    pub mfa_code: Option<String>,
    pub captcha: Option<String>,
    pub captcha_id: Option<String>,
    pub language: String,
    pub insecure: bool,
    // exec --sync-ssh 经由 linux_ssh_creds() 读取同名环境变量;字段保留以完整映射 .env 配置
    #[allow(dead_code)]
    pub linux_ssh_user: Option<String>,
    #[allow(dead_code)]
    pub linux_ssh_pwd: Option<String>,
}

fn env_get(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn env_flag(key: &str) -> Option<bool> {
    env_get(key).map(|v| {
        matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Load .env file into the process environment (does not override existing vars).
pub fn load_env_file(env_path: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(p) = env_path {
        if !p.exists() {
            return Err(anyhow!("指定的 .env 文件不存在: {}", p.display()));
        }
        dotenvy::from_path(p).with_context(|| format!("解析 .env 文件失败: {}", p.display()))?;
        return Ok(Some(p.to_path_buf()));
    }
    let mut candidates = vec![PathBuf::from(".env")];
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(".env"));
        }
    }
    for c in candidates {
        if c.exists() {
            dotenvy::from_path(&c)
                .with_context(|| format!("解析 .env 文件失败: {}", c.display()))?;
            return Ok(Some(c));
        }
    }
    Ok(None)
}

/// Split `http://host:port/entrance` into (base_url, entrance).
fn split_url(raw: &str) -> Result<(String, String)> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(anyhow!("面板地址不能为空"));
    }
    let mut url = raw.to_string();
    let mut entrance = String::new();
    let scheme_end = raw.find("://");
    let after_scheme = match scheme_end {
        Some(pos) => &raw[pos + 3..],
        None => raw,
    };
    if let Some(slash) = after_scheme.find('/') {
        let path = after_scheme[slash..].trim_matches('/');
        if !path.is_empty() {
            entrance = path.to_string();
        }
        let base = match scheme_end {
            Some(pos) => format!("{}{}", &raw[..pos + 3], &after_scheme[..slash]),
            None => after_scheme[..slash].to_string(),
        };
        url = base;
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok((url, entrance))
    } else {
        Ok((format!("http://{url}"), entrance))
    }
}

impl PanelConfig {
    pub fn from_env(overrides: &ConfigOverrides) -> Result<Self> {
        let url = overrides
            .url
            .clone()
            .or_else(|| env_get(ENV_URL))
            .ok_or_else(|| {
                anyhow!("未配置面板地址: 请通过 --url 参数、环境变量 PANEL_URL 或 .env 文件提供")
            })?;
        let (base_url, url_entrance) = split_url(&url)?;

        let entrance = overrides
            .entrance
            .clone()
            .or(env_get(ENV_ENTRANCE))
            .or((!url_entrance.is_empty()).then_some(url_entrance))
            .unwrap_or_default();

        let username = overrides
            .username
            .clone()
            .or_else(|| env_get(ENV_USERNAME))
            .ok_or_else(|| anyhow!("未配置面板用户名: 请通过 --user 参数、环境变量 PANEL_USERNAME 或 .env 文件提供"))?;
        let password = overrides
            .password
            .clone()
            .or_else(|| env_get(ENV_PASSWORD))
            .ok_or_else(|| anyhow!("未配置面板密码: 请通过 --password 参数、环境变量 PANEL_PASSWORD 或 .env 文件提供"))?;

        let insecure = overrides
            .insecure
            .or_else(|| env_flag(ENV_INSECURE))
            .unwrap_or(false);

        Ok(PanelConfig {
            url: base_url,
            entrance,
            username,
            password,
            mfa_code: overrides.mfa_code.clone().or_else(|| env_get(ENV_MFA)),
            captcha: overrides.captcha.clone().or_else(|| env_get(ENV_CAPTCHA)),
            captcha_id: overrides
                .captcha_id
                .clone()
                .or_else(|| env_get(ENV_CAPTCHA_ID)),
            language: overrides
                .language
                .clone()
                .or_else(|| env_get(ENV_LANGUAGE))
                .unwrap_or_else(|| "zh".to_string()),
            insecure,
            linux_ssh_user: overrides
                .linux_ssh_user
                .clone()
                .or_else(|| env_get(ENV_LINUX_SSH_USER)),
            linux_ssh_pwd: overrides
                .linux_ssh_pwd
                .clone()
                .or_else(|| env_get(ENV_LINUX_SSH_PWD)),
        })
    }
}

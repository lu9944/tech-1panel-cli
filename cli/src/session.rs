use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieData {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub profile: String,
    pub panel_url: String,
    pub entrance: String,
    pub username: String,
    pub panel_name: String,
    pub logged_in_at: String,
    #[serde(default)]
    pub insecure: bool,
    pub cookies: Vec<CookieData>,
}

fn config_dir() -> Result<PathBuf> {
    let dir = match dirs::config_dir() {
        Some(d) => d.join("1panel-cli"),
        None => {
            let home = std::env::var("HOME").map_err(|_| anyhow!("无法确定主目录"))?;
            PathBuf::from(home).join(".config").join("1panel-cli")
        }
    };
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn session_path(profile: &str) -> Result<PathBuf> {
    if profile.is_empty() || profile.contains(['/', '\\']) {
        return Err(anyhow!("非法的 profile 名称: {profile:?}"));
    }
    Ok(config_dir()?.join(format!("{profile}.json")))
}

pub fn save_session(session: &Session) -> Result<PathBuf> {
    let path = session_path(&session.profile)?;
    let json = serde_json::to_string_pretty(session)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn load_session(profile: &str) -> Result<Session> {
    let path = session_path(profile)?;
    let data = fs::read_to_string(&path)
        .with_context(|| format!("未找到已保存的会话: {}", path.display()))?;
    let session: Session = serde_json::from_str(&data)
        .with_context(|| format!("会话文件损坏: {}", path.display()))?;
    Ok(session)
}

pub fn delete_session(profile: &str) -> Result<bool> {
    let path = session_path(profile)?;
    if path.exists() {
        fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

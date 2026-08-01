use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::blocking::{Client, Response};
use reqwest::cookie::CookieStore;
use reqwest::cookie::Jar;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use std::sync::Arc;
use std::time::Duration;

use crate::crypto::url_decode;
use crate::session::CookieData;

pub struct PanelClient {
    pub base_url: String,
    pub http: Client,
    pub jar: Arc<Jar>,
}

impl PanelClient {
    pub fn new(base_url: &str, cookies: Option<&[CookieData]>, insecure: bool) -> Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let url = base_url
            .parse()
            .map_err(|e| anyhow!("非法的面板地址 {base_url}: {e}"))?;
        let jar = Arc::new(Jar::default());
        if let Some(cookies) = cookies {
            for c in cookies {
                jar.add_cookie_str(&format!("{}={}", c.name, c.value), &url);
            }
        }
        let mut builder = Client::builder()
            .cookie_store(true)
            .cookie_provider(jar.clone())
            .timeout(Duration::from_secs(30))
            .user_agent("1panel-cli");
        if insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http = builder.build()?;
        Ok(Self { base_url, http, jar })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    pub fn cookies(&self) -> Vec<CookieData> {
        let Ok(url) = self.base_url.parse() else {
            return vec![];
        };
        let Some(header) = self.jar.cookies(&url) else {
            return vec![];
        };
        let Ok(value) = header.to_str() else {
            return vec![];
        };
        value
            .split(';')
            .filter_map(|kv| {
                let (name, val) = kv.trim().split_once('=')?;
                Some(CookieData {
                    name: name.to_string(),
                    value: val.to_string(),
                })
            })
            .collect()
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.cookies().into_iter().find(|c| c.name == name).map(|c| c.value)
    }

    /// The RSA public key is served as the `panel_public_key` cookie, whose
    /// value is a URL-encoded base64 PEM block.
    pub fn public_key_pem(&self) -> Result<String> {
        let raw = self
            .cookie("panel_public_key")
            .ok_or_else(|| anyhow!("未能从面板获取 RSA 公钥 (panel_public_key cookie)"))?;
        let decoded = url_decode(&raw)?;
        let pem = BASE64
            .decode(decoded.as_bytes())
            .map_err(|e| anyhow!("解码面板公钥失败: {e}"))?;
        Ok(String::from_utf8(pem)?)
    }

    pub fn csrf_header(&self) -> Option<HeaderValue> {
        self.cookie("pcsrftoken")
            .and_then(|v| HeaderValue::from_str(&v).ok())
    }

    pub fn get(&self, path: &str) -> Result<Response> {
        Ok(self.http.get(self.url(path)).send()?)
    }

    pub fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<Response> {
        let mut req = self.http.post(self.url(path));
        req = req.header(CONTENT_TYPE, "application/json");
        if let Some(csrf) = self.csrf_header() {
            req = req.header("X-CSRF-Token", csrf);
        }
        Ok(req.json(body).send()?)
    }

    /// Build a POST request without the CSRF header (used by the anonymous
    /// login endpoints).
    pub fn post_json_no_csrf(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        self.http.post(self.url(path)).header(CONTENT_TYPE, "application/json")
    }

    pub fn post(&self, path: &str) -> Result<Response> {
        let mut req = self.http.post(self.url(path));
        if let Some(csrf) = self.csrf_header() {
            req = req.header("X-CSRF-Token", csrf);
        }
        Ok(req.send()?)
    }

    pub fn put_json(&self, path: &str, body: &serde_json::Value) -> Result<Response> {
        let mut req = self.http.put(self.url(path));
        req = req.header(CONTENT_TYPE, "application/json");
        if let Some(csrf) = self.csrf_header() {
            req = req.header("X-CSRF-Token", csrf);
        }
        Ok(req.json(body).send()?)
    }

    pub fn delete(&self, path: &str) -> Result<Response> {
        let mut req = self.http.delete(self.url(path));
        if let Some(csrf) = self.csrf_header() {
            req = req.header("X-CSRF-Token", csrf);
        }
        Ok(req.send()?)
    }

    /// Multipart 上传文件到面板指定目录(files/upload)
    pub fn upload_file(
        &self,
        dest_dir: &str,
        file_path: &std::path::Path,
        overwrite: bool,
    ) -> Result<Response> {
        let form = reqwest::blocking::multipart::Form::new()
            .file("file", file_path)
            .map_err(|e| anyhow!("读取本地文件失败: {e}"))?
            .text("path", dest_dir.to_string())
            .text("overwrite", overwrite.to_string());
        let mut req = self.http.post(self.url("api/v2/files/upload"));
        if let Some(csrf) = self.csrf_header() {
            req = req.header("X-CSRF-Token", csrf);
        }
        Ok(req.multipart(form).send()?)
    }
}

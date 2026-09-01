use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, IsTerminal, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::client::PanelClient;
use crate::config::PanelConfig;
use crate::crypto::encrypt_password;
use crate::session::{save_session, Session};

#[derive(Debug, Deserialize)]
struct ApiResp<T> {
    code: i32,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginSetting {
    need_captcha: bool,
    #[serde(default)]
    panel_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptchaResp {
    captcha_id: String,
    image_path: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginData {
    #[serde(default)]
    mfa_status: String,
    #[serde(default)]
    mfa_session: String,
}

fn check_code(code: i32, message: &str) -> Result<()> {
    if code != 200 {
        return Err(anyhow!("面板返回错误 (code={code}): {message}"));
    }
    Ok(())
}

fn prompt(text: &str) -> Result<String> {
    print!("{text}");
    io::stdout().flush()?;
    let mut line = String::new();
    let n = io::stdin().read_line(&mut line)?;
    if n == 0 {
        return Err(anyhow!("读取输入失败(可能不是交互式终端)"));
    }
    Ok(line.trim().to_string())
}

fn now_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (y, mo, d) = civil_from_days(secs.div_euclid(86400));
    let h = secs.rem_euclid(86400) / 3600;
    let mi = secs.rem_euclid(3600) / 60;
    let s = secs.rem_euclid(60);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} (UTC)")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as i64, d)
}

pub fn login(cfg: &PanelConfig, profile: &str) -> Result<Session> {
    let client = PanelClient::new(&cfg.url, None, cfg.insecure)?;

    let setting: ApiResp<LoginSetting> = client
        .get("api/v2/core/auth/setting")?
        .json()
        .map_err(|e| anyhow!("解析面板设置响应失败: {e}"))?;
    check_code(setting.code, &setting.message)?;
    let setting_data = setting.data.unwrap_or(LoginSetting {
        need_captcha: false,
        panel_name: String::new(),
    });

    let public_key = client.public_key_pem()?;
    crate::crypto::debug_public_key(&public_key);

    let (captcha, captcha_id) = if setting_data.need_captcha {
        handle_captcha(&client, cfg)?
    } else {
        (String::new(), String::new())
    };

    let encrypted_password = encrypt_password(&cfg.password, &public_key)?;
    if std::env::var("PANEL_CLI_DEBUG").is_ok() {
        eprintln!("[debug] encrypted password: {encrypted_password}");
    }

    let body = json!({
        "name": cfg.username,
        "password": encrypted_password,
        "captcha": captcha,
        "captchaID": captcha_id,
        "language": cfg.language,
        "authSource": "local",
    });

    let resp = login_request(&client, "api/v2/core/auth/login", &body, &cfg.entrance)?;
    let data = resp.data.unwrap_or_default();

    if data.mfa_status == "Enable" {
        let code = match cfg.mfa_code.clone().filter(|c| !c.is_empty()) {
            Some(c) => c,
            None => {
                if !io::stdin().is_terminal() {
                    return Err(anyhow!(
                        "该面板已开启 MFA 动态验证码,请在 .env 中配置 PANEL_MFA_CODE 或通过 --mfa 参数提供"
                    ));
                }
                prompt("该面板已开启 MFA,请输入动态验证码: ")?
            }
        };
        let mfa_body = json!({
            "sessionId": data.mfa_session,
            "code": code,
        });
        login_request(
            &client,
            "api/v2/core/auth/mfalogin",
            &mfa_body,
            &cfg.entrance,
        )?;
    }

    let session = Session {
        profile: profile.to_string(),
        panel_url: client.base_url.clone(),
        entrance: cfg.entrance.clone(),
        username: cfg.username.clone(),
        panel_name: setting_data.panel_name,
        logged_in_at: now_string(),
        insecure: cfg.insecure,
        cookies: client.cookies(),
    };
    let path = save_session(&session)?;

    println!(
        "登录成功! 用户名: {} 面板: {}",
        session.username, session.panel_name
    );
    println!("会话凭据已保存到: {}", path.display());

    // 初始化面板本地 SSH 连接:.env 声明了 LINUX_SSH_USER/PWD 时立即写入,
    // 保证 exec 默认以该用户执行,而不是残留的 root 连接(root 操作用 --sudo 提权)
    if let (Some(user), Some(pwd)) = crate::config::linux_ssh_creds() {
        match crate::exec::ensure_local_conn(profile, &user, &pwd, crate::exec::DEFAULT_SSH_PORT) {
            Ok(()) => println!(
                "面板本地 SSH 连接已配置为用户 {user}(exec 将以该用户执行;root 操作加 --sudo)"
            ),
            Err(e) => eprintln!(
                "警告: 自动配置面板本地 SSH 连接失败: {e:#}\n(不影响登录;请修正 LINUX_SSH_USER/LINUX_SSH_PWD 后重跑 login,或运行 doctor 复查)"
            ),
        }
    }

    Ok(session)
}

fn login_request(
    client: &PanelClient,
    path: &str,
    body: &Value,
    entrance: &str,
) -> Result<ApiResp<LoginData>> {
    let mut req = client.post_json_no_csrf(path);
    if !entrance.is_empty() {
        let encoded = BASE64.encode(entrance.as_bytes());
        req = req.header("EntranceCode", encoded);
    }
    let resp: ApiResp<LoginData> = req
        .json(body)
        .send()
        .map_err(|e| anyhow!("请求面板失败: {e}"))?
        .json()
        .map_err(|e| anyhow!("解析面板响应失败: {e}"))?;
    if resp.code != 200 {
        return Err(anyhow!("登录失败 (code={}): {}", resp.code, resp.message));
    }
    Ok(resp)
}

fn fetch_captcha(client: &PanelClient) -> Result<CaptchaResp> {
    let resp: ApiResp<CaptchaResp> = client
        .get("api/v2/core/auth/captcha")?
        .json()
        .map_err(|e| anyhow!("解析验证码响应失败: {e}"))?;
    if resp.code != 200 {
        return Err(anyhow!(
            "获取验证码失败 (code={}): {}",
            resp.code,
            resp.message
        ));
    }
    resp.data.ok_or_else(|| anyhow!("验证码响应缺少数据"))
}

fn handle_captcha(client: &PanelClient, cfg: &PanelConfig) -> Result<(String, String)> {
    if let (Some(ans), Some(cid)) = (cfg.captcha.clone(), cfg.captcha_id.clone()) {
        if !ans.is_empty() && !cid.is_empty() {
            return Ok((ans, cid));
        }
    }
    let cap = fetch_captcha(client)?;
    if let Some(ans) = cfg.captcha.clone().filter(|c| !c.is_empty()) {
        return Ok((ans, cap.captcha_id));
    }
    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "面板要求输入验证码,但当前不是交互式终端。请通过 PANEL_CAPTCHA / PANEL_CAPTCHA_ID 环境变量或 --captcha / --captcha-id 参数提供"
        ));
    }
    let b64 = cap
        .image_path
        .split(',')
        .nth(1)
        .ok_or_else(|| anyhow!("验证码图片格式异常"))?;
    let bytes = BASE64
        .decode(b64.as_bytes())
        .map_err(|e| anyhow!("解码验证码图片失败: {e}"))?;
    let img_path = std::env::temp_dir().join(format!("1panel-cli-captcha-{}.png", cap.captcha_id));
    std::fs::write(&img_path, &bytes)?;
    println!(
        "面板要求输入验证码。验证码图片已保存到: {}",
        img_path.display()
    );
    println!("请打开图片,输入其中的算式结果(例如图片为 3+5=? 时输入 8)。");
    let ans = prompt("验证码答案: ")?;
    if ans.is_empty() {
        return Err(anyhow!("未输入验证码"));
    }
    Ok((ans, cap.captcha_id))
}

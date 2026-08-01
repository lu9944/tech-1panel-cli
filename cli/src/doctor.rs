use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

use crate::client::PanelClient;
use crate::config::{ConfigOverrides, PanelConfig};
use crate::session::load_session;

fn step(n: usize, total: usize, title: &str, ok: bool, detail: &str) {
    let mark = if ok { "通过" } else { "失败" };
    println!("[{n}/{total}] {title} ... {mark} ({detail})");
}

fn guidance(lines: &[&str]) {
    for l in lines {
        println!("    - {l}");
    }
}

pub fn doctor(profile: &str, force: bool) -> Result<()> {
    let total = 5;
    let mut all_ok = true;

    println!("1Panel CLI 环境检查 (profile: {profile})\n");

    // 1. .env 配置文件
    let env_path = find_env_file();
    let ok1 = env_path.is_some();
    let detail1 = env_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "未找到".to_string());
    step(1, total, ".env 配置文件", ok1, &detail1);
    if !ok1 {
        all_ok = false;
        guidance(&[
            "在以下位置创建 .env 文件:",
            "  1) 当前目录 ./.env",
            "  2) 可执行文件所在目录 ./.env",
            "  3) 或通过 --env <路径> 显式指定",
            "可参考项目中的 .env.example 模板。",
        ]);
    }

    // 2. 配置项
    let cfg = match PanelConfig::from_env(&ConfigOverrides::default()) {
        Ok(c) => Some(c),
        Err(e) => {
            println!("[2/5] 配置项 ... 失败 ({e})");
            guidance(&[
                "缺少环境变量: PANEL_URL / PANEL_USERNAME / PANEL_PASSWORD",
                "在 .env 中添加,例如:",
                "  PANEL_URL=http://10.0.0.1:41394",
                "  PANEL_USERNAME=admin",
                "  PANEL_PASSWORD=your_password",
            ]);
            None
        }
    };
    let cfg = match cfg {
        Some(c) => c,
        None => return Ok(()),
    };
    let detail2 = format!(
        "URL={}, USER={}, ENTRANCE={}",
        cfg.url,
        cfg.username,
        if cfg.entrance.is_empty() { "(无)" } else { &cfg.entrance }
    );
    let ok2 = !cfg.url.is_empty() && !cfg.username.is_empty() && !cfg.password.is_empty();
    step(2, total, "配置项", ok2, &detail2);
    if !ok2 {
        all_ok = false;
    }

    // 3. 面板地址连通性
    let client = match PanelClient::new(&cfg.url, None, cfg.insecure) {
        Ok(c) => c,
        Err(e) => {
            println!("[3/5] 面板地址 ... 失败 ({e})");
            return Ok(());
        }
    };
    let probe = client.get("api/v2/core/auth/setting");
    match probe {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().unwrap_or_default();
            let code = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["code"].as_i64());
            let ok3 = status == 200 && code == Some(200);
            step(3, total, "面板地址连通性", ok3, &format!("HTTP {status}"));
            if !ok3 {
                all_ok = false;
                guidance(&[
                    &format!("面板返回异常状态: HTTP {status}, body: {body:.120}"),
                    "请检查:",
                    "  1) PANEL_URL 是否正确(含端口与路径)并能在浏览器访问",
                    "  2) 服务器网络/防火墙是否放行了该端口",
                    "  3) 面板服务是否在运行(可执行 1pctl status 查看)",
                ]);
            }
        }
        Err(e) => {
            println!("[3/5] 面板地址 ... 失败 ({e})");
            all_ok = false;
            guidance(&[
                "无法连接面板地址,请检查:",
                "  1) PANEL_URL 格式是否为 http://IP:端口 或 https://域名",
                "  2) 目标主机是否可达(ping / curl 验证)",
                "  3) 面板端口是否放行;HTTPS 自签名证书需设置 PANEL_INSECURE=true",
            ]);
        }
    }

    // 4. 已保存会话
    let session = load_session(profile).ok();
    match &session {
        Some(s) => {
            if s.panel_url != cfg.url {
                println!(
                    "[4/5] 已保存会话 ... 失败 (会话地址 {} 与配置地址 {} 不一致)",
                    s.panel_url, cfg.url
                );
                all_ok = false;
                guidance(&[
                    "会话与 .env 中的面板地址不匹配,将重新登录。",
                    "如多个面板,请用 --profile 区分。",
                ]);
            } else {
                let sclient = match PanelClient::new(&s.panel_url, Some(&s.cookies), s.insecure) {
                    Ok(c) => c,
                    Err(_) => {
                        println!("[4/5] 已保存会话 ... 失败 (凭据解析失败)");
                        return Ok(());
                    }
                };
                match sclient.get("api/v2/core/auth/current") {
                    Ok(resp) => {
                        let body = resp.text().unwrap_or_default();
                        let code = serde_json::from_str::<Value>(&body)
                            .ok()
                            .and_then(|v| v["code"].as_i64());
                        let ok4 = code == Some(200);
                        let detail4 = if ok4 {
                            serde_json::from_str::<Value>(&body)
                                .ok()
                                .and_then(|v| v["data"]["name"].as_str().map(|s| s.to_string()))
                                .unwrap_or_else(|| "有效".to_string())
                        } else {
                            "会话已失效".to_string()
                        };
                        step(4, total, "已保存会话", ok4, &format!("用户名 {detail4}"));
                        if !ok4 {
                            all_ok = false;
                            guidance(&[
                                "会话已过期或被服务器注销,将尝试重新登录(见下一步)。",
                            ]);
                        }
                    }
                    Err(e) => {
                        println!("[4/5] 已保存会话 ... 失败 ({e})");
                        all_ok = false;
                    }
                }
            }
        }
        None => {
            println!("[4/5] 已保存会话 ... 跳过 (尚未登录,将直接登录)");
        }
    }

    // 5. 登录验证
    let session_ok = match &session {
        Some(s) => s.panel_url == cfg.url && verify_session(s),
        None => false,
    };
    if session_ok && !force {
        println!("[5/5] 登录验证 ... 通过 (会话有效,无需重新登录;可用 --force 强制验证凭据)");
    } else {
        if force {
            println!("[5/5] 登录验证 ... 使用 --force 强制重新登录以验证凭据");
        }
        match crate::auth::login(&cfg, profile) {
            Ok(_) => {
                step(5, total, "登录验证", true, "登录成功,凭据已保存");
            }
            Err(e) => {
                println!("[5/5] 登录验证 ... 失败 ({e})");
                all_ok = false;
                guidance(&[
                    "请检查:",
                    "  1) PANEL_USERNAME / PANEL_PASSWORD 是否正确",
                    "  2) 若提示验证码:连续登录失败会触发验证码,请稍后重试或在面板清除限制",
                    "  3) 若提示 ErrEntrance:PANEL_ENTRANCE 安全入口是否正确",
                    "  4) 若开启 MFA:通过 PANEL_MFA_CODE 环境变量或 --mfa 参数提供动态验证码",
                ]);
            }
        }
    }

    println!();
    if all_ok {
        println!("环境检查全部通过,可以开始使用:\n  {:<20} 检查登录状态", "1panel-cli status");
        println!("  {:<20} 列出可安装应用", "1panel-cli apps list");
        println!("  {:<20} 更多命令见 1panel-cli --help", "");
    } else {
        println!("环境检查未全部通过,请按上面提示修复后重试。");
    }
    Ok(())
}

fn find_env_file() -> Option<PathBuf> {
    let candidates = {
        let mut v = vec![PathBuf::from(".env")];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                v.push(dir.join(".env"));
            }
        }
        v
    };
    candidates.into_iter().find(|p| p.exists())
}

fn verify_session(s: &crate::session::Session) -> bool {
    let Ok(client) = PanelClient::new(&s.panel_url, Some(&s.cookies), s.insecure) else {
        return false;
    };
    let Ok(resp) = client.get("api/v2/core/auth/current") else {
        return false;
    };
    let Ok(body) = resp.text() else {
        return false;
    };
    serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v["code"].as_i64())
        == Some(200)
}

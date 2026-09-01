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

fn step_warn(n: usize, total: usize, title: &str, detail: &str) {
    println!("[{n}/{total}] {title} ... 警告 ({detail})");
}

fn guidance(lines: &[&str]) {
    for l in lines {
        println!("    - {l}");
    }
}

fn guidance_owned(lines: &[String]) {
    guidance(&lines.iter().map(String::as_str).collect::<Vec<_>>());
}

pub fn doctor(profile: &str, force: bool) -> Result<()> {
    let total = 8;
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
            println!("[2/{total}] 配置项 ... 失败 ({e})");
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
        if cfg.entrance.is_empty() {
            "(无)"
        } else {
            &cfg.entrance
        }
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
            println!("[3/{total}] 面板地址 ... 失败 ({e})");
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
            println!("[3/{total}] 面板地址 ... 失败 ({e})");
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
                    "[4/{total}] 已保存会话 ... 失败 (会话地址 {} 与配置地址 {} 不一致)",
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
                        println!("[4/{total}] 已保存会话 ... 失败 (凭据解析失败)");
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
                            guidance(&["会话已过期或被服务器注销,将尝试重新登录(见下一步)。"]);
                        }
                    }
                    Err(e) => {
                        println!("[4/{total}] 已保存会话 ... 失败 ({e})");
                        all_ok = false;
                    }
                }
            }
        }
        None => {
            println!("[4/{total}] 已保存会话 ... 跳过 (尚未登录,将直接登录)");
        }
    }

    // 5. 登录验证
    let session_ok = match &session {
        Some(s) => s.panel_url == cfg.url && verify_session(s),
        None => false,
    };
    if session_ok && !force {
        println!("[5/{total}] 登录验证 ... 通过 (会话有效,无需重新登录;可用 --force 强制验证凭据)");
    } else {
        if force {
            println!("[5/{total}] 登录验证 ... 使用 --force 强制重新登录以验证凭据");
        }
        match crate::auth::login(&cfg, profile) {
            Ok(_) => {
                step(5, total, "登录验证", true, "登录成功,凭据已保存");
            }
            Err(e) => {
                println!("[5/{total}] 登录验证 ... 失败 ({e})");
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

    match crate::commands::panel_version(profile) {
        Ok((version, edition)) => {
            let compatible = version.starts_with("v2.") || version.starts_with("2.");
            step(
                6,
                total,
                "面板版本兼容性",
                compatible,
                &format!("{version} ({edition});已验证基线 dev-v2"),
            );
            if !compatible {
                all_ok = false;
                guidance(&["当前技能包面向 1Panel v2;请升级面板后再使用写操作。"]);
            }
        }
        Err(e) => {
            step(6, total, "面板版本兼容性", false, &e.to_string());
            all_ok = false;
        }
    }

    // 7. 本地 SSH 连接(exec 前置):配置状态 + 与 .env 声明用户对齐 + 实测连通性
    let creds = crate::config::linux_ssh_creds();
    let mut conn_user: Option<String> = crate::exec::local_conn_info(profile)
        .ok()
        .filter(|c| !c.addr.is_empty())
        .map(|c| c.user);

    // .env 声明了凭据且连接缺失/用户不一致时,自动覆盖写入(exec 默认以声明用户执行,不残留 root)
    if let (Some(user), Some(pwd)) = (&creds.0, &creds.1) {
        let mismatch = match &conn_user {
            None => true,
            Some(u) => !u.is_empty() && u != user,
        };
        if mismatch {
            match crate::exec::ensure_local_conn(profile, user, pwd, crate::exec::DEFAULT_SSH_PORT)
            {
                Ok(()) => {
                    println!(
                        "[7/{total}] 本地 SSH 连接 ... 自动改配为用户 {user}(原为 {})",
                        conn_user.as_deref().unwrap_or("未配置")
                    );
                    conn_user = Some(user.clone());
                }
                Err(e) => {
                    step(
                        7,
                        total,
                        "本地 SSH 连接 (exec 前置)",
                        false,
                        &format!("自动改配失败: {e:#}"),
                    );
                    all_ok = false;
                    guidance_owned(&[
                        "写入面板「设置 → 终端 → SSH 本地连接」失败:".to_string(),
                        "  1) 检查 LINUX_SSH_USER / LINUX_SSH_PWD 是否正确".to_string(),
                        "  2) 确认主机 SSH 服务已启用(端口默认 22,可用 exec --ssh-port 调整)".to_string(),
                    ]);
                }
            }
        }
    }

    let mut ssh_ok = false;
    let who = conn_user.clone().unwrap_or_default();
    match conn_user {
        None => {
            step(
                7,
                total,
                "本地 SSH 连接 (exec 前置)",
                false,
                "面板未配置,且 .env 未声明 LINUX_SSH_USER / LINUX_SSH_PWD",
            );
            all_ok = false;
            guidance_owned(&[
                "exec 需要面板先配置「设置 → 终端 → SSH 本地连接」:".to_string(),
                "  1) 在 .env 中添加 LINUX_SSH_USER / LINUX_SSH_PWD,重跑 doctor 或 login 即自动写入".to_string(),
                "  2) 或到面板 UI 手动配置".to_string(),
            ]);
        }
        Some(user) => match crate::exec::run_capture(profile, "whoami", false, 20) {
            Ok((0, out)) => {
                let actual = out.trim();
                let extra = if actual.is_empty() {
                    String::new()
                } else if actual == user {
                    format!(",whoami 实测 {actual}")
                } else {
                    format!(",whoami 实测为 {actual}(与连接声明用户不一致)")
                };
                step(
                    7,
                    total,
                    "本地 SSH 连接 (exec 前置)",
                    true,
                    &format!("用户 {user}{extra}"),
                );
                ssh_ok = true;
            }
            Ok((code, out)) => {
                step(
                    7,
                    total,
                    "本地 SSH 连接 (exec 前置)",
                    false,
                    &format!("whoami 退出码 {code}: {}", out.trim()),
                );
                all_ok = false;
                guidance_owned(&[
                    format!("用户 {user} 的本地 SSH 连接验证失败,exec 无法执行:"),
                    "  1) 凭据可能已变更:重跑 1panel-cli login 自动改配".to_string(),
                    format!("  2) 或到面板「设置 → 终端 → SSH 本地连接」核对 {user} 的密码与端口"),
                ]);
            }
            Err(e) => {
                step(7, total, "本地 SSH 连接 (exec 前置)", false, &format!("{e:#}"));
                all_ok = false;
                guidance_owned(&[
                    "exec 连通性验证失败:".to_string(),
                    "  1) 重跑 1panel-cli login(或 exec --sync-ssh)改配本地 SSH 连接".to_string(),
                    "  2) 确认主机 SSH 服务可达".to_string(),
                ]);
            }
        },
    }

    // 8. sudo 免密(exec --sudo 前置;不阻塞整体通过,仅告警)
    if !ssh_ok {
        println!("[8/{total}] sudo 免密 ... 跳过 (上一步 SSH 连接未验证通过)");
    } else {
        match crate::exec::run_capture(profile, "true", true, 20) {
            Ok((0, _)) => {
                step(
                    8,
                    total,
                    "sudo 免密 (exec --sudo 前置)",
                    true,
                    &format!("用户 {who} 可免密 sudo"),
                );
            }
            Ok((code, out)) => {
                step_warn(
                    8,
                    total,
                    "sudo 免密 (exec --sudo 前置)",
                    &format!("sudo -n 退出码 {code}: {}", out.trim()),
                );
                guidance_owned(&[
                    format!("用户 {who} 未配置免密 sudo,exec --sudo 将失败;如需提权,在主机上配置:"),
                    format!("  echo '{who} ALL=(ALL) NOPASSWD:ALL' | sudo tee /etc/sudoers.d/1panel-cli"),
                    "  或将本地 SSH 连接改配为 root(此时不需要 --sudo)".to_string(),
                ]);
            }
            Err(e) => {
                step_warn(8, total, "sudo 免密 (exec --sudo 前置)", &format!("{e:#}"));
                guidance_owned(&[
                    format!("无法验证用户 {who} 的免密 sudo;如需 --sudo 提权,在主机上配置:"),
                    format!("  echo '{who} ALL=(ALL) NOPASSWD:ALL' | sudo tee /etc/sudoers.d/1panel-cli"),
                ]);
            }
        }
    }

    println!();
    if all_ok {
        println!(
            "环境检查全部通过,可以开始使用:\n  {:<20} 检查登录状态",
            "1panel-cli status"
        );
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

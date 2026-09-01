use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as WsError, HandshakeError, Message, WebSocket};

use crate::client::PanelClient;
use crate::config;
use crate::session::{load_session, Session};

pub const DEFAULT_TIMEOUT_SECS: i64 = 30;
pub const DEFAULT_COLS: i64 = 120;
pub const DEFAULT_ROWS: i64 = 40;
pub const DEFAULT_SSH_PORT: u16 = 22;
/// 收集输出的上限(超出截断并标记 truncated)
pub const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

const WS_PATH: &str = "/api/v2/hosts/terminal/local";
const SENTINEL_PREFIX: &str = "__1PCLI_";
const MAX_COMMAND_LEN: usize = 4000;
/// 哨兵匹配用的归一化流窗口大小(环形,防超大输出撑爆内存)
const MATCH_WINDOW_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ExecOptions {
    pub command: String,
    pub timeout: i64,
    pub cols: i64,
    pub rows: i64,
    pub json: bool,
    pub tail: Option<usize>,
    pub raw: bool,
    pub cwd: Option<String>,
    pub sync_ssh: bool,
    pub ssh_port: u16,
    /// 以 sudo 执行(要求 SSH 用户配置了免密 sudo)
    pub sudo: bool,
}

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub exit_code: i32,
    pub duration_ms: u64,
    pub truncated: bool,
    pub error: Option<String>,
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct WsMsg {
    #[serde(rename = "type")]
    pub ws_type: String,
    #[serde(default)]
    pub data: String,
}

/// 由面板 base URL 构建终端 WS 握手 URL(无需拼接安全入口路径)
pub fn ws_url(session: &Session, command: &str, cols: i64, rows: i64) -> Result<String> {
    if cols <= 0 || rows <= 0 {
        bail!("--cols/--rows 必须为正整数(服务端要求纯数字)");
    }
    let base = session.panel_url.trim().trim_end_matches('/');
    let ws_base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if base.starts_with("ws://") || base.starts_with("wss://") {
        base.to_string()
    } else {
        format!("ws://{base}")
    };
    if ws_base.len() <= 6 {
        bail!("面板地址不能为空");
    }
    let cmd = utf8_percent_encode(command, NON_ALPHANUMERIC).to_string();
    Ok(format!(
        "{ws_base}{WS_PATH}?command={cmd}&cols={cols}&rows={rows}"
    ))
}

/// WS 握手附加请求头:全部会话 Cookie + CurrentNode(与 client.rs 保持一致)
pub fn build_handshake(session: &Session) -> Vec<(&'static str, String)> {
    let cookie = session
        .cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");
    let node = std::env::var(config::ENV_NODE).unwrap_or_else(|_| "local".into());
    let node = utf8_percent_encode(node.trim(), NON_ALPHANUMERIC).to_string();
    vec![("cookie", cookie), ("currentnode", node)]
}

/// `POST api/v2/settings/ssh` (SaveLocalConn) 的请求体,password 需 base64 编码
pub fn local_conn_body(user: &str, pwd: &str, port: u16) -> Value {
    json!({
        "addr": "127.0.0.1",
        "port": port,
        "user": user,
        "authMode": "password",
        "password": BASE64.encode(pwd.as_bytes()),
    })
}

/// 写入面板"本地 SSH 连接"(保存即由面板真实拨号校验)。
/// 供登录初始化与 doctor 复用:确保 exec 以 .env 声明的用户执行,而非残留的 root 连接。
pub fn ensure_local_conn(profile: &str, user: &str, pwd: &str, port: u16) -> Result<()> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    let resp = client.post_json("api/v2/settings/ssh", &local_conn_body(user, pwd, port))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if status.as_u16() == 401 {
        bail!("会话已失效,请重新运行 login");
    }
    let parsed: Result<Value, _> = serde_json::from_str(&text);
    match parsed {
        Ok(v) => {
            let code = v["code"].as_i64().unwrap_or(-1);
            if code != 200 {
                bail!(
                    "配置面板本地 SSH 连接失败 (code={code}): {}",
                    v["message"].as_str().unwrap_or("未知错误")
                );
            }
        }
        Err(_) => {
            bail!("配置面板本地 SSH 连接失败: HTTP {status}");
        }
    }
    Ok(())
}

fn quote_posix(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// 组装发送给 shell 的完整命令:预校验 + cwd 拼接 + 子 shell 包裹 + 退出码哨兵。
/// 用户命令放进 `( )` 子 shell:`exit`/`exec` 类命令只结束子 shell,
/// 外层 shell 继续执行哨兵 echo,退出码经 `$?` 透传。
/// sudo 为 true 时命令包进 `sudo -n -H bash -c`,cd 一并移入 root 环境内执行。
pub fn compose_command(
    cwd: Option<&str>,
    command: &str,
    token: &str,
    sudo: bool,
) -> Result<String> {
    let command = command.trim();
    if command.is_empty() {
        bail!("命令不能为空");
    }
    if command.contains('\n') || command.contains('\r') {
        bail!("命令不能包含换行符(exec 仅支持单行命令)");
    }
    if command.len() > MAX_COMMAND_LEN {
        eprintln!(
            "警告: 命令长度 {} 字节,超过 {MAX_COMMAND_LEN} 字节,可能超出 URL 长度限制导致失败",
            command.len()
        );
    }
    let cd_prefix = match cwd.map(str::trim).filter(|d| !d.is_empty()) {
        Some(dir) => {
            if dir.contains('\n') || dir.contains('\r') {
                bail!("--cwd 目录不能包含换行符");
            }
            Some(format!("cd -- {} && ", quote_posix(dir)))
        }
        None => None,
    };
    let mut full = match (sudo, cd_prefix) {
        (true, prefix) => {
            let mut inner = prefix.unwrap_or_default();
            inner.push_str(command);
            format!("( sudo -n -H bash -c {} )", quote_posix(&inner))
        }
        (false, prefix) => match prefix {
            Some(p) => format!("{p}( {command} )"),
            None => format!("( {command} )"),
        },
    };
    full.push_str(&format!("; echo {SENTINEL_PREFIX}{token}_$?"));
    Ok(full)
}

/// 生成 8 字符 hex 随机哨兵 token
pub fn new_token() -> String {
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| format!("{:x}", rng.gen_range(0..16)))
        .collect()
}

/// 在归一化流(剥离 ANSI 转义与 \r\n)上匹配哨兵,单次命中即完成,
/// 捕获组即命令退出码;回显中的字面量 `$?` 不参与匹配。
pub fn find_sentinel(normalized: &str, token: &str) -> Option<i32> {
    let needle = format!("{SENTINEL_PREFIX}{token}_");
    let mut search_from = 0usize;
    loop {
        let found = normalized[search_from..].find(&needle)?;
        let start = search_from + found + needle.len();
        let bytes = normalized.as_bytes();
        let mut end = start;
        while end < bytes.len() && end - start < 3 && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end > start {
            if let Ok(code) = normalized[start..end].parse::<i32>() {
                return Some(code);
            }
        }
        search_from = start;
    }
}

/// 剥离 ANSI/ECMA-48 转义序列(CSI/OSC/双字符序列)与 BEL
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if c != '\x07' {
                out.push(c);
            }
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                // CSI: 参数 0x30-0x3F,中间字节 0x20-0x2F,终止字节 0x40-0x7E
                for n in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // OSC: 由 BEL 或 ESC \ 终止
                while let Some(n) = chars.next() {
                    if n == '\x07' {
                        break;
                    }
                    if n == '\x1b' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            Some('(') | Some(')') | Some('*') | Some('+') | Some('#') | Some('%') => {
                chars.next();
                chars.next();
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// 归一化用于哨兵匹配:剥离 ANSI 转义并移除所有 \r 与 \n(规避 PTY 折行拆断)
fn normalize_chunk(chunk: &str) -> String {
    strip_ansi(chunk)
        .chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .collect()
}

fn trim_blank_edges<'a>(lines: &'a [&'a str]) -> Vec<&'a str> {
    let mut start = 0;
    let mut end = lines.len();
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    lines[start..end].to_vec()
}

/// 默认输出清洗:以哨兵行为锚,剥离 ANSI、PTY 回显输入行、哨兵行及尾部提示符残余
pub fn clean_output(raw: &str, token: &str) -> String {
    let text = strip_ansi(raw);
    // 行内残留的光标归位 \r(如 clear 序列后的裸 \r)一并去掉
    let lines: Vec<&str> = text.lines().map(|l| l.trim_matches('\r')).collect();
    let token_full = format!("{SENTINEL_PREFIX}{token}");

    // 回显输入行锚:字面量 `$?`(未展开)或 ` clear &&` 前缀
    let start = if let Some(i) = lines
        .iter()
        .rposition(|l| l.contains(&format!("{token_full}_$")))
    {
        i + 1
    } else if let Some(i) = lines.iter().rposition(|l| l.starts_with(" clear &&")) {
        i + 1
    } else {
        0
    };

    // 哨兵输出行(展开后的数字)之后的提示符残余一并丢弃
    let end = lines[start..]
        .iter()
        .rposition(|l| l.contains(&token_full))
        .map(|rel| start + rel)
        .unwrap_or(lines.len());

    let kept: Vec<&str> = lines[start..end]
        .iter()
        .copied()
        .filter(|l| !l.contains(&token_full))
        .collect();
    trim_blank_edges(&kept).join("\n")
}

/// 仅保留最后 n 行,返回 (文本, 是否发生截断)
pub fn tail_lines(text: &str, n: usize) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= n {
        return (text.to_string(), false);
    }
    if n == 0 {
        return (String::new(), true);
    }
    (lines[lines.len() - n..].join("\n"), true)
}

/// 序列化单行 JSON 结果(字段顺序固定,便于脚本/AI 消费)
pub fn to_json(result: &ExecResult) -> Result<String> {
    serde_json::to_string(result).context("序列化 exec 结果失败")
}

fn insecure_tls_config() -> Result<rustls::ClientConfig> {
    #[derive(Debug)]
    struct NoVerify;
    impl rustls::client::danger::ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error>
        {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
        {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
        {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .mapping
                .iter()
                .map(|(scheme, _)| *scheme)
                .collect()
        }
    }
    let verifier: Arc<dyn rustls::client::danger::ServerCertVerifier> = Arc::new(NoVerify);
    let cfg = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(cfg)
}

fn friendly_ws_error(e: WsError) -> anyhow::Error {
    match e {
        WsError::Http(resp) => {
            let status = resp.status();
            match status.as_u16() {
                401 => anyhow!("会话已失效,请重新运行 login"),
                403 => anyhow!("面板拒绝访问 (HTTP 403): 可能是绑定域名限制或演示模式"),
                404 => anyhow!("终端接口不存在 (HTTP 404): 请确认面板版本 (需 v2.2.0+)"),
                407 => anyhow!("面板正在升级或维护中 (HTTP 407),请稍后重试"),
                _ => anyhow!("WS 握手失败: HTTP {status}"),
            }
        }
        WsError::Tls(t) => anyhow!("TLS 连接失败: {t}(自签名证书可运行 login 时加 --insecure)"),
        other => anyhow!("WS 连接失败: {other}"),
    }
}

fn connect_ws(session: &Session, url: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
    let uri: http::Uri = url
        .parse()
        .with_context(|| format!("解析 WS 地址失败: {url}"))?;
    let is_tls = uri.scheme_str() == Some("wss");
    let host = uri
        .host()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("WS 地址缺少主机名: {url}"))?;
    let port = uri.port_u16().unwrap_or(if is_tls { 443 } else { 80 });

    let mut request = url
        .into_client_request()
        .map_err(|e| anyhow!("构造 WS 握手请求失败: {e}"))?;
    {
        let headers = request.headers_mut();
        for (name, value) in build_handshake(session) {
            let hv = http::HeaderValue::from_str(&value)
                .map_err(|e| anyhow!("非法的请求头 {name}: {e}"))?;
            headers.insert(http::HeaderName::from_static(name), hv);
        }
    }

    let stream = TcpStream::connect((host.as_str(), port))
        .with_context(|| format!("连接面板失败: {host}:{port}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let _ = stream.set_nodelay(true);

    let connector = if is_tls && session.insecure {
        Some(tungstenite::Connector::Rustls(Arc::new(
            insecure_tls_config()?,
        )))
    } else {
        None
    };

    match tungstenite::client_tls_with_config(request, stream, None, connector) {
        Ok((ws, _resp)) => Ok(ws),
        Err(HandshakeError::Failure(e)) => Err(friendly_ws_error(e)),
        Err(HandshakeError::Interrupted(_)) => Err(anyhow!("WS 握手被中断")),
    }
}

fn set_read_deadline(ws: &WebSocket<MaybeTlsStream<TcpStream>>, d: Duration) {
    let tcp = match ws.get_ref() {
        MaybeTlsStream::Plain(s) => Some(s),
        MaybeTlsStream::Rustls(s) => Some(s.get_ref()),
        _ => None,
    };
    if let Some(t) = tcp {
        let _ = t.set_read_timeout(Some(d));
    }
}

fn close_quiet(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    let _ = ws.close(None);
    let _ = ws.flush();
}

/// 面板「SSH 本地连接」概要(GET api/v2/settings/ssh/conn)
#[derive(Debug, Default, Clone)]
pub struct LocalConnInfo {
    pub addr: String,
    pub user: String,
}

/// 读取面板本地 SSH 连接概要(addr 为空即未配置)
pub fn local_conn_info(profile: &str) -> Result<LocalConnInfo> {
    let session = load_session(profile)?;
    let client = PanelClient::new(&session.panel_url, Some(&session.cookies), session.insecure)?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ConnInfo {
        #[serde(default)]
        addr: String,
        #[serde(default)]
        user: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        data: Option<ConnInfo>,
    }
    let resp: Resp = client
        .get("api/v2/settings/ssh/conn")?
        .json()
        .map_err(|e| anyhow!("解析本地连接信息失败: {e}"))?;
    Ok(resp.data.map(|d| LocalConnInfo {
        addr: d.addr,
        user: d.user,
    }).unwrap_or_default())
}

/// 探测面板本地 SSH 连接是否已配置(GET api/v2/settings/ssh/conn 的 addr 字段)
fn local_conn_addr(profile: &str) -> Result<String> {
    Ok(local_conn_info(profile)?.addr)
}

/// 服务端以 Close 帧终止终端会话后的错误归因。
/// 注意:面板 agent 发送的 Close 帧 payload 不含 RFC 状态码,
/// tungstenite 会把不可识别的帧替换为 reason="Protocol violation",
/// 因此这里结合 settings/ssh/conn 探测给出准确归因。
fn explain_server_close(profile: &str, reason: &str) -> anyhow::Error {
    let r = reason.trim();
    let readable = !r.is_empty() && r != "Protocol violation";
    if let Ok(addr) = local_conn_addr(profile) {
        if addr.is_empty() {
            return anyhow!(
                "面板本地 SSH 连接未配置{}\n提示: 运行 exec 时加 --sync-ssh(需在 .env 中配置 LINUX_SSH_USER / LINUX_SSH_PWD),或到面板『设置 → 终端 → SSH 本地连接』手动配置",
                if readable { format!(": {r}") } else { String::new() }
            );
        }
    }
    if readable {
        if r.contains("demo server") {
            return anyhow!("面板处于演示模式,终端功能被禁用: {r}");
        }
        anyhow!("服务端终止了终端连接: {r}")
    } else {
        anyhow!(
            "服务端终止了终端连接: 本地 SSH 连接验证可能失败(凭据已变更或连接不可达)\n提示: 可用 --sync-ssh 配合 LINUX_SSH_USER / LINUX_SSH_PWD 覆盖更新"
        )
    }
}

enum EndState {
    Completed(i32),
    ServerClosed(String),
    Disconnected(&'static str),
    TimedOut,
}

#[allow(clippy::too_many_arguments)]
fn run_ws_session(
    session: &Session,
    composed: &str,
    token: &str,
    opts: &ExecOptions,
) -> Result<(EndState, Vec<u8>, bool, u64)> {
    let url = ws_url(session, composed, opts.cols, opts.rows)?;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(opts.timeout.max(1) as u64);
    let mut ws = connect_ws(session, &url)?;

    let mut buf: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut match_buf = String::new();
    #[allow(unused_assignments)]
    let mut end_state: Option<EndState> = None;

    loop {
        if Instant::now() >= deadline {
            end_state = Some(EndState::TimedOut);
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        set_read_deadline(&ws, remaining.max(Duration::from_millis(1)));

        match ws.read() {
            Ok(Message::Text(txt)) => {
                let mut chunk = String::new();
                match serde_json::from_str::<WsMsg>(&txt) {
                    Ok(msg) if msg.ws_type == "cmd" => match BASE64.decode(msg.data.as_bytes()) {
                        Ok(decoded) => chunk.push_str(&String::from_utf8_lossy(&decoded)),
                        Err(_) => chunk.push_str(&txt),
                    },
                    Ok(_) => {}
                    Err(_) => chunk.push_str(&txt),
                }
                if !chunk.is_empty() {
                    if buf.len() + chunk.len() > MAX_OUTPUT_BYTES {
                        let room = MAX_OUTPUT_BYTES.saturating_sub(buf.len());
                        buf.extend(chunk.as_bytes()[..room.min(chunk.len())].iter().copied());
                        truncated = true;
                    } else {
                        buf.extend(chunk.as_bytes().iter().copied());
                    }
                    match_buf.push_str(&normalize_chunk(&chunk));
                    if match_buf.len() > MATCH_WINDOW_BYTES + 64 * 1024 {
                        let mut cut = match_buf.len() - MATCH_WINDOW_BYTES;
                        while !match_buf.is_char_boundary(cut) {
                            cut += 1;
                        }
                        match_buf.drain(..cut);
                    }
                    if let Some(code) = find_sentinel(&match_buf, token) {
                        end_state = Some(EndState::Completed(code));
                        break;
                    }
                }
            }
            Ok(Message::Binary(data)) => {
                let chunk = String::from_utf8_lossy(&data).to_string();
                if buf.len() + chunk.len() > MAX_OUTPUT_BYTES {
                    let room = MAX_OUTPUT_BYTES.saturating_sub(buf.len());
                    buf.extend(chunk.as_bytes()[..room.min(chunk.len())].iter().copied());
                    truncated = true;
                } else {
                    buf.extend(chunk.as_bytes().iter().copied());
                }
                match_buf.push_str(&normalize_chunk(&chunk));
                if let Some(code) = find_sentinel(&match_buf, token) {
                    end_state = Some(EndState::Completed(code));
                    break;
                }
            }
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
            Ok(Message::Close(frame)) => {
                end_state = Some(match frame {
                    Some(f) => EndState::ServerClosed(f.reason.to_string()),
                    None => EndState::Disconnected("连接被服务端关闭(未携带原因)"),
                });
                break;
            }
            Err(WsError::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
            {
                end_state = Some(EndState::TimedOut);
                break;
            }
            Err(WsError::ConnectionClosed) | Err(WsError::AlreadyClosed) => {
                end_state = Some(EndState::Disconnected("连接被意外关闭"));
                break;
            }
            Err(e) => {
                let _ = ws.flush();
                return Err(anyhow!("WebSocket 读取失败: {e}"));
            }
        }
        // 及时把自动应答的 Pong 等待发帧刷出去
        let _ = ws.flush();
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    close_quiet(&mut ws);
    match end_state {
        Some(state) => Ok((state, buf, truncated, duration_ms)),
        None => Ok((EndState::TimedOut, buf, truncated, duration_ms)),
    }
}

/// 构建最终结果:应用清洗/--raw/--tail;返回 (结果, tail 是否截断)
fn build_result(
    opts: &ExecOptions,
    raw: &[u8],
    token: &str,
    truncated: bool,
    error: Option<String>,
    exit_code: i32,
    duration_ms: u64,
) -> (ExecResult, bool) {
    let raw_text = String::from_utf8_lossy(raw).to_string();
    let cleaned = if opts.raw {
        raw_text
    } else {
        clean_output(&raw_text, token)
    };
    let (output, tail_truncated) = match opts.tail {
        Some(n) => tail_lines(&cleaned, n),
        None => (cleaned, false),
    };
    let result = ExecResult {
        exit_code,
        duration_ms,
        truncated,
        error,
        output,
    };
    (result, tail_truncated)
}

/// 输出最终结果:--json 打印单行 JSON,否则打印(清洗/tail 后的)文本
fn finish(result: &ExecResult, opts: &ExecOptions, tail_truncated: bool) -> Result<i32> {
    if opts.json {
        println!("{}", to_json(result)?);
        return Ok(result.exit_code);
    }
    if tail_truncated {
        eprintln!("[提示] 输出超长,仅显示末尾部分(完整内容可在远端重定向到文件后用 tail 查看)");
    }
    if result.truncated {
        eprintln!(
            "[提示] 输出超过 {} MB 上限,已截断(建议远端重定向到文件后用 tail 查看)",
            MAX_OUTPUT_BYTES / 1024 / 1024
        );
    }
    if !result.output.is_empty() {
        println!("{}", result.output);
    }
    Ok(result.exit_code)
}

/// 出错时的兜底:--json 打印带 error 的 JSON 并返回 1,否则交给上层报错
fn fail_with_output(result: &ExecResult, opts: &ExecOptions, err: anyhow::Error) -> Result<i32> {
    if opts.json {
        println!("{}", to_json(result)?);
        Ok(1)
    } else {
        if !result.output.is_empty() {
            println!("{}", result.output);
        }
        Err(err)
    }
}

/// 入口:返回进程退出码
pub fn run(profile: &str, opts: &ExecOptions) -> Result<i32> {
    match run_inner(profile, opts) {
        Ok(code) => Ok(code),
        Err(e) => {
            if opts.json {
                let payload = json!({
                    "exit_code": 1,
                    "duration_ms": 0,
                    "truncated": false,
                    "error": format!("{e:#}"),
                    "output": "",
                });
                println!("{payload}");
                Ok(1)
            } else {
                Err(e)
            }
        }
    }
}

fn run_inner(profile: &str, opts: &ExecOptions) -> Result<i32> {
    match execute(profile, opts)? {
        ExecOutcome::Done(result, tail_truncated) => finish(&result, opts, tail_truncated),
        ExecOutcome::Failed(result, err) => fail_with_output(&result, opts, err),
    }
}

/// 供 doctor 等内部调用:执行单行命令,返回 (退出码, 清洗后输出),不打印。
pub fn run_capture(profile: &str, command: &str, sudo: bool, timeout: i64) -> Result<(i32, String)> {
    let opts = ExecOptions {
        command: command.to_string(),
        timeout,
        cols: DEFAULT_COLS,
        rows: DEFAULT_ROWS,
        json: false,
        tail: None,
        raw: false,
        cwd: None,
        sync_ssh: false,
        ssh_port: DEFAULT_SSH_PORT,
        sudo,
    };
    match execute(profile, &opts)? {
        ExecOutcome::Done(result, _) => Ok((result.exit_code, result.output)),
        ExecOutcome::Failed(_, err) => Err(err),
    }
}

/// 执行结果:完成(可打印)或失败(按 --json/文本分流)
enum ExecOutcome {
    Done(ExecResult, bool),
    Failed(ExecResult, anyhow::Error),
}

/// 执行主流程:预校验 + SSH 连接对齐 + WS 会话,构建结果(不打印)
fn execute(profile: &str, opts: &ExecOptions) -> Result<ExecOutcome> {
    if opts.timeout <= 0 {
        bail!("--timeout 必须为正整数秒");
    }
    if opts.cols <= 0 || opts.rows <= 0 {
        bail!("--cols/--rows 必须为正整数");
    }
    let session = load_session(profile)?;
    let token = new_token();
    let composed = compose_command(opts.cwd.as_deref(), &opts.command, &token, opts.sudo)?;

    if opts.sync_ssh {
        let (user, pwd) = config::linux_ssh_creds();
        let (Some(user), Some(pwd)) = (user, pwd) else {
            bail!("--sync-ssh 需要在 .env(或环境变量)中配置 LINUX_SSH_USER / LINUX_SSH_PWD");
        };
        ensure_local_conn(profile, &user, &pwd, opts.ssh_port)?;
    } else {
        // 自动对齐:.env 声明了 SSH 用户时,不允许残留 root 等其他连接,
        // 连接缺失或用户不一致时自动覆盖写入(exec 默认以普通用户执行,root 用 --sudo)
        if let (Some(user), Some(pwd)) = config::linux_ssh_creds() {
            let needs_sync = match local_conn_info(profile) {
                Ok(info) => {
                    info.addr.is_empty() || (!info.user.is_empty() && info.user != user)
                }
                Err(_) => false,
            };
            if needs_sync {
                ensure_local_conn(profile, &user, &pwd, opts.ssh_port)?;
            }
        }
    }

    let (state, buf, truncated, duration_ms) = run_ws_session(&session, &composed, &token, opts)?;
    match state {
        EndState::Completed(code) => {
            let code = code.clamp(0, 255);
            let (result, tail_truncated) =
                build_result(opts, &buf, &token, truncated, None, code, duration_ms);
            Ok(ExecOutcome::Done(result, tail_truncated))
        }
        EndState::TimedOut => {
            let (result, tail_truncated) = build_result(
                opts,
                &buf,
                &token,
                truncated,
                Some(format!(
                    "超时({}s)未见完成哨兵,已返回收集到的输出",
                    opts.timeout
                )),
                124,
                duration_ms,
            );
            Ok(ExecOutcome::Done(result, tail_truncated))
        }
        EndState::ServerClosed(reason) => {
            let err = explain_server_close(profile, &reason);
            let (result, _) = build_result(
                opts,
                &buf,
                &token,
                truncated,
                Some(format!("{err:#}")),
                1,
                duration_ms,
            );
            Ok(ExecOutcome::Failed(result, err))
        }
        EndState::Disconnected(what) => {
            let err = anyhow!("{what}(shell 可能已退出或面板 agent 重启),命令可能未执行完成");
            let (result, _) = build_result(
                opts,
                &buf,
                &token,
                truncated,
                Some(format!("{err:#}")),
                1,
                duration_ms,
            );
            Ok(ExecOutcome::Failed(result, err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_session(url: &str) -> Session {
        Session {
            profile: "default".into(),
            panel_url: url.into(),
            entrance: String::new(),
            username: "admin".into(),
            panel_name: String::new(),
            logged_in_at: String::new(),
            insecure: false,
            cookies: vec![],
        }
    }

    #[test]
    fn test_ws_url_schemes_and_ports() {
        let s = test_session("http://10.0.0.1:39870");
        let url = ws_url(&s, "echo hi", 120, 40).unwrap();
        assert!(url.starts_with("ws://10.0.0.1:39870/api/v2/hosts/terminal/local?"));
        assert!(url.contains("command=echo%20hi&cols=120&rows=40"), "{url}");

        let s = test_session("https://panel.example.com");
        let url = ws_url(&s, "true", 80, 24).unwrap();
        assert!(url.starts_with("wss://panel.example.com/api/v2/hosts/terminal/local?"));
        assert!(url.contains("cols=80&rows=24"), "{url}");

        let s = test_session("http://127.0.0.1/");
        let url = ws_url(&s, "ls -la /var/www", 120, 40).unwrap();
        assert!(url.contains("command=ls%20%2Dla%20%2Fvar%2Fwww"), "{url}");

        assert!(ws_url(&s, "true", 0, 40).is_err());
    }

    #[test]
    fn test_ws_msg_parse_and_base64() {
        let msg: WsMsg = serde_json::from_str(r#"{"type":"cmd","data":"aGVsbG8="}"#).unwrap();
        assert_eq!(msg.ws_type, "cmd");
        assert_eq!(BASE64.decode(msg.data.as_bytes()).unwrap(), b"hello");

        let msg: WsMsg = serde_json::from_str(r#"{"type":"heartbeat","timestamp":123}"#).unwrap();
        assert_eq!(msg.ws_type, "heartbeat");
        assert!(msg.data.is_empty());

        let msg: WsMsg = serde_json::from_str(r#"{"type":"resize","cols":80,"rows":40}"#).unwrap();
        assert_eq!(msg.ws_type, "resize");
    }

    #[test]
    fn test_build_handshake_headers() {
        let mut s = test_session("http://127.0.0.1:39870");
        s.cookies = vec![
            crate::session::CookieData {
                name: "psession".into(),
                value: "MTczLi4u".into(),
            },
            crate::session::CookieData {
                name: "SecurityEntrance".into(),
                value: "abc".into(),
            },
        ];
        std::env::remove_var(config::ENV_NODE);
        let hs = build_handshake(&s);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].0, "cookie");
        assert_eq!(hs[0].1, "psession=MTczLi4u; SecurityEntrance=abc");
        assert_eq!(hs[1].0, "currentnode");
        assert_eq!(hs[1].1, "local");
    }

    #[test]
    fn test_find_sentinel_single_hit_and_exit_code() {
        let token = new_token();
        assert_eq!(token.len(), 8);
        let sentinel = format!("{SENTINEL_PREFIX}{token}");
        // 回显行(字面量 $?)不参与匹配
        assert_eq!(
            find_sentinel(&format!(" clear &&echo hi; echo {sentinel}_$?"), &token),
            None
        );
        // 真实输出:单次命中即完成
        assert_eq!(
            find_sentinel(&format!("hi\r\n{sentinel}_0\r\n$ "), &token),
            Some(0)
        );
        assert_eq!(
            find_sentinel(&format!("out\r\n{sentinel}_7"), &token),
            Some(7)
        );
        assert_eq!(find_sentinel(&format!("{sentinel}_255"), &token), Some(255));
        // 0 次命中(超时路径)
        assert_eq!(find_sentinel("no sentinel here", &token), None);
        assert_eq!(find_sentinel("", &token), None);
    }

    #[test]
    fn test_find_sentinel_on_wrapped_or_ansi_broken_stream() {
        let token = "ab12cd34";
        let sentinel = format!("{SENTINEL_PREFIX}{token}");
        // PTY 折行把哨兵拦腰截断:归一化(剥离 ANSI + 去掉 \r\n)后仍应命中
        let raw = "hello\r\n\x1b[?2004h__1PCLI_ab12cd3\r\n4_42\r\n[ubuntu@host ~]$ ";
        let normalized = normalize_chunk(raw);
        assert_eq!(find_sentinel(&normalized, token), Some(42));
        // ANSI 序列夹在中间不影响
        let raw2 = format!("{sentinel}_\x1b[0K3");
        assert_eq!(find_sentinel(&normalize_chunk(&raw2), token), Some(3));
        // 超过 3 位数字按贪婪 3 位解析(与文档正则 (\d{1,3}) 语义一致)
        assert_eq!(find_sentinel(&format!("{sentinel}_1234"), token), Some(123));
    }

    #[test]
    fn test_compose_command_basic_and_sentinel() {
        let token = "deadbeef";
        let cmd = compose_command(None, "ls -la", token, false).unwrap();
        assert_eq!(cmd, "( ls -la ); echo __1PCLI_deadbeef_$?");
        let cmd = compose_command(Some("/opt/myapp"), "bash deploy.sh", token, false).unwrap();
        assert_eq!(
            cmd,
            "cd -- '/opt/myapp' && ( bash deploy.sh ); echo __1PCLI_deadbeef_$?"
        );
        // exit/exec 类命令被子 shell 隔离,哨兵仍可执行并透传退出码
        let cmd = compose_command(None, "exit 7", token, false).unwrap();
        assert_eq!(cmd, "( exit 7 ); echo __1PCLI_deadbeef_$?");
    }

    #[test]
    fn test_compose_command_sudo() {
        let token = "deadbeef";
        let cmd = compose_command(None, "systemctl restart nginx", token, true).unwrap();
        assert_eq!(
            cmd,
            "( sudo -n -H bash -c 'systemctl restart nginx' ); echo __1PCLI_deadbeef_$?"
        );
        // --sudo 时 cd 移入 sudo 环境内执行(目录可能仅 root 可读)
        let cmd = compose_command(Some("/opt/a b"), "ls", token, true).unwrap();
        assert_eq!(
            cmd,
            r"( sudo -n -H bash -c 'cd -- '\''/opt/a b'\'' && ls' ); echo __1PCLI_deadbeef_$?"
        );
        // 预校验对 sudo 路径同样生效
        assert!(compose_command(None, "  ", token, true).is_err());
        assert!(compose_command(None, "echo a\necho b", token, true).is_err());
    }

    #[test]
    fn test_compose_command_cwd_quoting_and_validation() {
        let token = "deadbeef";
        let cmd = compose_command(Some("/opt/a b/c"), "true", token, false).unwrap();
        assert!(cmd.starts_with("cd -- '/opt/a b/c' && ( true ); "), "{cmd}");
        let cmd = compose_command(Some("/opt/it's"), "true", token, false).unwrap();
        assert!(
            cmd.starts_with(r"cd -- '/opt/it'\''s' && ( true ); "),
            "{cmd}"
        );
        // 预校验:空命令 / 换行
        assert!(compose_command(None, "  ", token, false).is_err());
        assert!(compose_command(None, "echo a\necho b", token, false).is_err());
        assert!(compose_command(None, "echo a\recho b", token, false).is_err());
        assert!(compose_command(Some("/opt/a\nb"), "true", token, false).is_err());
    }

    #[test]
    fn test_clean_output_strips_echo_banner_and_prompt() {
        let token = "ab12cd34";
        let sentinel = format!("{SENTINEL_PREFIX}{token}");
        let raw = concat!(
            "Last login: Mon Aug 31 00:00:00 2026 from 127.0.0.1\r\n",
            "[ubuntu@host ~]$  clear &&( echo hello && hostname ); echo __1PCLI_ab12cd34_$?\r\n",
            "\x1b[H\x1b[2J\x1b[3J",
            "hello\r\n",
            "myhost\r\n",
            "__1PCLI_ab12cd34_0\r\n",
            "[ubuntu@host ~]$ "
        );
        let cleaned = clean_output(raw, token);
        assert_eq!(cleaned, "hello\nmyhost");

        // --raw 不清洗(由调用方控制,这里只验证清洗与原始不同)
        assert!(raw.contains(&format!("{sentinel}_$?")));
    }

    #[test]
    fn test_clean_output_without_echo_falls_back() {
        let token = "ab12cd34";
        let sentinel = format!("{SENTINEL_PREFIX}{token}");
        // stty -echo 场景:没有回显行,仅哨兵输出行
        let raw = format!("banner\r\nline1\r\n{sentinel}_2\r\n$ ");
        let cleaned = clean_output(&raw, token);
        assert!(cleaned.contains("line1"), "{cleaned}");
        assert!(!cleaned.contains(&sentinel), "{cleaned}");
        assert!(!cleaned.contains("$ "), "{cleaned}");
    }

    #[test]
    fn test_clean_output_keeps_output_containing_clear_and_word() {
        let token = "ab12cd34";
        let sentinel = format!("{SENTINEL_PREFIX}{token}");
        // 命令输出中出现 "clear &&" 字样但不在行首(带前导空格的行首判定不误伤)
        let raw = format!("run: clear &&ls\r\nout\r\n{sentinel}_0\r\n");
        let cleaned = clean_output(&raw, token);
        assert_eq!(cleaned, "run: clear &&ls\nout");
    }

    #[test]
    fn test_tail_lines() {
        let text = "a\nb\nc\nd\ne";
        assert_eq!(tail_lines(text, 2), ("d\ne".to_string(), true));
        assert_eq!(tail_lines(text, 5), (text.to_string(), false));
        assert_eq!(tail_lines(text, 10), (text.to_string(), false));
        assert_eq!(tail_lines(text, 0), (String::new(), true));
        assert_eq!(tail_lines("", 3), (String::new(), false));
    }

    #[test]
    fn test_to_json_shape_and_order() {
        let r = ExecResult {
            exit_code: 7,
            duration_ms: 1234,
            truncated: false,
            error: None,
            output: "hi".into(),
        };
        assert_eq!(
            to_json(&r).unwrap(),
            r#"{"exit_code":7,"duration_ms":1234,"truncated":false,"error":null,"output":"hi"}"#
        );
        let r = ExecResult {
            exit_code: 1,
            duration_ms: 0,
            truncated: true,
            error: Some("boom".into()),
            output: String::new(),
        };
        assert_eq!(
            to_json(&r).unwrap(),
            r#"{"exit_code":1,"duration_ms":0,"truncated":true,"error":"boom","output":""}"#
        );
    }

    #[test]
    fn test_local_conn_body_base64_password() {
        let body = local_conn_body("ubuntu", "s3cret", 22);
        assert_eq!(body["addr"], "127.0.0.1");
        assert_eq!(body["port"], 22);
        assert_eq!(body["user"], "ubuntu");
        assert_eq!(body["authMode"], "password");
        assert_eq!(body["password"].as_str().unwrap(), BASE64.encode(b"s3cret"));
        let body = local_conn_body("u", "p", 2222);
        assert_eq!(body["port"], 2222);
    }

    #[test]
    fn test_strip_ansi_variants() {
        assert_eq!(strip_ansi("\x1b[2J\x1b[Hhello"), "hello");
        assert_eq!(strip_ansi("a\x1b]0;title\x07b"), "ab");
        assert_eq!(strip_ansi("a\x1b]0;title\x1b\\b"), "ab");
        assert_eq!(strip_ansi("\x1b(Bx"), "x");
        assert_eq!(strip_ansi("plain\r\ntext\x07"), "plain\r\ntext");
    }

    #[test]
    fn test_normalize_chunk_removes_line_breaks() {
        assert_eq!(normalize_chunk("a\r\nb\r\nc"), "abc");
        assert_eq!(normalize_chunk("\x1b[31mx\r\ny\x1b[0m"), "xy");
    }
}

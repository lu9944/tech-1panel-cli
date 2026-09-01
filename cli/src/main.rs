use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod apps;
mod auth;
mod catalog;
mod client;
mod commands;
mod config;
mod crypto;
mod db;
mod doctor;
mod exec;
mod firewall;
mod redis;
mod session;
mod web;

use commands::ApiMethod;

#[derive(Parser)]
#[command(
    name = "1panel-cli",
    version,
    about = "登录并调用 1Panel 面板 Web 服务的命令行工具",
    after_help = "示例:\n  login       读取 .env 中的账号信息登录面板\n  status      检查当前登录状态\n  api get core/auth/current  调用面板 API"
)]
struct Cli {
    /// .env 配置文件路径(默认查找当前目录与程序目录下的 .env)
    #[arg(short, long, global = true, value_name = "PATH")]
    env: Option<PathBuf>,

    /// 会话 profile 名称,可同时保存多个面板的登录凭据
    #[arg(
        short,
        long,
        global = true,
        default_value = "default",
        value_name = "NAME"
    )]
    profile: String,

    /// 目标节点标识(多节点面板使用;默认 local,也可设置 PANEL_NODE)
    #[arg(long, global = true, value_name = "NODE")]
    node: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 登录 1Panel 面板并保存会话凭据
    Login {
        /// 面板地址,如 http://10.0.0.1:41394(可包含安全入口路径)
        #[arg(long)]
        url: Option<String>,
        /// 面板安全入口(可从地址路径中自动识别)
        #[arg(long)]
        entrance: Option<String>,
        /// 面板用户名
        #[arg(long)]
        user: Option<String>,
        /// 面板密码
        #[arg(long)]
        password: Option<String>,
        /// MFA 动态验证码(开启 MFA 时需要)
        #[arg(long)]
        mfa: Option<String>,
        /// 验证码答案(面板要求验证码时需要)
        #[arg(long)]
        captcha: Option<String>,
        /// 验证码 ID(与 --captcha 配合使用,用于预先获取的验证码)
        #[arg(long)]
        captcha_id: Option<String>,
        /// 信任自签名证书
        #[arg(long)]
        insecure: bool,
    },
    /// 检查当前登录状态
    Status,
    /// 查看已保存的会话凭据信息
    Info,
    /// 退出登录并清除本地保存的凭据
    Logout,
    /// 使用已保存的凭据调用面板 API
    Api {
        #[command(subcommand)]
        action: ApiAction,
    },
    /// 应用商店操作
    Apps {
        #[command(subcommand)]
        action: AppsAction,
    },
    /// 网站(OpenResty/nginx)操作
    Web {
        #[command(subcommand)]
        action: WebAction,
    },
    /// 数据库操作(MySQL 数据库/用户管理)
    Db {
        #[command(subcommand)]
        action: DbAction,
    },
    /// Redis 实例操作
    Redis {
        #[command(subcommand)]
        action: RedisAction,
    },
    /// 防火墙操作
    Firewall {
        #[command(subcommand)]
        action: FirewallAction,
    },
    /// 检查环境配置是否合格(会尝试登录并给出修复指导)
    #[command(alias = "check")]
    Doctor {
        /// 强制重新登录以验证 .env 中的账号密码
        #[arg(long)]
        force: bool,
    },
    /// 在面板本机执行单行命令(通过终端 WebSocket,非交互)
    Exec {
        /// 要执行的单行命令(不能包含换行符)
        command: String,
        /// 超时秒数(超时返回 124,已收集内容仍会打印)
        #[arg(long, default_value_t = exec::DEFAULT_TIMEOUT_SECS)]
        timeout: i64,
        /// 执行前先切换到指定目录
        #[arg(long)]
        cwd: Option<String>,
        /// PTY 列数(默认 120)
        #[arg(long, default_value_t = exec::DEFAULT_COLS)]
        cols: i64,
        /// PTY 行数(默认 40)
        #[arg(long, default_value_t = exec::DEFAULT_ROWS)]
        rows: i64,
        /// 输出单行结构化 JSON(适合脚本/AI 消费)
        #[arg(long)]
        json: bool,
        /// 仅显示清洗后输出的最后 N 行
        #[arg(long)]
        tail: Option<usize>,
        /// 不做输出清洗(保留 ANSI/回显/提示符/哨兵)
        #[arg(long)]
        raw: bool,
        /// 用 LINUX_SSH_USER / LINUX_SSH_PWD 自动配置面板本地 SSH 连接后执行
        #[arg(long)]
        sync_ssh: bool,
        /// --sync-ssh 时使用的 SSH 端口(默认 22)
        #[arg(long, default_value_t = exec::DEFAULT_SSH_PORT)]
        ssh_port: u16,
        /// 以 root 身份执行(包进 sudo -n -H bash -c;要求 SSH 用户已配置免密 sudo)
        #[arg(long)]
        sudo: bool,
    },
}

#[derive(Subcommand)]
enum ApiAction {
    /// 列出 API 路由(合并内置 dev-v2 清单与当前面板 Swagger)
    List {
        /// 按路径、处理器、摘要或标签过滤
        #[arg(long, default_value = "")]
        filter: String,
    },
    /// 查看指定 API 的定义和请求模型
    Describe {
        /// HTTP 方法,如 GET / POST
        method: String,
        /// API 路径,可省略 /api/v2 前缀
        path: String,
    },
    /// GET 请求,如: get core/auth/current
    Get { path: String },
    /// POST 请求
    Post {
        path: String,
        /// JSON 请求体
        #[arg(short, long)]
        body: Option<String>,
    },
    /// PUT 请求
    Put {
        path: String,
        /// JSON 请求体
        #[arg(short, long)]
        body: Option<String>,
    },
    /// DELETE 请求
    Delete { path: String },
}

#[derive(Subcommand)]
enum AppsAction {
    /// 列出可安装应用
    List {
        /// 页码(默认 1)
        #[arg(long, default_value_t = 1)]
        page: i64,
        /// 每页数量(默认 50)
        #[arg(long, default_value_t = 50)]
        page_size: i64,
        /// 按名称模糊搜索
        #[arg(long)]
        name: Option<String>,
        /// 按应用类型过滤(如 website / runtime / tool)
        #[arg(long)]
        r#type: Option<String>,
        /// 按标签过滤(如 AI / Database / Website)
        #[arg(long)]
        tag: Option<String>,
        /// 只显示已安装的应用
        #[arg(long)]
        installed: bool,
        /// 拉取全部应用
        #[arg(long)]
        all: bool,
    },
    /// 安装指定应用
    Install {
        /// 应用名称(key),如 openresty / mysql / redis
        name: String,
        /// 指定安装版本(默认使用最新版本)
        #[arg(long)]
        version: Option<String>,
        /// 覆盖安装参数,格式 KEY=VALUE,可多次指定
        #[arg(long = "param", value_parser = parse_kv)]
        params: Vec<(String, String)>,
        /// 等待安装完成并显示进度
        #[arg(long)]
        wait: bool,
        /// 等待超时时间(秒,默认 600)
        #[arg(long, default_value_t = 600)]
        timeout: i64,
    },
    /// 列出所有应用分类标签
    Tags,
    /// 查看已安装应用的详细信息(含数据库/Redis 连接信息)
    Info {
        /// 已安装应用的名称,如 mysql / redis
        name: String,
    },
    /// 查看或修改已安装应用的配置参数
    Config {
        /// 已安装应用的名称,如 mysql / redis / openresty
        name: String,
        /// 修改配置项,格式 KEY=VALUE,可多次指定;不带此参数时仅查看
        #[arg(long = "set", value_parser = parse_kv)]
        set: Vec<(String, String)>,
        /// 同时显示 docker-compose 内容
        #[arg(long)]
        compose: bool,
    },
    /// 修改数据库 root 密码(mysql / mariadb / redis)
    Password {
        /// 数据库应用名称,如 mysql / redis
        name: String,
        /// 新密码
        value: String,
    },
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| format!("参数格式应为 KEY=VALUE: {s}"))?;
    Ok((k.trim().to_string(), v.trim().to_string()))
}

#[derive(Subcommand)]
enum WebAction {
    /// 列出所有网站/域名
    List,
    /// 新建网站
    Create {
        /// 域名,如 test3.bitzh.edu.cn
        domain: String,
        /// 网站类型: static / deployment / proxy / runtime / stream(默认 static)
        #[arg(long, default_value = "static")]
        r#type: String,
        /// 网站别名(默认取域名首段)
        #[arg(long)]
        alias: Option<String>,
        /// 备注
        #[arg(long)]
        remark: Option<String>,
    },
    /// 读取或修改指定域名的 nginx 配置
    Config {
        /// 域名,如 test.local
        domain: String,
        /// 用本地文件内容更新 nginx 配置(不带此参数时仅读取)
        #[arg(long)]
        file: Option<String>,
    },
    /// 查看或设置网站 HTTPS
    Https {
        /// 域名,如 test.local
        domain: String,
        /// 开启 HTTPS;搭配 --ssl-id 或 --cert/--key 使用
        #[arg(long)]
        enable: bool,
        /// 关闭 HTTPS
        #[arg(long)]
        disable: bool,
        /// 证书 ID(web ssl 申请成功后返回)
        #[arg(long)]
        ssl_id: Option<u64>,
        /// 证书 PEM 文件(手动导入证书时使用)
        #[arg(long)]
        cert: Option<String>,
        /// 私钥 PEM 文件(手动导入证书时使用)
        #[arg(long)]
        key: Option<String>,
        /// HTTP 处理方式: HTTPSOnly / HTTPToHTTPS / HTTPAlso
        #[arg(long, default_value = "HTTPToHTTPS")]
        http_config: String,
    },
    /// 申请 SSL 证书(Let's Encrypt)
    Ssl {
        /// 域名,如 test.local
        domain: String,
        /// ACME 账号邮箱
        #[arg(long)]
        email: String,
        /// 验证方式: http / dnsAccount(默认 http)
        #[arg(long, default_value = "http")]
        provider: String,
        /// 密钥类型(默认 RSA2048)
        #[arg(long, default_value = "RSA2048")]
        key_type: String,
        /// 自动续期(默认开启)
        #[arg(long)]
        no_auto_renew: bool,
    },
    /// 上传本地文件到网站目录
    Upload {
        /// 域名,如 test.local
        domain: String,
        /// 本地文件路径
        file: String,
        /// 网站目录下的子目录(默认网站根目录)
        #[arg(long, default_value = "")]
        to: String,
    },
    /// 解压网站目录中的压缩包
    Extract {
        /// 域名,如 test.local
        domain: String,
        /// 压缩包路径(相对于网站目录,或以 / 开头的绝对路径)
        archive: String,
        /// 解压目标子目录(默认网站根目录)
        #[arg(long, default_value = "")]
        to: String,
        /// 压缩类型(默认按扩展名识别): zip / tar.gz / tar / gz ...
        #[arg(long, default_value = "")]
        r#type: String,
    },
}

#[derive(Subcommand)]
enum DbAction {
    /// 列出数据库/实例
    List {
        /// 类型: mysql / redis(默认 mysql)
        #[arg(long, default_value = "mysql")]
        r#type: String,
        /// 数据库实例名(默认与 --type 相同)
        #[arg(long)]
        instance: Option<String>,
    },
    /// 创建数据库(MySQL)
    Create {
        /// 数据库名
        name: String,
        /// 同时创建的用户名(可选)
        #[arg(long)]
        user: Option<String>,
        /// 用户密码(可选)
        #[arg(long)]
        password: Option<String>,
        /// MySQL 实例名(默认 mysql)
        #[arg(long, default_value = "mysql")]
        instance: String,
        /// 字符集(默认 utf8mb4)
        #[arg(long, default_value = "utf8mb4")]
        format: String,
        /// 用户访问权限(默认 %)
        #[arg(long, default_value = "%")]
        permission: String,
        /// 备注
        #[arg(long)]
        remark: Option<String>,
    },
    /// 删除数据库(MySQL)
    Delete {
        /// 数据库名
        name: String,
        /// 强制删除(忽略被引用资源)
        #[arg(long)]
        force: bool,
        /// 数据库类型: mysql / mariadb
        #[arg(long, default_value = "mysql")]
        r#type: String,
        /// 数据库实例名(默认与 --type 相同)
        #[arg(long)]
        instance: Option<String>,
    },
    /// 列出数据库用户
    Users {
        /// MySQL 实例名(默认 mysql)
        #[arg(long, default_value = "mysql")]
        instance: String,
    },
    /// 用户管理
    User {
        #[command(subcommand)]
        action: DbUserAction,
    },
    /// 修改数据库 root 密码(MySQL)
    Passwd {
        /// 新密码
        password: String,
        /// MySQL 实例名(默认 mysql)
        #[arg(long, default_value = "mysql")]
        instance: String,
    },
}

#[derive(Subcommand)]
enum RedisAction {
    /// 列出 Redis 实例
    List,
    /// 查看 Redis 实例状态与配置
    Info {
        /// Redis 实例名(默认 redis)
        name: String,
    },
    /// 读取或修改 Redis 配置
    Config {
        /// Redis 实例名(默认 redis)
        name: String,
        /// 修改配置项,格式 KEY=VALUE(timeout/maxclients/maxmemory),可多次指定
        #[arg(long = "set", value_parser = parse_kv)]
        set: Vec<(String, String)>,
    },
    /// 修改 Redis 密码
    Passwd {
        /// Redis 实例名(默认 redis)
        name: String,
        /// 新密码
        password: String,
    },
}

#[derive(Subcommand)]
enum FirewallAction {
    /// 查看防火墙状态
    Status,
    /// 启动防火墙
    Start,
    /// 停止防火墙
    Stop,
    /// 重启防火墙
    Restart,
    /// 允许 Ping
    AllowPing,
    /// 禁止 Ping
    BanPing,
    /// 列出防火墙规则
    List {
        /// 规则类型: port / address(默认 port)
        #[arg(long, default_value = "port")]
        r#type: String,
        /// 按端口/地址关键字过滤
        #[arg(long, default_value = "")]
        info: String,
        /// 按策略过滤: accept / drop
        #[arg(long, default_value = "")]
        strategy: String,
    },
    /// 端口规则
    Port {
        #[command(subcommand)]
        action: PortAction,
    },
    /// IP 规则
    Ip {
        #[command(subcommand)]
        action: IpAction,
    },
    /// 端口转发
    Forward {
        #[command(subcommand)]
        action: ForwardAction,
    },
    /// Docker 端口守护(新版统一防火墙 API)
    Docker {
        #[command(subcommand)]
        action: DockerAction,
    },
    /// 批量添加端口放行
    Batch {
        /// 添加多个端口,逗号分隔,如 8080,9090
        ports: String,
        /// 协议: tcp / udp / tcp/udp(默认 tcp)
        #[arg(long, default_value = "tcp")]
        protocol: String,
        /// 策略: accept / drop(默认 accept)
        #[arg(long, default_value = "accept")]
        strategy: String,
        /// 来源地址(默认 Anywhere)
        #[arg(long, default_value = "")]
        source: String,
        /// 备注
        #[arg(long, default_value = "")]
        desc: String,
    },
}

#[derive(Subcommand)]
enum DockerAction {
    /// 查看 Docker 端口守护状态(后端/绑定/初始化)
    Status,
    /// 列出 Docker 已发布端口与防护策略
    Ports,
    /// 同步 Docker 端口防护规则
    Sync,
    /// 初始化/绑定/解绑 Docker 端口守护
    Operate {
        /// 操作: init / bind / unbind(默认 init)
        #[arg(long, default_value = "init")]
        operation: String,
    },
    /// 放行已发布端口(仅允许指定来源访问)
    Allow {
        /// 宿主端口,如 8080 或 0.0.0.0:8080
        port: String,
        /// 协议: tcp / udp / tcp/udp(默认 tcp)
        #[arg(long, default_value = "tcp")]
        protocol: String,
        /// 允许访问的来源 IP(逗号分隔,必填)
        #[arg(long, default_value = "")]
        sources: String,
        /// 备注
        #[arg(long, default_value = "")]
        desc: String,
    },
    /// 拒绝指定来源访问已发布端口(不带 --sources 时拒绝所有来源)
    Deny {
        /// 宿主端口,如 8080 或 0.0.0.0:8080
        port: String,
        /// 协议: tcp / udp / tcp/udp(默认 tcp)
        #[arg(long, default_value = "tcp")]
        protocol: String,
        /// 拒绝访问的来源 IP(逗号分隔;为空则拒绝所有来源)
        #[arg(long, default_value = "")]
        sources: String,
        /// 备注
        #[arg(long, default_value = "")]
        desc: String,
    },
    /// 删除 Docker 端口守护策略
    PolicyDel {
        /// 策略 UUID(firewall docker ports 中查看)
        #[arg(long)]
        uuid: String,
    },
}

#[derive(Subcommand)]
enum PortAction {
    /// 添加端口规则
    Add {
        /// 端口,如 8080 或 8080-8090
        port: String,
        /// 协议: tcp / udp / tcp/udp(默认 tcp)
        #[arg(long, default_value = "tcp")]
        protocol: String,
        /// 策略: accept / drop(默认 accept)
        #[arg(long, default_value = "accept")]
        strategy: String,
        /// 来源地址(默认 Anywhere)
        #[arg(long, default_value = "")]
        source: String,
        /// 备注
        #[arg(long, default_value = "")]
        desc: String,
    },
    /// 删除端口规则
    Del {
        /// 端口
        port: String,
        /// 协议(需与添加时一致,默认 tcp)
        #[arg(long, default_value = "tcp")]
        protocol: String,
        /// 策略(需与添加时一致,默认 accept)
        #[arg(long, default_value = "accept")]
        strategy: String,
        /// 来源地址(默认 Anywhere)
        #[arg(long, default_value = "")]
        source: String,
    },
}

#[derive(Subcommand)]
enum IpAction {
    /// 添加 IP 规则
    Add {
        /// IP 或网段,如 1.2.3.4 或 1.2.3.0/24
        address: String,
        /// 策略: accept / drop(默认 drop)
        #[arg(long, default_value = "drop")]
        strategy: String,
        /// 备注
        #[arg(long, default_value = "")]
        desc: String,
    },
    /// 删除 IP 规则
    Del {
        /// IP 或网段
        address: String,
        /// 策略(默认 drop)
        #[arg(long, default_value = "drop")]
        strategy: String,
    },
}

#[derive(Subcommand)]
enum ForwardAction {
    /// 添加端口转发
    Add {
        /// 监听端口
        port: String,
        /// 转发目标,格式 IP:端口,如 10.0.0.2:8080
        #[arg(long)]
        to: String,
        /// 协议: tcp / udp / tcp/udp(默认 tcp)
        #[arg(long, default_value = "tcp")]
        protocol: String,
        /// 网卡(可选)
        #[arg(long, default_value = "")]
        interface: String,
    },
    /// 删除端口转发
    Del {
        /// 监听端口
        port: String,
        /// 转发目标,格式 IP:端口
        #[arg(long)]
        to: String,
        /// 协议(默认 tcp)
        #[arg(long, default_value = "tcp")]
        protocol: String,
        /// 网卡(可选)
        #[arg(long, default_value = "")]
        interface: String,
        /// 规则编号(firewall list --type forward 中的编号)
        #[arg(long, default_value = "")]
        num: String,
    },
}

#[derive(Subcommand)]
enum DbUserAction {
    /// 创建用户
    Add {
        /// 用户名
        username: String,
        /// 密码
        password: String,
        /// 允许的主机(默认 %)
        #[arg(long, default_value = "%")]
        host: String,
        /// 授权的数据库(逗号分隔,可选)
        #[arg(long, default_value = "")]
        dbs: String,
        /// 备注
        #[arg(long)]
        remark: Option<String>,
        /// MySQL 实例名(默认 mysql)
        #[arg(long, default_value = "mysql")]
        instance: String,
    },
    /// 删除用户
    Del {
        /// 用户名
        username: String,
        /// 主机(默认 %)
        #[arg(long, default_value = "%")]
        host: String,
        /// MySQL 实例名(默认 mysql)
        #[arg(long, default_value = "mysql")]
        instance: String,
    },
    /// 修改用户密码
    Passwd {
        /// 用户名
        username: String,
        /// 新密码
        password: String,
        /// 主机(默认 %)
        #[arg(long, default_value = "%")]
        host: String,
        /// MySQL 实例名(默认 mysql)
        #[arg(long, default_value = "mysql")]
        instance: String,
    },
}

fn main() -> Result<()> {
    // 避免管道输出被提前关闭(如 | head)时出现 Broken pipe panic
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    config::load_env_file(cli.env.as_deref())?;
    config::set_node_override(cli.node.as_deref());

    match cli.command {
        Commands::Login {
            url,
            entrance,
            user,
            password,
            mfa,
            captcha,
            captcha_id,
            insecure,
        } => {
            let overrides = config::ConfigOverrides {
                url,
                entrance,
                username: user,
                password,
                mfa_code: mfa,
                captcha,
                captcha_id,
                insecure: Some(insecure),
                language: None,
                linux_ssh_user: None,
                linux_ssh_pwd: None,
            };
            let cfg = config::PanelConfig::from_env(&overrides)?;
            auth::login(&cfg, &cli.profile)?;
        }
        Commands::Status => commands::status(&cli.profile)?,
        Commands::Info => commands::info(&cli.profile)?,
        Commands::Logout => commands::logout(&cli.profile)?,
        Commands::Api { action } => match action {
            ApiAction::List { filter } => catalog::list(&cli.profile, &filter)?,
            ApiAction::Describe { method, path } => {
                catalog::describe(&cli.profile, &method, &path)?
            }
            ApiAction::Get { path } => {
                commands::api_call(&cli.profile, ApiMethod::Get, &path, None)?
            }
            ApiAction::Post { path, body } => {
                commands::api_call(&cli.profile, ApiMethod::Post, &path, body.as_deref())?
            }
            ApiAction::Put { path, body } => {
                commands::api_call(&cli.profile, ApiMethod::Put, &path, body.as_deref())?
            }
            ApiAction::Delete { path } => {
                commands::api_call(&cli.profile, ApiMethod::Delete, &path, None)?
            }
        },
        Commands::Apps { action } => match action {
            AppsAction::List {
                page,
                page_size,
                name,
                r#type,
                tag,
                installed,
                all,
            } => {
                let q = apps::AppsQuery {
                    page,
                    page_size,
                    name,
                    r#type,
                    tag,
                    installed,
                    all,
                };
                apps::list(&cli.profile, &q)?;
            }
            AppsAction::Tags => apps::tags(&cli.profile)?,
            AppsAction::Info { name } => apps::info(&cli.profile, &name)?,
            AppsAction::Config { name, set, compose } => {
                let opts = apps::ConfigOptions {
                    set,
                    show_compose: compose,
                };
                apps::config(&cli.profile, &name, &opts)?;
            }
            AppsAction::Password { name, value } => apps::password(&cli.profile, &name, &value)?,
            AppsAction::Install {
                name,
                version,
                params,
                wait,
                timeout,
            } => {
                let opts = apps::InstallOptions {
                    version,
                    params,
                    wait,
                    timeout,
                };
                apps::install(&cli.profile, &name, &opts)?;
            }
        },
        Commands::Web { action } => match action {
            WebAction::List => web::list(&cli.profile)?,
            WebAction::Create {
                domain,
                r#type,
                alias,
                remark,
            } => {
                let opts = web::CreateOptions {
                    r#type,
                    alias,
                    remark,
                };
                web::create(&cli.profile, &domain, &opts)?;
            }
            WebAction::Config { domain, file } => {
                let opts = web::ConfigOptions { file };
                web::config(&cli.profile, &domain, &opts)?;
            }
            WebAction::Https {
                domain,
                enable,
                disable,
                ssl_id,
                cert,
                key,
                http_config,
            } => {
                let opts = web::HttpsOptions {
                    enable: if disable {
                        Some(false)
                    } else if enable || ssl_id.is_some() || cert.is_some() {
                        Some(true)
                    } else {
                        None
                    },
                    ssl_id,
                    http_config: Some(http_config),
                    cert_file: cert,
                    key_file: key,
                };
                web::https(&cli.profile, &domain, &opts)?;
            }
            WebAction::Ssl {
                domain,
                email,
                provider,
                key_type,
                no_auto_renew,
            } => {
                let opts = web::SslOptions {
                    email,
                    provider,
                    key_type,
                    auto_renew: !no_auto_renew,
                };
                web::ssl(&cli.profile, &domain, &opts)?;
            }
            WebAction::Upload { domain, file, to } => {
                web::upload(&cli.profile, &domain, &file, &to)?;
            }
            WebAction::Extract {
                domain,
                archive,
                to,
                r#type,
            } => web::extract(&cli.profile, &domain, &archive, &to, &r#type)?,
        },
        Commands::Db { action } => match action {
            DbAction::List { r#type, instance } => {
                db::list(&cli.profile, &r#type, instance.as_deref())?
            }
            DbAction::Create {
                name,
                user,
                password,
                instance,
                format,
                permission,
                remark,
            } => {
                let opts = db::CreateOptions {
                    user,
                    password,
                    instance,
                    format,
                    permission,
                    remark,
                };
                db::create(&cli.profile, &name, &opts)?;
            }
            DbAction::Delete {
                name,
                force,
                r#type,
                instance,
            } => {
                let opts = db::DeleteOptions {
                    instance: instance.unwrap_or_else(|| r#type.clone()),
                    db_type: r#type,
                    force,
                };
                db::delete(&cli.profile, &name, &opts)?;
            }
            DbAction::Users { instance } => db::users(&cli.profile, &instance)?,
            DbAction::User { action } => match action {
                DbUserAction::Add {
                    username,
                    password,
                    host,
                    dbs,
                    remark,
                    instance,
                } => {
                    let opts = db::UserOptions {
                        instance,
                        host,
                        dbs: dbs
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect(),
                        remark,
                    };
                    db::user_add(&cli.profile, &username, &password, &opts)?;
                }
                DbUserAction::Del {
                    username,
                    host,
                    instance,
                } => {
                    let opts = db::UserOptions {
                        instance,
                        host,
                        dbs: vec![],
                        remark: None,
                    };
                    db::user_del(&cli.profile, &username, &opts)?;
                }
                DbUserAction::Passwd {
                    username,
                    password,
                    host,
                    instance,
                } => {
                    let opts = db::UserOptions {
                        instance,
                        host,
                        dbs: vec![],
                        remark: None,
                    };
                    db::user_passwd(&cli.profile, &username, &password, &opts)?;
                }
            },
            DbAction::Passwd { instance, password } => {
                db::root_passwd(&cli.profile, &instance, &password)?;
            }
        },
        Commands::Redis { action } => match action {
            RedisAction::List => redis::list(&cli.profile)?,
            RedisAction::Info { name } => redis::info(&cli.profile, &name)?,
            RedisAction::Config { name, set } => {
                let opts = redis::ConfigOptions { set };
                redis::config(&cli.profile, &name, &opts)?;
            }
            RedisAction::Passwd { name, password } => {
                redis::passwd(&cli.profile, &name, &password)?;
            }
        },
        Commands::Firewall { action } => match action {
            FirewallAction::Status => firewall::status(&cli.profile)?,
            FirewallAction::Start => firewall::operate(&cli.profile, "start")?,
            FirewallAction::Stop => firewall::operate(&cli.profile, "stop")?,
            FirewallAction::Restart => firewall::operate(&cli.profile, "restart")?,
            FirewallAction::AllowPing => firewall::operate(&cli.profile, "allow-ping")?,
            FirewallAction::BanPing => firewall::operate(&cli.profile, "ban-ping")?,
            FirewallAction::List {
                r#type,
                info,
                strategy,
            } => {
                let opts = firewall::ListOptions {
                    r#type,
                    info,
                    strategy,
                };
                firewall::list(&cli.profile, &opts)?;
            }
            FirewallAction::Port { action } => match action {
                PortAction::Add {
                    port,
                    protocol,
                    strategy,
                    source,
                    desc,
                } => {
                    let opts = firewall::PortOptions {
                        protocol,
                        strategy,
                        source,
                        desc,
                    };
                    firewall::port_add(&cli.profile, &port, &opts)?;
                }
                PortAction::Del {
                    port,
                    protocol,
                    strategy,
                    source,
                } => {
                    let opts = firewall::PortOptions {
                        protocol,
                        strategy,
                        source,
                        desc: String::new(),
                    };
                    firewall::port_del(&cli.profile, &port, &opts)?;
                }
            },
            FirewallAction::Ip { action } => match action {
                IpAction::Add {
                    address,
                    strategy,
                    desc,
                } => {
                    let opts = firewall::IpOptions { strategy, desc };
                    firewall::ip_add(&cli.profile, &address, &opts)?;
                }
                IpAction::Del { address, strategy } => {
                    let opts = firewall::IpOptions {
                        strategy,
                        desc: String::new(),
                    };
                    firewall::ip_del(&cli.profile, &address, &opts)?;
                }
            },
            FirewallAction::Forward { action } => match action {
                ForwardAction::Add {
                    port,
                    to,
                    protocol,
                    interface,
                } => {
                    let opts = firewall::ForwardOptions {
                        protocol,
                        target: to,
                        interface,
                        num: String::new(),
                    };
                    firewall::forward_add(&cli.profile, &port, &opts)?;
                }
                ForwardAction::Del {
                    port,
                    to,
                    protocol,
                    interface,
                    num,
                } => {
                    let opts = firewall::ForwardOptions {
                        protocol,
                        target: to,
                        interface,
                        num,
                    };
                    firewall::forward_del(&cli.profile, &port, &opts)?;
                }
            },
            FirewallAction::Docker { action } => match action {
                DockerAction::Status => firewall::docker_status(&cli.profile)?,
                DockerAction::Ports => firewall::docker_ports(&cli.profile)?,
                DockerAction::Sync => firewall::docker_sync(&cli.profile)?,
                DockerAction::Operate { operation } => {
                    firewall::docker_operate(&cli.profile, &operation)?
                }
                DockerAction::Allow {
                    port,
                    protocol,
                    sources,
                    desc,
                } => firewall::docker_apply(
                    &cli.profile,
                    &port,
                    &protocol,
                    &sources,
                    &desc,
                    "allow_sources",
                )?,
                DockerAction::Deny {
                    port,
                    protocol,
                    sources,
                    desc,
                } => {
                    let mode = if sources.trim().is_empty() {
                        "deny_all"
                    } else {
                        "deny_sources"
                    };
                    firewall::docker_apply(&cli.profile, &port, &protocol, &sources, &desc, mode)?;
                }
                DockerAction::PolicyDel { uuid } => {
                    firewall::docker_policy_del(&cli.profile, &uuid)?
                }
            },
            FirewallAction::Batch {
                ports,
                protocol,
                strategy,
                source,
                desc,
            } => {
                let opts = firewall::PortOptions {
                    protocol,
                    strategy,
                    source,
                    desc,
                };
                firewall::batch_add(&cli.profile, &ports, &opts)?;
            }
        },
        Commands::Doctor { force } => doctor::doctor(&cli.profile, force)?,
        Commands::Exec {
            command,
            timeout,
            cwd,
            cols,
            rows,
            json,
            tail,
            raw,
            sync_ssh,
            ssh_port,
            sudo,
        } => {
            let opts = exec::ExecOptions {
                command,
                timeout,
                cwd,
                cols,
                rows,
                json,
                tail,
                raw,
                sync_ssh,
                ssh_port,
                sudo,
            };
            std::process::exit(exec::run(&cli.profile, &opts)?);
        }
    }

    Ok(())
}

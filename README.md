# tech-1panel-cli v0.3.0 — 1Panel v2 面板管理 Skill + CLI

通过命令行工具 `1panel-cli` 管理 [1Panel](https://1panel.cn) 面板。所有操作均通过面板官方 API 完成,效果与面板管理页面一致。

当前验证基线为 **1Panel dev-v2**(下一版,统一防火墙 API + Docker 端口守护),同时兼容 **v2.2.5** 及更早稳定版旧防火墙接口。详细审计见 [`references/compatibility-dev-v2.md`](references/compatibility-dev-v2.md)。

项目同时提供:

- **Agent Skill**:符合 [Agent Skills](https://opencode.ai/docs/skills/) 标准的 `SKILL.md`,可直接用于 opencode / Claude Code 等工具,让 AI 通过 CLI 安全地完成面板运维(登录、装应用、建站、防火墙、数据库、Redis 等)。
- **Rust CLI**:`1panel-cli` 可执行文件,由纯 Rust 实现,单文件无运行时依赖。
- **自动发布**:GitHub Actions 构建 Linux x86_64 / aarch64 二进制,打包为 `1panel-skill.zip` 发布到 Releases。

## 功能

- **会话**:`login / status / info / logout / doctor`,自动完成 RSA+AES 密码加密、安全入口、验证码与 MFA;登录一次后会话凭据保存在本机(`~/.config/1panel-cli/<profile>.json`,权限 0600),无需重复登录
- **应用商店 `apps`**:列出/安装应用、查看应用详情(含数据库与 Redis 连接信息)、修改配置参数与数据库 root 密码
- **数据库 `db`**:MySQL/MariaDB 数据库/用户增删改查、root 密码
- **Redis `redis`**:实例状态/配置读写/密码
- **网站 `web`**:网站列表、nginx 配置读写(保存后自动 reload)、HTTPS/SSL 证书、文件上传与解压
- **防火墙 `firewall`**:自动适配新版统一规则接口与 v2.2.5 旧接口,支持状态/启停/Ping、端口/IP/转发规则增删查、批量放行;新版面板额外支持 Docker 端口守护(`firewall docker` 状态/列表/同步/放行/拒绝)
- **API `api`**:从官方源码生成 766 条路由目录,可 list/describe 并调用 GET/POST/PUT/DELETE,自动携带 Cookie、CSRF、语言和节点头

## 安装

### 方式一:从 Releases 下载(推荐)

1. 打开 [Releases](https://github.com/lu9944/tech-1panel-cli/releases),按服务器架构下载:
   - `1panel-skill.zip` — Linux x86_64
   - `1panel-skill-aarch64.zip` — Linux aarch64
2. 解压并验证:

   ```sh
   unzip 1panel-skill.zip && cd 1panel-skill
   ./bin/1panel-cli --version
   ```

### 方式二:源码构建

```sh
git clone https://github.com/lu9944/tech-1panel-cli.git
cd tech-1panel-cli/cli
cargo build --release --locked
# 产物: cli/target/release/1panel-cli
```

### 作为 Skill 安装(opencode / Claude Code)

将解压后的 `SKILL.md` 所在目录放入 skill 路径,例如:

```sh
mkdir -p ~/.config/opencode/skills
cp -r 1panel-skill ~/.config/opencode/skills/1panel-cli
```

之后让 AI 工具加载 `1panel-cli` skill,它会在执行面板操作前自动运行 `1panel-cli doctor` 检查环境。

## 快速开始

1. **配置账号**:复制 `.env.example` 为 `.env` 并填写面板信息(面板地址、用户名、密码):

   ```sh
   cp .env.example .env
   vim .env
   ```

2. **环境检查**(建议每次操作前先执行):

   ```sh
   ./bin/1panel-cli doctor
   ```

3. **登录**:

   ```sh
   ./bin/1panel-cli login
   ```

4. **常用命令**:

   ```sh
   1panel-cli status                    # 检查登录状态
   1panel-cli apps list --installed     # 已安装应用
   1panel-cli apps install mysql --wait # 安装 MySQL(等待完成)
   1panel-cli apps info mysql           # 应用详情 + 数据库连接信息
   1panel-cli db list                   # 列出数据库
   1panel-cli redis info redis          # Redis 状态与配置
   1panel-cli web list                  # 列出网站/域名
   1panel-cli firewall status           # 防火墙状态
   1panel-cli api get core/auth/current # 调用面板任意 API
   1panel-cli api list --filter templates # 发现源码注册的 API
   ```

完整命令速查见同目录 `commands.md`,CLI 详细用法与示例见 `cli/README.md`。

## 配置说明(.env)

| 变量 | 必填 | 说明 |
|---|---|---|
| `PANEL_URL` | 是 | 面板地址,可包含安全入口路径,如 `http://10.0.0.1:41394/abcdef1234` |
| `PANEL_USERNAME` | 是 | 面板登录用户名 |
| `PANEL_PASSWORD` | 是 | 面板登录密码 |
| `PANEL_ENTRANCE` | 否 | 安全入口(也可从 `PANEL_URL` 路径自动识别) |
| `PANEL_LANGUAGE` | 否 | 请求语言,默认 `zh` |
| `PANEL_NODE` | 否 | 多节点目标节点,默认 `local`;也可用 `--node` |
| `PANEL_MFA_CODE` | 否 | 开启 MFA 时的动态验证码(自动化场景) |
| `PANEL_CAPTCHA` / `PANEL_CAPTCHA_ID` | 否 | 登录验证码答案与 ID(配合外部 OCR 自动化) |
| `PANEL_INSECURE` | 否 | 面板为 HTTPS 且使用自签名证书时设为 `true` |

CLI 启动时依次从当前目录、可执行文件所在目录查找 `.env`,也可用 `--env <path>` 指定,或通过 `--profile <name>` 保存多套配置;同时支持环境变量与命令行参数。配置优先级:**命令行参数 > 环境变量 > .env 文件**。

## 安全说明

- `.env` 包含面板登录密码,**请勿提交到版本库**(仓库已通过 `.gitignore` 忽略)。
- 登录会话凭据保存在 `~/.config/1panel-cli/<profile>.json`(权限 0600),请勿泄露给他人。
- 密码等敏感信息出现在命令行参数时,注意日志与 shell 历史脱敏。

## 仓库结构

```
tech-1panel-cli/
├── SKILL.md               # 技能主体(触发条件、环境检查、命令速查、关键约定)
├── commands.md            # 完整命令速查表(会话 / apps / db / redis / web / firewall / api)
├── .env.example           # 面板账号配置模板
├── references/            # dev-v2 API 清单与兼容性审计
├── scripts/               # API 清单生成器
├── cli/                   # 1panel-cli Rust 源码
│   ├── src/               #   源码模块
│   ├── Cargo.toml
│   ├── README.md          #   CLI 使用文档
│   └── .env.example
└── .github/workflows/
    └── release.yml        # 构建二进制 → 打包 1panel-skill.zip → 发布到 Releases
```

## 发布产物

GitHub Actions(打 `v*` tag 或手动触发)会构建 Linux x86_64 / aarch64 两种二进制,并打包为符合规范结构的 `1panel-skill.zip`:

```
1panel-skill.zip
└── 1panel-skill/
    ├── SKILL.md
    ├── commands.md
    ├── README.md
    ├── .env.example       # 面板账号配置模板(根目录副本)
    ├── references/
    │   ├── compatibility-dev-v2.md
    │   └── api-dev-v2.json
    └── bin/
        ├── .env.example   # 配置模板(复制为 .env 并填写,与二进制同目录)
        └── 1panel-cli     # 可执行文件
```

## License

MIT

---
name: 1panel-cli
description: >-
  管理 1Panel v2 面板的命令行工具(1panel-cli)。当用户提到 1Panel / 面板 / 宝塔
  类似的服务器管理面板,或需要登录面板、安装/管理应用(openresty、mysql、redis、
  phpmyadmin 等)、查看应用连接信息、修改数据库密码、创建/管理网站与域名、读写
  nginx 配置、配置 HTTPS/SSL、上传解压网站文件、管理 MySQL/MariaDB 数据库与用户、
  管理 Redis、防火墙端口/IP/转发规则,或发现和调用面板 API 时使用本 skill。执行前先
  运行 `1panel-cli doctor`;原始 API 调用前先用 `api list` / `api describe` 查询,不要猜测路径。
compatibility: opencode
metadata:
  tool: 1panel-cli
  panel: 1Panel
  verified-panel-version: dev-v2
---

# 1panel-cli — 1Panel v2 面板管理

通过 1Panel 官方 API 管理面板。当前验证基线是 1Panel dev-v2(下一版,统一防火墙 API);同时兼容 v2.2.5 及更早稳定版的旧防火墙接口。版本差异见 `references/compatibility-dev-v2.md`。

## 环境

按顺序探测二进制,使用第一个存在的路径:

1. `<本 skill 目录>/bin/1panel-cli`
2. `<仓库>/cli/target/release/1panel-cli`
3. `PATH` 中的 `1panel-cli`

把 `.env.example` 复制为 `.env`,至少填写 `PANEL_URL`、`PANEL_USERNAME`、`PANEL_PASSWORD`;启用安全入口时填写 `PANEL_ENTRANCE`。多节点面板可设置 `PANEL_NODE` 或传全局参数 `--node`。会话保存在 `~/.config/1panel-cli/<profile>.json`。

## 操作流程

任何面板操作前先执行:

```sh
1panel-cli doctor
```

仅在六项检查全部通过后继续。优先使用 `apps`、`db`、`redis`、`web`、`firewall` 等专用命令。专用命令无法覆盖时:

```sh
1panel-cli api list --filter template
1panel-cli api describe POST websites/templates/search
1panel-cli api post websites/templates/search --body '{"page":1,"pageSize":20}'
```

`api list` 基于官方 dev-v2 源码生成的 766 条路由清单,并尝试读取当前面板 Swagger;它能显示 Swagger 未收录但源码已注册的路由。

## 常用命令

```sh
# 会话与版本
1panel-cli login
1panel-cli status
1panel-cli doctor --force
1panel-cli --profile prod --node local status

# 应用
1panel-cli apps list --installed
1panel-cli apps install mysql --wait
1panel-cli apps info mysql
1panel-cli apps config openresty
1panel-cli apps password mysql '新密码'

# MySQL / MariaDB
1panel-cli db list
1panel-cli db list --type mariadb --instance mariadb
1panel-cli db create appdb --user app --password 'App@123'
1panel-cli db delete appdb --type mariadb --instance mariadb
1panel-cli db users --instance mysql

# Redis
1panel-cli redis info redis
1panel-cli redis config redis --set maxmemory=268435456
1panel-cli redis passwd redis '新密码'

# 网站
1panel-cli web list
1panel-cli web create example.com --type static
1panel-cli web config example.com
1panel-cli web config example.com --file ./nginx.conf
1panel-cli web https example.com --enable --ssl-id <ID>
1panel-cli web ssl example.com --email admin@example.com
1panel-cli web upload example.com ./site.zip --to index
1panel-cli web extract example.com index/site.zip --to index

# 防火墙(自动探测稳定版旧 API 或新版统一 API)
1panel-cli firewall status
1panel-cli firewall port add 8080 --protocol tcp
1panel-cli firewall port del 8080
1panel-cli firewall ip add 1.2.3.4 --strategy drop
1panel-cli firewall forward add 18080 --to 10.0.0.2:8080
1panel-cli firewall list --type forward

# 防火墙 Docker 端口守护(新版统一 API)
1panel-cli firewall docker status
1panel-cli firewall docker ports
1panel-cli firewall docker allow 8080 --sources 1.2.3.4 --desc web
1panel-cli firewall docker deny 0.0.0.0:8080 --desc '对外关闭'

# API 发现与调用
1panel-cli api list --filter openclaw
1panel-cli api describe POST ai/agents/plugins/search
1panel-cli api get core/auth/current
1panel-cli api post apps/search --body '{"page":1,"pageSize":10}'
```

完整参数见 `commands.md`。

## 关键约定

1. 专用命令优先于原始 `api`;原始调用必须先查询 `api list` / `api describe`。
2. `api` 路径相对 `/api/v2`;也接受完整的 `/api/v2/...` 路径。
3. 登录使用本地密码源 `authSource=local`;LDAP/OIDC/SAML 等企业认证登录不由此 CLI 代办。
4. 多节点面板务必选择正确的 `PANEL_NODE` / `--node`;未指定时发送 `CurrentNode: local`。
5. 新版防火墙创建会先调用规则检查接口并使用签名 `checkFlag`;不要绕过该流程手拼写请求。
6. nginx 配置保存后面板会 reload;写入前先读取原配置。生产密码与 Cookie 不应写入日志或提交仓库。
7. Swagger 不是完整路由清单;以 `references/api-dev-v2.json` 的源码路由为准。企业版闭源扩展接口不在该清单内。
8. `firewall docker` 与 `rules/check` 等新接口仅在新版面板存在;旧版面板会自动回退到旧防火墙接口,但无 Docker 端口守护。

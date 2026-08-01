---
name: 1panel-cli
description: >-
  管理 1Panel 面板的命令行工具(1panel-cli)。当用户提到 1Panel / 面板 / 宝塔
  类似的服务器管理面板,或需要登录面板、安装/管理应用(openresty、mysql、redis、
  phpmyadmin 等)、查看应用连接信息、修改数据库密码、创建/管理网站与域名、读写
  nginx 配置、配置 HTTPS/SSL、上传解压网站文件、管理 MySQL 数据库/用户、
  管理 Redis、配置防火墙端口/IP/转发规则,或调用面板任意 API 时使用本 skill。
  使用前先运行 `1panel-cli doctor` 检查环境,再按需执行对应命令,不要凭空猜测
  API 或手工拼接请求。
compatibility: opencode
metadata:
  tool: 1panel-cli
  panel: 1Panel
---

# 1panel-cli — 1Panel 面板管理 CLI

用命令行管理 1Panel 面板,所有操作均通过面板官方 API 完成,效果与管理页面一致。

## 环境

- 二进制位置(按顺序探测,取第一个存在的):
  1. `<本 skill 目录>/bin/1panel-cli` — 解压 `1panel-skill.zip` 后的标准位置
  2. `/root/code/1panel-cli/target/release/1panel-cli` — 本地开发构建
  3. `PATH` 中的 `1panel-cli`
- 配置: 把 `bin/.env.example` 复制为 `bin/.env` 并填写(PANEL_URL / PANEL_USERNAME / PANEL_PASSWORD / PANEL_ENTRANCE),CLI 会自动读取二进制同目录的 `.env`;也可放在当前工作目录
- 会话凭据自动保存在 `~/.config/1panel-cli/<profile>.json`,登录一次后无需重复登录

执行命令前先确定二进制路径,例如:

```sh
CLI=/root/code/1panel-cli/target/release/1panel-cli   # 本地开发
# 或
CLI="$(dirname "$0")/bin/1panel-cli"                  # zip 解压后(相对本 skill 目录)
# 或把 bin/1panel-cli 加入 PATH 后直接用 1panel-cli
```

## 第一步:检查环境

任何操作前先执行:

```
"$CLI" doctor
```

- 全部通过 → 继续后续操作
- 某项失败 → 按输出中的指导修复(改 .env、检查地址连通性等),不要跳过

## 常用命令速查

> 以下命令用 `1panel-cli` 表示;实际执行时按上方环境说明使用探测到的二进制路径。

```sh
# 会话
1panel-cli login                # 登录(读取 .env)
1panel-cli status               # 检查登录状态
1panel-cli doctor --force       # 强制重新登录验证账号密码

# 应用商店
1panel-cli apps list --installed        # 已安装应用
1panel-cli apps install mysql --wait    # 安装应用(默认最新版,--wait 等完成)
1panel-cli apps install phpmyadmin --param PANEL_APP_PORT_HTTP=8090 --wait
1panel-cli apps info mysql              # 应用详情 + 数据库连接信息(主机/端口/密码)
1panel-cli apps config openresty        # 查看应用配置参数
1panel-cli apps password mysql '新密码'  # 修改数据库 root 密码

# 数据库(MySQL)
1panel-cli db list                       # 列出数据库
1panel-cli db create appdb --user app --password 'App@123'
1panel-cli db users                      # 列出用户
1panel-cli db user add dev 'Dev@123' --dbs appdb

# Redis
1panel-cli redis info redis              # 状态与配置
1panel-cli redis config redis --set maxmemory=268435456
1panel-cli redis passwd redis '新密码'

# 网站 / OpenResty
1panel-cli web list                      # 列出所有域名
1panel-cli web create example.com --type static
1panel-cli web config example.com        # 读取 nginx 配置
1panel-cli web config example.com --file ./nginx.conf   # 修改 nginx 配置
1panel-cli web https example.com --enable --ssl-id <ID>
1panel-cli web ssl example.com --email admin@example.com
1panel-cli web upload example.com ./site.zip --to index
1panel-cli web extract example.com index/site.zip --to index

# 防火墙
1panel-cli firewall status
1panel-cli firewall start
1panel-cli firewall stop
1panel-cli firewall restart
1panel-cli firewall allow-ping
1panel-cli firewall ban-ping
1panel-cli firewall port add 8080 --protocol tcp
1panel-cli firewall port del 8080
1panel-cli firewall ip add 1.2.3.4 --strategy drop
1panel-cli firewall ip del 1.2.3.4
1panel-cli firewall forward add 18080 --to 10.0.0.2:8080
1panel-cli firewall forward del 18080 --to 10.0.0.2:8080 --num N

# 任意 API
1panel-cli api get core/auth/current
1panel-cli api post apps/search --body '{"page":1,"pageSize":10}'
1panel-cli api put core/settings/xxx --body '{}'
1panel-cli api delete core/xxx
```

完整命令速查见同目录 `commands.md`,CLI 源码与构建说明见仓库 `cli/` 目录(README 在 `cli/README.md`)。

## 关键约定

1. **路径**: 二进制在 `target/release/1panel-cli`,工作目录用 `/root/code/1panel-cli`(读取 .env)。
2. **会话复用**: 已登录时直接执行操作命令即可;登录失败(401/验证码/MFA)时按错误提示处理,必要时 `doctor --force` 或 `login --mfa <code>`。
3. **数据库密码修改** 走面板专用接口(真正的 ALTER USER),改完用 `apps info mysql` 查看最新连接信息。
4. **nginx 配置修改**: 编辑后立即生效(面板自动 reload);修改前建议先 `web config <域名>` 读取原配置。
5. **验证码**: 连续登录失败会触发面板验证码,此时需在面板清除或等待;避免反复用错误密码登录。
6. **安全**: 密码明文出现在命令行参数时,注意日志脱敏;涉及生产密码先询问用户是否展示。
7. **优先 CLI 而非 API**: 能用 `apps / db / redis / web / firewall` 子命令完成的操作,不要手工调 `api`。

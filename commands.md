# 1panel-cli 命令速查表

所有命令需先登录: `1panel-cli login`(读取 .env)或确认 `1panel-cli status` 已登录。

## 会话

| 命令 | 说明 |
|---|---|
| `login` | 登录面板,凭据存 `~/.config/1panel-cli/<profile>.json` |
| `status` | 检查会话是否有效 |
| `info` | 查看会话信息(不含 Cookie 明文) |
| `logout` | 登出并清除本地凭据 |
| `doctor [--force]` | 环境检查: .env / 配置项 / 地址连通 / 会话 / 登录 |

## apps — 应用商店

| 命令 | 说明 |
|---|---|
| `apps list [--name X] [--type X] [--tag X] [--installed] [--all]` | 列出可安装应用 |
| `apps install <name> [--version V] [--param K=V] [--wait]` | 安装应用 |
| `apps info <name>` | 应用详情 + 数据库/Redis 连接信息 |
| `apps config <name> [--set K=V] [--compose]` | 查看/修改应用配置(改后重建容器) |
| `apps password <name> <new>` | 修改 mysql/mariadb/redis root 密码 |
| `apps tags` | 列出分类标签 |

## db — MySQL 数据库

| 命令 | 说明 |
|---|---|
| `db list` | 列出数据库 |
| `db create <name> [--user U] [--password P] [--format utf8mb4] [--permission %]` | 创建数据库(可同时建用户) |
| `db delete <name> [--force]` | 删除数据库 |
| `db users [--instance mysql]` | 列出用户 |
| `db user add <user> <pass> [--host %] [--dbs a,b]` | 创建用户并授权 |
| `db user del <user> [--host %]` | 删除用户 |
| `db user passwd <user> <new> [--host %]` | 修改用户密码 |
| `db passwd <new>` | 修改 mysql root 密码 |

## redis — Redis 实例

| 命令 | 说明 |
|---|---|
| `redis list` | 列出实例 |
| `redis info <name>` | 状态 + 配置(端口/内存/连接数/requirepass) |
| `redis config <name> [--set timeout=.. --set maxclients=.. --set maxmemory=..]` | 读/改配置 |
| `redis passwd <name> <new>` | 修改密码 |

## web — 网站 / OpenResty

| 命令 | 说明 |
|---|---|
| `web list` | 列出所有网站/域名 |
| `web create <domain> [--type static] [--alias X]` | 新建网站 |
| `web config <domain> [--file x.conf]` | 读取/修改指定域名 nginx 配置 |
| `web https <domain> [--enable --ssl-id N \| --cert/--key \| --disable]` | 查看/设置 HTTPS |
| `web ssl <domain> --email x [--provider http]` | 申请 Let's Encrypt 证书 |
| `web upload <domain> <file> [--to 目录]` | 上传文件到网站目录 |
| `web extract <domain> <压缩包> [--to 目录] [--type zip]` | 解压到网站目录 |

## firewall — 防火墙

| 命令 | 说明 |
|---|---|
| `firewall status` | 防火墙状态 |
| `firewall start` / `stop` / `restart` | 启动 / 停止 / 重启防火墙 |
| `firewall allow-ping` / `ban-ping` | 允许 / 禁止 Ping |
| `firewall list [--type port\|address\|forward] [--info X] [--strategy X]` | 规则列表 |
| `firewall port add <port> [--protocol tcp\|udp\|tcp/udp] [--strategy accept\|drop] [--source IP] [--desc]` | 添加端口规则 |
| `firewall port del <port> [--protocol tcp] [--strategy accept] [--source IP]` | 删除端口规则 |
| `firewall batch <p1,p2,...> [--protocol tcp]` | 批量放行端口 |
| `firewall ip add <address> [--strategy drop] [--desc]` | 添加 IP 规则 |
| `firewall ip del <address> [--strategy drop]` | 删除 IP 规则 |
| `firewall forward add <port> --to IP:PORT [--protocol tcp]` | 添加端口转发 |
| `firewall forward del <port> --to IP:PORT --num N` | 删除端口转发(编号用 list 查看) |

## api — 任意面板 API

`api get <path>` / `api post <path> [--body JSON]` / `api put <path> [--body JSON]` / `api delete <path>`,路径相对 `/api/v2`,自动带 Cookie + CSRF。
例: `api get core/auth/current`、`api post apps/search --body '{"page":1,"pageSize":10}'`。

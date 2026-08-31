# 1panel-cli v0.3.0 命令速查

先运行 `1panel-cli doctor`,确认六项检查通过。所有命令支持全局参数 `--env <PATH>`、`--profile <NAME>`、`--node <NODE>`;节点也可由 `PANEL_NODE` 设置。

## 会话

| 命令 | 说明 |
|---|---|
| `login` | 使用本地账号源登录,保存会话 |
| `status` | 检查会话并显示面板版本/版本类型 |
| `info` | 查看脱敏后的会话信息 |
| `logout` | 登出并清除本地凭据 |
| `doctor [--force]` | 检查配置、连通性、会话、登录与面板版本兼容性 |

## apps — 应用商店

| 命令 | 说明 |
|---|---|
| `apps list [--name X] [--type X] [--tag X] [--installed] [--all]` | 列出应用 |
| `apps install <name> [--version V] [--param K=V] [--wait]` | 安装应用 |
| `apps info <name>` | 应用详情及数据库/Redis 连接信息 |
| `apps config <name> [--set K=V] [--compose]` | 查看/修改配置 |
| `apps password <name> <new>` | 修改 mysql/mariadb/redis root 密码 |
| `apps tags` | 列出分类标签 |

## db — MySQL / MariaDB

| 命令 | 说明 |
|---|---|
| `db list [--type mysql\|mariadb] [--instance NAME]` | 列出指定类型和实例的数据库 |
| `db create <name> [--user U] [--password P] [--instance NAME]` | 创建数据库 |
| `db delete <name> [--type mysql\|mariadb] [--instance NAME] [--force]` | 按正确数据库类型删除 |
| `db users [--instance NAME]` | 列出用户 |
| `db user add <user> <pass> [--host %] [--dbs a,b] [--instance NAME]` | 创建用户并授权 |
| `db user del <user> [--host %] [--instance NAME]` | 删除用户 |
| `db user passwd <user> <new> [--host %] [--instance NAME]` | 修改用户密码 |
| `db passwd <new> [--instance NAME]` | 修改实例 root 密码 |

## redis

| 命令 | 说明 |
|---|---|
| `redis list` | 列出实例 |
| `redis info <name>` | 状态与配置 |
| `redis config <name> [--set K=V]` | 查看/修改配置 |
| `redis passwd <name> <new>` | 修改密码 |

## web — 网站 / OpenResty

| 命令 | 说明 |
|---|---|
| `web list` | 列出网站和实际网站目录 |
| `web create <domain> [--type static] [--alias X]` | 新建网站 |
| `web config <domain> [--file x.conf]` | 读取/修改 nginx 配置 |
| `web https <domain> [--enable --ssl-id N \| --cert/--key \| --disable]` | 查看/设置 HTTPS |
| `web ssl <domain> --email x [--provider http]` | 申请证书 |
| `web upload <domain> <file> [--to DIR]` | 上传文件 |
| `web extract <domain> <archive> [--to DIR] [--type zip]` | 解压文件 |

## firewall

命令自动探测新版统一规则接口(`/hosts/firewall/settings`)和 v2.2.5 旧接口。新版创建流程会自动执行 `rules/check` → `rules`,删除使用托管规则 UUID;转发走独立 `/forward/*` 接口。

| 命令 | 说明 |
|---|---|
| `firewall status` | 状态、后端与新版后端选择(系统/转发/Docker) |
| `firewall start\|stop\|restart` | 生命周期操作 |
| `firewall allow-ping\|ban-ping` | Ping 策略 |
| `firewall list [--type port\|address\|forward] [--info X] [--strategy X]` | 规则列表 |
| `firewall port add <port> [--protocol tcp\|udp\|tcp/udp] [--strategy accept\|drop] [--source IP] [--desc X]` | 添加端口规则 |
| `firewall port del <port> [--protocol P] [--strategy S] [--source IP]` | 删除匹配的托管端口规则 |
| `firewall batch <p1,p2,...> [--protocol P]` | 批量添加端口规则 |
| `firewall ip add <address> [--strategy drop] [--desc X]` | 添加 IP 规则 |
| `firewall ip del <address> [--strategy drop]` | 删除匹配的托管 IP 规则 |
| `firewall forward add <port> --to IP:PORT [--protocol P]` | 添加端口转发 |
| `firewall forward del <port> --to IP:PORT [--num N]` | 删除端口转发 |

### firewall docker(新版 Docker 端口守护,仅新版面板)

| 命令 | 说明 |
|---|---|
| `firewall docker status` | 守护状态(后端/初始化/绑定/生效) |
| `firewall docker ports` | 列出容器已发布端口与防护策略(含策略 UUID) |
| `firewall docker sync` | 同步 Docker 端口防护规则 |
| `firewall docker operate [--operation init\|bind\|unbind]` | 初始化/绑定/解绑守护 |
| `firewall docker allow <PORT> --sources a,b [--protocol P] [--desc X]` | 仅允许指定来源访问已发布端口 |
| `firewall docker deny <PORT> [--sources a,b] [--protocol P] [--desc X]` | 拒绝指定来源;省略 --sources 时拒绝所有来源 |
| `firewall docker policy-del --uuid <UUID>` | 删除一条防护策略 |

`<PORT>` 支持 `8080`(匹配任意宿主 IP)或 `0.0.0.0:8080` 形式;策略会应用到所有匹配的已发布端口。更高级的规则同步(`rules/sync`)、重置(`rules/reset`)与排序(`rules/reorder`)可通过 `api` 命令调用。

## exec — 在面板本机执行命令

通过终端 WebSocket(`/api/v2/hosts/terminal/local`)把单行命令推送到面板本机执行,等待完成后一次性取回输出,非交互。退出码透传:命令完成时 CLI 以命令退出码退出(0 成功;1-255 透传;124 超时;CLI 自身错误为 1,可用 `--json` 的 `error` 字段精确区分)。

前置条件:面板已配置「设置 → 终端 → SSH 本地连接」;未配置时可在 `.env` 中提供 `LINUX_SSH_USER` / `LINUX_SSH_PWD`,然后用 `--sync-ssh` 自动写入。

| 命令 | 说明 |
|---|---|
| `exec <COMMAND> [--timeout SECS]` | 执行单行命令(不能含换行;默认超时 30s,超时返回 124 并打印已收集输出) |
| `exec <CMD> --cwd DIR` | 执行前先 `cd -- 'DIR'` |
| `exec <CMD> --json` | 输出单行 `{"exit_code","duration_ms","truncated","error","output"}` |
| `exec <CMD> --tail N` | 仅显示清洗后输出的最后 N 行 |
| `exec <CMD> --raw` | 不清洗输出(保留 ANSI/回显/提示符/哨兵;默认剥离) |
| `exec <CMD> --cols N --rows N` | PTY 尺寸(默认 120x40) |
| `exec <CMD> --sync-ssh [--ssh-port N]` | 先用 LINUX_SSH_USER/LINUX_SSH_PWD 自动配置本地 SSH 连接再执行 |

示例:

```sh
1panel-cli exec 'ls -la /var/www'
1panel-cli exec 'echo hello && hostname' --sync-ssh
1panel-cli exec 'bash deploy.sh' --cwd /opt/myapp --timeout 300
1panel-cli exec 'systemctl status docker' --timeout 10; echo "exit=$?"
1panel-cli exec 'whoami' --json
1panel-cli exec 'bash build.sh' --cwd /opt/app --tail 50
# 长任务推荐"无状态轮询",不要拉大 --timeout 硬等:
1panel-cli exec 'nohup bash deploy.sh > /tmp/deploy.log 2>&1 &'
1panel-cli exec 'tail -n 50 /tmp/deploy.log'
1panel-cli exec 'pgrep -f deploy.sh'   # 退出码 0=还在跑
```

注意:交互式命令(`sudo` 要密码、`top`/`vim` 等 TUI)会挂到超时;用 `-y`、`</dev/null` 等非交互参数。命令经 `( )` 子 shell 执行,`exit`/`exec` 类命令安全且退出码仍可透传。

## api — 发现和调用 API

| 命令 | 说明 |
|---|---|
| `api list [--filter TEXT]` | 列出官方源码路由;按路径、处理器、摘要、标签过滤 |
| `api describe <METHOD> <PATH>` | 显示路由元数据,并合并当前面板 Swagger 请求模型 |
| `api get <PATH>` | GET |
| `api post <PATH> [--body JSON]` | POST |
| `api put <PATH> [--body JSON]` | PUT |
| `api delete <PATH>` | DELETE |

路径相对 `/api/v2`,请求自动带 Cookie、CSRF、`Accept-Language` 和 `CurrentNode`。示例:

```sh
1panel-cli api list --filter templates
1panel-cli api describe POST websites/templates/search
1panel-cli api describe POST containers/compose/pin
1panel-cli api post ai/agents/plugins/search --body '{"page":1,"pageSize":20}'
```

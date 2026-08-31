# 1Panel CLI v0.3.0

一个用 Rust 编写的命令行工具,用于登录已安装的 1Panel 面板 Web 服务,并把登录成功的会话凭据保存到本机,之后可用保存的凭据调用面板 API。

验证基线为 1Panel dev-v2(下一版,统一防火墙 API);同时兼容 v2.2.5 及更早稳定版旧防火墙接口。请求自动发送 `authSource=local`、`Accept-Language` 与 `CurrentNode`。

## 功能

- `login`  读取 `.env` 中的账号信息登录面板,自动完成 RSA+AES 密码加密、安全入口、验证码与 MFA 流程,并将会话凭据(psession / pcsrftoken / SecurityEntrance 等 Cookie)保存到本机
- `status` 检查当前会话并显示面板版本
- `info`   查看已保存的会话信息(不展示 Cookie 明文)
- `logout` 调用面板登出接口并清除本地保存的凭据
- `apps`   应用商店操作:列出可安装应用、安装应用、查看应用详情(含数据库/Redis 连接信息)、修改配置与数据库密码
- `db`    数据库操作:MySQL/MariaDB 数据库/用户增删改查、root 密码
- `redis` Redis 实例操作:状态/配置/密码(与 db 平级)
- `firewall` 防火墙操作:状态、启停、Ping、端口/IP/转发规则增删查、批量放行;新版面板支持 Docker 端口守护(status/ports/sync/operate/allow/deny/policy-del)
- `web`    网站(OpenResty/nginx)操作:网站列表、nginx 配置读写、HTTPS/SSL、文件上传与解压
- `exec`   在面板本机执行单行命令(终端 WebSocket):退出码透传、输出清洗、`--cwd`/`--timeout`/`--json`/`--tail`、`--sync-ssh` 自动配置本地 SSH 连接
- `api`    发现官方源码路由、查看 Swagger 模型并调用 GET/POST/PUT/DELETE

## 构建

```bash
cd 1panel-cli
cargo build --release
# 生成的可执行文件: target/release/1panel-cli
```

## 配置(.env)

复制 `.env.example` 为 `.env` 并填写真实信息,CLI 启动时会依次从:当前目录、可执行文件所在目录查找 `.env`,也可以通过 `--env <path>` 指定。同时支持环境变量与命令行参数,优先级为 `命令行参数 > 环境变量 > .env 文件`。

```dotenv
# 面板地址(可包含安全入口路径)
PANEL_URL=http://10.0.0.1:41394/abcdef1234

# 面板安全入口(也可从 PANEL_URL 路径中自动识别)
PANEL_ENTRANCE=abcdef1234

# 用户名与密码
PANEL_USERNAME=admin
PANEL_PASSWORD=changeme

# 可选:面板开启 MFA 时需要的动态验证码
# PANEL_MFA_CODE=123456

# 可选:面板要求验证码时预先提供答案与 ID(配合外部 OCR 自动化)
# PANEL_CAPTCHA=8
# PANEL_CAPTCHA_ID=xxxxxx

# 可选:面板为 HTTPS 且使用自签名证书时设置为 true
# PANEL_INSECURE=false

# 可选:多节点面板目标节点(默认 local)
# PANEL_NODE=local

# 可选:面板所在主机的 SSH 凭据(仅 exec --sync-ssh 自动配置本地连接时使用)
# LINUX_SSH_USER=ubuntu
# LINUX_SSH_PWD=changeme
```

## 使用示例

```bash
# 登录(默认使用当前目录下的 .env)
1panel-cli login

# 指定 .env 文件 / profile
1panel-cli --env /path/to/.env login
1panel-cli --profile my-panel login

# 使用命令行参数(优先级最高)
1panel-cli login --url http://10.0.0.1:41394 --user admin --password 'xxx'

# 登录成功后
1panel-cli status                      # 检查登录状态
1panel-cli info                        # 查看会话信息
1panel-cli api get core/auth/current   # 调用面板 API

# API 发现(内置 dev-v2 源码清单 + 当前面板 Swagger)
1panel-cli api list --filter templates
1panel-cli api describe POST websites/templates/search

# 环境检查(.env / 连通性 / 会话 / 登录,失败时给出修复指导)
1panel-cli doctor                      # 快速检查
1panel-cli doctor --force              # 强制重新登录验证账号密码

# 应用商店
1panel-cli apps list                   # 列出可安装应用(默认每页 50)
1panel-cli apps list --all             # 拉取全部应用
1panel-cli apps list --name nginx      # 按名称模糊搜索
1panel-cli apps list --type runtime    # 按类型过滤(website/runtime/tool...)
1panel-cli apps list --tag Database    # 按标签过滤
1panel-cli apps list --installed       # 只显示已安装的应用
1panel-cli apps tags                   # 列出所有应用分类

# 安装应用(使用各应用默认参数)
1panel-cli apps install openresty
1panel-cli apps install mysql --wait   # --wait 等待安装完成并显示进度
1panel-cli apps install redis --wait

# 指定版本 / 覆盖安装参数(KEY=VALUE,可多次指定)
1panel-cli apps install mysql --version 8.4.11 \
    --param PANEL_DB_ROOT_PASSWORD=MyPass123 \
    --param PANEL_APP_PORT_HTTP=3307
1panel-cli apps install phpmyadmin --param PANEL_APP_PORT_HTTP=8090 --wait

# 查看已安装应用的详细信息(数据库/Redis 会显示连接信息)
1panel-cli apps info mysql
1panel-cli apps info redis
1panel-cli apps info openresty

# 查看应用配置(参数 / docker-compose)
1panel-cli apps config mysql
1panel-cli apps config openresty --compose

# 修改应用配置(修改后会自动重建容器)
1panel-cli apps config openresty --set WEBSITE_DIR=/data/www
1panel-cli apps config redis --set PANEL_APP_PORT_HTTP=6380

# 修改数据库 root 密码(替换弱密码,mysql/mariadb/redis 专用接口)
1panel-cli apps password mysql 'Mysql@2026#XyZ!q9'
1panel-cli apps password redis 'Redis@2026#XyZ!r8'

# === 数据库管理 ===
# MySQL 数据库
1panel-cli db list                              # 列出 MySQL 数据库
1panel-cli db list --type mariadb --instance mariadb # 列出 MariaDB 数据库
1panel-cli db create appdb --user app --password 'App@123' --remark '应用库'
1panel-cli db delete appdb                      # 删除(被引用时报错,加 --force 强制)
1panel-cli db users                             # 列出数据库用户
1panel-cli db user add dev 'Dev@123' --dbs appdb --host %
1panel-cli db user del dev
1panel-cli db user passwd dev 'Dev@456'
1panel-cli db passwd 'Mysql@2026#XyZ'           # 修改 mysql root 密码

# Redis(与 db 平级)
1panel-cli redis list                             # 列出 Redis 实例
1panel-cli redis info redis                       # 状态 + 配置
1panel-cli redis config redis                     # 读取配置
1panel-cli redis config redis --set maxmemory=268435456 --set timeout=300
1panel-cli redis passwd redis 'Redis@2026#XyZ'    # 修改 Redis 密码

# === 防火墙 ===
1panel-cli firewall status                       # 状态(Ping/版本/运行状态)
1panel-cli firewall start | stop | restart       # 启停防火墙
1panel-cli firewall allow-ping | ban-ping        # 允许/禁止 Ping
1panel-cli firewall list [--type port|address|forward] [--info] [--strategy]
1panel-cli firewall port add 8080 --protocol tcp --strategy accept --desc 'web'
1panel-cli firewall port del 8080 --protocol tcp
1panel-cli firewall batch 8081,8082,8083 --protocol tcp
1panel-cli firewall ip add 1.2.3.4 --strategy drop --desc '黑名单'
1panel-cli firewall ip del 1.2.3.4
1panel-cli firewall forward add 18080 --to 10.0.0.2:8080
1panel-cli firewall forward del 18080 --to 10.0.0.2:8080 --num 1   # 编号用 list 查看

# Docker 端口守护(新版统一防火墙 API,旧版面板不可用)
1panel-cli firewall docker status                  # 守护状态
1panel-cli firewall docker ports                   # 已发布端口与防护策略
1panel-cli firewall docker allow 8080 --sources 1.2.3.4 --desc web   # 仅允许指定来源
1panel-cli firewall docker deny 0.0.0.0:8080 --desc '对外关闭'        # 拒绝所有来源
1panel-cli firewall docker policy-del --uuid <UUID>                  # 删除策略

# 在面板本机执行命令(前置:面板已配置本地 SSH 连接,或用 --sync-ssh 自动配置)
1panel-cli exec 'echo hello && hostname' --sync-ssh  # 自动写入本地 SSH 连接后执行
1panel-cli exec 'df -h && free -m'                   # 执行并打印清洗后输出
1panel-cli exec 'bash deploy.sh' --cwd /opt/myapp --timeout 300   # 切目录 + 调大超时
1panel-cli exec 'systemctl status docker'; echo "exit=$?"         # 退出码透传
1panel-cli exec 'whoami' --json                      # 单行 JSON 结构化结果
1panel-cli exec 'bash build.sh' --cwd /opt/app --tail 50          # 超长输出只看结尾
# 长任务建议后台化后轮询日志,不要拉大 --timeout 硬等:
1panel-cli exec 'nohup bash deploy.sh > /tmp/deploy.log 2>&1 &'
1panel-cli exec 'tail -n 50 /tmp/deploy.log'

# 网站(OpenResty/nginx)管理
# 列出所有网站/域名
1panel-cli web list

# 读取指定域名的 nginx 配置
1panel-cli web config test.local

# 用本地文件内容修改 nginx 配置(保存后自动 reload 生效)
1panel-cli web config test.local --file ./nginx.conf

# 查看 HTTPS 状态
1panel-cli web https test.local

# 申请 SSL 证书(Let's Encrypt,http 验证)并打印证书 ID
1panel-cli web ssl test.local --email admin@example.com

# 用申请的证书 ID 开启 HTTPS
1panel-cli web https test.local --enable --ssl-id 1

# 手动导入证书(如自签名)开启 HTTPS
1panel-cli web https test.local --enable --cert ./test.crt --key ./test.key

# 关闭 HTTPS
1panel-cli web https test.local --disable

# 上传本地文件到网站目录(相对网站根目录,如 --to index)
1panel-cli web upload test.local ./site.zip --to index

# 解压网站目录中的压缩包到指定目录
1panel-cli web extract test.local index/site.zip --to index/site

# 带 JSON 请求体的 POST
1panel-cli api post core/settings/... --body '{"key":"value"}'

# 退出登录并清除本地凭据
1panel-cli logout
```

## 验证码与 MFA

- **验证码**:面板仅在登录失败(密码错误)后才会要求验证码。CLI 检测到需要验证码时,会自动拉取算式图片并保存到临时目录(如 `/tmp/1panel-cli-captcha-*.png`),提示你打开图片输入算式结果;非交互场景可通过 `PANEL_CAPTCHA` / `--captcha` 提供。若已通过外部 OCR 预先获取验证码,可同时提供 `--captcha` 与 `--captcha-id` 跳过自动拉取。
- **MFA**:面板开启 MFA 时,CLI 会交互式提示输入动态验证码;自动化场景可通过 `PANEL_MFA_CODE` / `--mfa` 提供。

## 登录原理(与前端保持一致)

1. 请求 `/api/v2/core/auth/setting`,同时从 `panel_public_key` Cookie 获取面板 RSA 公钥
2. 生成 16 字节随机 AES 密钥(hex 字符串),用 RSA(PKCS1v15)加密该密钥字符串
3. 用 AES-256-CBC(PKCS7)加密密码,最终提交 `keyCipher:iv:cipher` 格式的密码字段
4. 通过 `EntranceCode` 请求头(base64 编码)携带安全入口
5. 所有请求附带 `Accept-Language` 和 URI 编码的 `CurrentNode`(默认 `local`)
6. 登录成功后保存 `psession`、`pcsrftoken`、`SecurityEntrance` 等 Cookie;后续 API 请求自动携带,写操作附带 `X-CSRF-Token`

## 会话存储

登录凭据以 JSON 形式保存在 `~/.config/1panel-cli/<profile>.json`(权限 `0600`),每个 `profile` 一份,默认 `default`。

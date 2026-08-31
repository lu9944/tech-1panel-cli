# 1Panel dev-v2(下一版)兼容性审计

审计日期:2026-08-31。基线为官方开发分支 `origin/dev-v2`(`433f1a940fff0fdbaf0a14def61283f0685199dc`,即 v2.2.5 之后的下一版);同时保留对稳定版 `v2.2.5`(`e7ef35740ccd5ea026e091ee15168a2ba35ab923`)的兼容。内置 API 清单由 `scripts/generate_api_catalog.py` 从该提交源码重新生成,共 **766 条路由**(Swagger 收录 750 条,源码注册多于 Swagger)。

## 结论

- dev-v2 相比 v2.2.5 **删除了 12 条旧防火墙路由**,新增 **23 条防火墙路由 + 1 条文件路由**(`POST /files/move/stop`);其余路由(登录、应用商店、数据库、Redis、网站、容器、Compose、AI 等)在 router 层没有任何增删改。
- v0.3.0 CLI 对旧版/新版防火墙做双模适配:先探测 `GET /api/v2/hosts/firewall/settings`,存在则走新版统一规则/独立转发/Docker 守护 API,否则回退到 v2.2.5 旧路由。因此两个版本的面板都能使用专用命令。
- 登录、请求头(`authSource=local`、`Accept-Language`、URI 编码的 `CurrentNode`)、CSRF 与会话模型在 dev-v2 中未变化,CLI 无需改动。
- dev-v2 新增了 Docker 端口守护(port guard)子系统,CLI v0.3.0 新增 `firewall docker` 命令组覆盖其核心操作。

## dev-v2 移除的路由(旧防火墙)

旧版按"端口/IP/转发/批量/过滤器"分离的接口全部移除:

```
POST /hosts/firewall/search            POST /hosts/firewall/port
POST /hosts/firewall/ip                POST /hosts/firewall/forward
POST /hosts/firewall/batch             POST /hosts/firewall/update/port
POST /hosts/firewall/update/addr       POST /hosts/firewall/update/description
POST /hosts/firewall/filter/rule/search
POST /hosts/firewall/filter/rule/operate
POST /hosts/firewall/filter/rule/batch
POST /hosts/firewall/filter/chain/status
```

## dev-v2 新增的路由

统一规则 / 后端设置:

```
GET  /hosts/firewall/settings
POST /hosts/firewall/settings/operate
POST /hosts/firewall/rules/search
POST /hosts/firewall/rules/check
POST /hosts/firewall/rules
POST /hosts/firewall/rules/update
POST /hosts/firewall/rules/delete
POST /hosts/firewall/rules/reorder
POST /hosts/firewall/rules/reset
POST /hosts/firewall/rules/native/detail
POST /hosts/firewall/rules/sync
POST /hosts/firewall/rules/sync/preview
GET  /hosts/firewall/rules/sync/task
```

独立转发:

```
POST /hosts/firewall/forward/base
POST /hosts/firewall/forward/search
POST /hosts/firewall/forward/operate
POST /hosts/firewall/forward/enable
```

Docker 端口守护:

```
GET  /hosts/firewall/docker/ports
GET  /hosts/firewall/docker/endpoints
POST /hosts/firewall/docker/sync
POST /hosts/firewall/docker/operate
POST /hosts/firewall/docker/policies/batch
POST /hosts/firewall/docker/policies/delete/batch
```

文件:

```
POST /files/move/stop
```

## 请求模型要点(v0.3.0 已对齐)

| 接口 | 模型要点 |
|---|---|
| `rules/search` | `page/pageSize/scopes/all/info/families/actions/states/excludeChains`;scopes 用 `ManagedInputScopes`:iptables/nftables 为 2 族 × 3 链(`1PANEL_BASIC_BEFORE/BASIC/AFTER`),firewalld 为 `inet/public`,ufw 为 2 族 × `incoming` |
| `rules/check` → `rules` | 响应含 `decision/classification/reason/requestedRule/allowedActions/checkFlag`;CLI 对 `blocked` 报错、`no_change`/`exact_managed`/`exact_external` 跳过,其余携带 `checkFlag` + `action=create` + `sourceKind=user` 提交。checkFlag 为服务端 HMAC 签名,必须原样回传且规则对象与检查时一致 |
| `rules/delete` | 请求体 `{"uuids": [...]}`;仅删除托管规则(desired UUID),外部/受保护规则不删 |
| `forward/search` | `page/pageSize/info/status/strategy`;响应字段与旧版一致(`num/targetIP/targetPort/interface`) |
| `forward/operate` | `{"rules":[{operation,num,protocol,port,targetIP,targetPort,interface}],"forceDelete":false}`,operation 为 `add/remove` |
| `docker/policies/batch` | `endpoints`(family/hostIP/hostPort/protocol)+ `mode`(`allow_sources`/`deny_sources`/`deny_all`)+ `sources` + `description`;`deny_all` 时 sources 为空 |
| `base`(LoadFirewallBaseInfo) | 响应升级为 `FirewallSubsystemStatus`,新增 `backend/conflictBackend/ipv4/ipv6` 等字段;`name` 仍为后端名,旧版兼容 |
| `operate`(生命周期) | operation 白名单 `start/stop/restart/disableBanPing/enableBanPing`,与旧版一致 |

## 其他非防火墙变化

- `POST /files/move/stop`:停止文件移动任务(配合已有的 `/files/move`),可用 `api post files/move/stop` 调用。
- 面板设置新增 `AllowIPTrustedProxies`,API 设置页新增 `apiTrustedProxies`(仅影响服务端解析客户端 IP,不影响 CLI)。
- Dashboard 新增 NPU/GPU 字段(`npuData`、`pciBdfAddress` 等),纯响应扩展,不破坏解析。

## 终端 WebSocket(exec 命令,v0.4.0 新增)

`exec` 命令依赖的终端接口在 v2.2.5 与 dev-v2 完全一致(`git diff v2.2.5 origin/dev-v2` 已复核 `core/utils/terminal/ws_session.go`、`agent/app/api/v2/terminal.go`、设置服务与 `/hosts/terminal/*` 路由均无差异):

```
GET /api/v2/hosts/terminal/local   # command/cols/rows 查询参数,WS 升级,会话 Cookie 认证
POST /api/v2/settings/ssh          # 保存面板本地 SSH 连接(--sync-ssh 使用,password base64)
GET /api/v2/settings/ssh/conn      # 读取本地连接配置(addr 为空即未配置)
POST /api/v2/settings/ssh/default  # defaultConn=Disable & withReset=true 可清除本地连接
```

实测要点(2026-08-31,v2.2.5 面板):

- WS 握手只需会话 Cookie + `CurrentNode: local`,无需 CSRF 与安全入口路径。
- agent 发生的**错误以 Close 帧返回,但 payload 是裸错误文本,不含 RFC 6455 的 2 字节状态码**(gorilla `WriteControl(CloseMessage, err)` 的行为)。tungstenite 客户端解析时会把前 2 字节当作状态码,而 "fa"("failed to..." 开头)等不是合法状态码,于是整帧被替换为 `reason="Protocol violation"`,**原始错误文本不可读**。因此 `exec` 在收到 Close 帧后改用 `GET /settings/ssh/conn` 探测归因:addr 为空 → "本地 SSH 连接未配置";非空 → 提示凭据可能已变更可 `--sync-ssh` 覆盖。
- 完成信号采用"随机哨兵携带 `$?`":实际注入 ` clear &&( <用户命令> ); echo __1PCLI_<token>_$?`。用户命令放进 `( )` 子 shell,`exit`/`exec` 类命令只结束子 shell,哨兵仍在外层执行并透传退出码(实测 `exec 'exit 7' --json` 返回 `exit_code:7` 且 CLI 退出码为 7)。
- 命令正常结束(哨兵命中)后客户端主动关闭 WS;若用户命令杀掉外层 shell,WS 以 TCP EOF 结束(无 Close 帧),`exec` 报"连接被意外关闭"。

## 范围与限制

- 清单覆盖开源仓库 `agent/router` 与 `core/router`。企业版闭源 LDAP/OIDC/SAML 扩展路由不在源码清单内。
- CLI 的交互式登录仅支持本地账号密码源。已有 Cookie 会话仍可调用其权限范围内的接口。
- dev-v2 为开发分支,接口仍可能在正式发布前变化。每次稳定版升级应重新运行 `scripts/generate_api_catalog.py` 并复核本报告。

## 再生成 API 清单

```sh
python3 scripts/generate_api_catalog.py \
  --source /path/to/1Panel \
  --version dev-v2 \
  --output references/api-dev-v2.json
```
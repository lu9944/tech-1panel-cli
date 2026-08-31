# 开发文档:`1panel-cli exec` 在面板本机执行命令

> 目标版本:1Panel v2.2.5(dev-v2 兼容)。实现语言:Rust(CLI 与 1panel-cli 同仓)。

## 0. 参考与本地环境

### 0.1 参考源码

1Panel 源码已检出到本机,文档中所有代码路径均以此目录为根:

```text
/home/ubuntu/code/1Panel           # 1Panel 源码(当前 HEAD: v2.2.5,另有 dev-v2 分支)
```

- 核心服务(面板 HTTP/会话/代理):`core/...`
- Agent(本机终端、SSH、设置):`agent/...`

阅读顺序建议:
1. `agent/app/api/v2/terminal.go` — 终端 WS 入口(本地/主机/容器)
2. `core/utils/terminal/ws_session.go` — WebSocket 帧协议与 SSH 会话
3. `agent/app/api/v2/setting.go` / `agent/app/service/setting.go` — 本地 SSH 连接(LocalSSHConn)的保存与读取
4. `core/middleware/session.go`、`core/init/router/proxy.go`、`agent/middleware/certificate.go` — 认证与本地代理链路

### 0.2 本地测试环境

本机已安装与该源码版本一致的 1Panel 服务(位于 `http://127.0.0.1:39870`,已配置安全入口)。测试配置见项目根目录:

```text
/home/ubuntu/code/tech-1panel-cli/.env
```

其中包含面板地址、安全入口、登录账号密码。**该文件含真实凭据,勿提交到仓库**(已被 `.gitignore` 忽略)。文档正文不展示具体值,开发/测试时按需读取。

面板本地 SSH 连接(`hosts/terminal/local` 的前置条件)可通过面板 UI「设置 → 终端 → SSH 本地连接」配置,也可由 `exec` 命令在运行时用下面的环境变量自动配置(见 4.4)。

### 0.3 `exec` 相关环境变量

在 `.env`(及 `.env.example`)中新增两个变量,供 `exec` 自动配置面板"本地 SSH 连接"时使用:

```dotenv
LINUX_SSH_USER=ubuntu   # 面板所在 Linux 主机的 SSH 用户名
LINUX_SSH_PWD=ubuntu    # 对应用户的 SSH 密码
```

约定:
- 由 `cli/src/config.rs` 读取(新增 `ENV_LINUX_SSH_USER` / `ENV_LINUX_SSH_PWD` 常量,复用现有 `env_get` 逻辑),并纳入 `ConfigOverrides` / `PanelConfig`。
- `exec` 若检测到面板本地 SSH 连接未配置(或用户显式指定 `--sync-ssh`),则用这两个变量调用 `settings/ssh` API 写入本地连接(addr 固定 `127.0.0.1`,port 默认 `22`,authMode `password`),使 `hosts/terminal/local` 可用。
- 未设置这两个变量时,`exec` 不自动配置,直接透传服务端"未配置本地连接"的错误。

## 1. 背景与目标

`1panel-cli` 目前只有 REST 风格的子命令(`apps / db / redis / web / firewall / api`),无法在面板本机(或容器/主机)上执行 Shell 命令。1Panel 的"终端"页面基于 WebSocket 实现,面板未暴露"执行命令并返回结果"的 REST 接口。

本文档设计一个 `exec` 子命令:通过 1Panel 的 WebSocket 终端接口,把一条**单行命令**推送到面板本机执行,按**"等待一段时间后一次性取回输出"**的方式展示结果,无需交互。

**v1 范围(已确认):**
- 目标:仅面板本机(`hosts/terminal/local`)
- 输出:等待后一次性打印,且**默认清洗**(剥离 ANSI/回显/提示符,`--raw` 看原始流)
- 命令:单行命令(不支持的换行/多行脚本后续再加);**退出码透传**(哨兵携带 `$?`,见 2.5)
- 机器可读:`--json` 结构化结果、`--tail N` 输出截断、`--cwd` 切目录(服务脚本/AI 场景)

## 2. 可行性验证(基于 v2.2.5 源码)

### 2.1 终端接口本身支持"执行单条命令"

`agent/app/api/v2/terminal.go`:

```go
func (b *BaseApi) WsLocalTerminal(c *gin.Context) {
    b.runSSHSession(c, loadLocalConn, c.DefaultQuery("command", ""))
}
```

`runSSHSession` 连接本机 SSH 后调用 `core/utils/terminal/ws_session.go` 的 `NewLogicSshWsSession`:

```go
if len(initCmd) != 0 {
    time.Sleep(100 * time.Millisecond)
    _, _ = stdinP.Write([]byte(" clear &&" + initCmd + "\n"))
}
```

即:只要 WS 握手 URL 带上 `command` 查询参数,服务端 100ms 后自动把 ` clear &&<command>\n` 写入 SSH shell,命令输出通过 WebSocket 以 base64 JSON 帧推回。**不需要交互。**

### 2.2 WebSocket 帧格式

`core/utils/terminal/ws_session.go`:

```go
const (
    WsMsgCmd       = "cmd"
    WsMsgResize    = "resize"
    WsMsgHeartbeat = "heartbeat"
)
type WsMsg struct {
    Type      string `json:"type"`
    Data      string `json:"data,omitempty"`      // cmd: base64 编码的终端输出/输入
    Cols      int    `json:"cols,omitempty"`      // resize
    Rows      int    `json:"rows,omitempty"`      // resize
    Timestamp int    `json:"timestamp,omitempty"` // heartbeat
}
```

- **服务端 → 客户端**:`{"type":"cmd","data":"<base64>"}` 携带终端输出(每 60ms 定时器批量推送,`sendComboOutput`);`heartbeat` 原样回显。
- **客户端 → 服务端**:`{"type":"cmd","data":"<base64 输入>"}`;`{"type":"resize","cols":N,"rows":N}`;`{"type":"heartbeat","timestamp":<ms>}`(可选,长命令可周期发送用于保活/延迟检测)。
- 我们只读不写即可(命令通过 URL 参数注入)。
- `cols`/`rows` 查询参数服务端用 `strconv.Atoi` 解析且失败即报错(`terminal.go:107-114`),必须传纯数字;服务端默认 80x40。
- agent 侧 `upGrader.CheckOrigin` 恒为 true(`terminal.go:305-310`),握手无需伪造 `Origin` 头。

### 2.3 握手成功后的错误如何传递(以 Close 帧为主)

`wshandleError`(`agent/app/api/v2/terminal.go:285-303`)是 agent 侧统一的 WS 错误通道:**优先发送携带错误文本的 WebSocket Close 帧**(payload 即错误消息),仅当写 Close 帧本身失败时才退化为一条 `cmd` 消息。触发点包括:

- 本地 SSH 连接未配置/凭据错误:`loadLocalConn` 失败,错误被包装为 `failed to set up the connection. Please check the host information: no such ssh conn info in db`(`terminal.go:125-127` + `setting.go:328-333`);
- `cols`/`rows` 非数字、agent 建连/PTY 申请失败等。

**实现要点**:WS 升级(HTTP 101)成功后,错误大概率以 `Close` 帧形式到达(tungstenite 表现为 `Message::Close(Some(frame))`,`frame.reason` 为错误文本)。`exec` 的读循环必须处理 Close 帧 reason 并将其作为服务端错误上报,不能只等 `cmd` 消息。

### 2.4 认证链路(关键结论:复用会话 Cookie 即可)

WS 握手是 GET 请求,会穿过 core 的全局中间件链(`core/init/router/router.go:70` `Routers()` 中的注册顺序):

```text
WhiteAllow → BindDomain → FrontendFallback → OperationLog → GlobalLoading
→ CoreAPIAuth(API Key) → PasswordExpired → CSRFTokenGuard → RBAC → Proxy
```

对 `GET /api/v2/hosts/terminal/local` 逐项核对(v2.2.5 源码):

- **会话 Cookie**:v2 的会话 cookie 名是 **`psession`**(`core/constant/session.go:5` `SessionName`),不是 v1 的 `panel`。登录时面板还会下发 `SecurityEntrance`、`pcsrftoken`、`panel_public_key` cookie(`core/init/session/psession/psession.go:128-130`、`core/app/auth/auth.go:184`)。`1panel-cli` 登录后保存**全部** cookie 并在后续请求重放(`cli/src/auth.rs` 的 `client.cookies()`),因此实现上无需关心具体名字。
- **实际拦 API 的是谁**:`SessionAuth()`(`core/middleware/session.go:15`)在 v2 只挂在 swagger 路由(`core/init/router/router.go:78`);真正校验会话的是 `PasswordExpired()`(对 `/api/v2/*` 调 `global.SESSION.Get`,失效返回 `ErrNotLogin` JSON)与 `Proxy()` 内的 `checkSession`(失效返回 **401 HTML**,`core/init/router/proxy.go:43-46`)。错误处理需同时兼容 JSON 与 HTML 两种 401 响应。
- **CSRF**:`CSRFTokenGuard` 只校验非 GET/HEAD/OPTIONS/TRACE(`core/middleware/csrf_protect.go:36-46`),WS 握手是 GET,**无需 CSRF token**(`--sync-ssh` 的 POST 仍需,`post_json` 已带)。
- **节点选择**:`Proxy()` 优先读 `operateNode` 查询参数,其次 `CurrentNode` 请求头(`core/init/router/proxy.go:29-38`);`currentNode` 为 `local` 或空时,`/api/v2/hosts/terminal/local` 直接代理给本机 agent(Unix socket,`proxy.go:54-57`,`httputil.ReverseProxy` 原生支持 WS 升级),agent 侧 `agent/middleware/certificate.go:19` 在 `global.IsMaster` 时直接放行,**不需要额外的 agent API Key**。
- **其它可能的拦截**(边缘场景,见 §5):`GlobalLoading` 要求 SystemStatus 为 `Free`(面板升级中返回 407);`BindDomain` 若设置了绑定域名,用非绑定域名/127.0.0.1 访问会被 HTML 错误拒绝;`WhiteAllow` 对私网 IP 直接放行(本机访问 127.0.0.1 不受 AllowIPs 限制)。
- 因此 WS 握手只需携带全部会话 Cookie + `CurrentNode: local` 请求头。**这些 `1panel-cli` 已保存在 `cli/src/session.rs` 的 `Session.cookies` 中,直接复用;URL 无需拼接安全入口路径(API 路径不经过 entrance 校验)。**

### 2.5 完成信号(关键难点)

- 命令执行完后 **shell 不退出**,WS 连接会一直保持,服务端不会自动关闭(唯一例外是检测到 `\r\nlogout\r\n`,与我们无关)。
- 服务端关闭 WS 的时机(`sws.Wait` 返回后)与 `sendComboOutput` 发送缓冲存在**竞态**:`sendComboOutput` 收到退出信号可能丢弃最后一批输出(见 `ws_session.go:204` 附近),因此**不能依赖 WS 关闭作为完成信号**(会丢尾部输出)。

**方案:随机哨兵携带 `$?`(单次命中即完成 + 退出码透传)**

- 实际发送(服务端会拼成 ` clear &&<命令行>\n`):`<user_cmd>; echo __1PCLI_<随机token>_$?`
- PTY 开启回显(`ssh.TerminalModes{ssh.ECHO: 1}`)时,哨兵在输出中出现 2 处但**形态不同**:
  1. 回显的输入行里是**字面量** `__1PCLI_<token>_$?`(`$?` 未展开);
  2. `echo` 真正输出的是**展开后**的 `__1PCLI_<token>_<0-255>`。
- 因此用正则 `__1PCLI_<token>_(\d{1,3})` 匹配,**单次命中即判定命令完成**,捕获组即命令退出码(作为 CLI 退出码透传,见 §3);随后主动关闭 WS。
- 相比"回显 + 输出各计 1 次、共 2 次"的朴素方案,该设计有三重收益:
  - **退出码可透传**——脚本/CI 能判断命令成败;
  - **不再依赖回显必然出现**——shell 被设置 `stty -echo` 或被 profile 干扰回显时,仍能完成判定(字面量 `$?` 本来就不参与匹配);
  - **折行拆断暴露面减半**——只需真实输出那一处完好。
- 不依赖 WS 关闭时序 → 规避尾部输出丢失。

**容错:** 若命令语法错误、shell 被杀死导致哨兵未出现(0 次命中),以 `--timeout` 兜底(默认 30s),超时打印已收集内容并返回 124。

**注意(终端折行会拆断哨兵)**:PTY 列数有限(`--cols`,默认 120),回显输入行与 `echo` 输出行都会在列边界被插入 `\r\n`(软折行),哨兵串可能被拦腰截断,朴素子串匹配会漏判。对策(实现时二选一或组合):

- 在**归一化后的流**上匹配:把累计输出先剥离 ANSI 转义序列与 `\r\n` 再做正则匹配(展示时仍用原始流);
- 选用足够短的随机 token(如 8 个 hex 字符),降低跨折行边界的概率(不根治)。

另外精确地说,服务端的 logout 检测是**整批缓冲恰好等于** `\r\nlogout\r\n`(`ws_session.go:204` 的整串比较),不是子串匹配,正常命令输出不会误触发。

## 3. 命令设计

```text
1panel-cli exec <COMMAND>
    [--timeout SECS]     超时(默认 30;超时返回 124,远端命令随 WS 关闭被终止)
    [--cwd DIR]          执行前先切换到 DIR(拼为 cd -- 'DIR' && <COMMAND>)
    [--cols N]           PTY 列数(默认 120)
    [--rows N]           PTY 行数(默认 40)
    [--json]             输出单行结构化 JSON(见 3.1),适合脚本/AI 消费
    [--tail N]           仅显示清洗后输出的最后 N 行(防超长输出刷屏/撑爆上下文)
    [--raw]              不做输出清洗(保留 ANSI/回显/提示符/哨兵;默认清洗)
    [--sync-ssh]         用 LINUX_SSH_USER / LINUX_SSH_PWD 自动配置面板本地 SSH 连接后执行
    [--ssh-port N]       --sync-ssh 时使用的 SSH 端口(默认 22)
```

示例:

```sh
1panel-cli exec 'ls -la /var/www'
1panel-cli exec 'df -h && free -m'
# 切换目录执行脚本(耗时任务调大超时)
1panel-cli exec 'bash deploy.sh' --cwd /opt/myapp --timeout 300
# 退出码透传:命令/脚本失败时 CLI 以相同退出码退出
1panel-cli exec 'systemctl status docker' --timeout 10; echo "exit=$?"
# 结构化结果(脚本/AI 消费)
1panel-cli exec 'whoami' --json
# 超长输出只看结尾
1panel-cli exec 'bash build.sh' --cwd /opt/app --tail 50
# 自动写入本地 SSH 连接(LINUX_SSH_USER/PWD)后执行
1panel-cli exec 'whoami' --sync-ssh
```

返回码:
- `0`:命令完成且退出码为 0
- `1-255`:命令完成,**透传命令退出码**(注意:命令本身退出 1 与 CLI 基础设施错误同为 1,需精确区分时用 `--json` 的 `error` 字段)
- `1`:CLI 侧错误(握手失败/服务端 Close 帧报错/`--sync-ssh` 失败/预校验不过)
- `124`:超时(对齐 GNU `timeout` 惯例;已收集内容仍会打印)

### 3.1 输出清洗与 AI 友好性

定位:`exec` 的主要消费者是脚本与 AI agent,输出质量比传输方式更重要:

- **默认清洗**:以哨兵行为锚,剥离 ANSI 转义、PTY 回显输入行(` clear &&...`)、哨兵输出行及其后的提示符残余;`--raw` 保留原始流。清洗是启发式的,自定义 PS1/无换行提示符等极端情况可能残留杂项,可用 `--raw` 对照排查。
- **`--json`**:单行 `{"exit_code":N,"duration_ms":N,"truncated":false,"error":null,"output":"..."}`;`output` 默认为清洗后文本(与 `--raw` 组合则为原始流)。CLI 自身出错时输出 `{"error":"..."}` 并退出 1。
- **上下文保护**:收集上限默认 10MB(超出截断并标记 `truncated`);`--tail N` 仅显示末 N 行——AI 反复轮询日志时建议总是带 `--tail`。
- **不做实时流式输出(`--stream` 已裁剪)**:协议层本就是流式(服务端每 60ms 推一帧),实时打印只对人盯进度/管道有意义;本 CLI 的消费者是脚本与 AI(一次性调用模型,进程退出才能拿到结果),流式无增益,故只保留"收集后一次性打印"。
- **长任务用"无状态轮询",不做持久会话**:面板每次 WS 连接都是新 shell、无会话 ID,持久化需客户端常驻 daemon(成本高收益低)。agent 处理长任务的推荐模式:
  ```sh
  1panel-cli exec 'nohup bash deploy.sh > /tmp/deploy.log 2>&1 &'
  1panel-cli exec 'tail -n 50 /tmp/deploy.log'   # 反复轮询日志
  1panel-cli exec 'pgrep -f deploy.sh'            # 退出码 0=还在跑
  ```

## 4. 实现步骤

### 4.1 依赖(`cli/Cargo.toml`)

```toml
tungstenite = { version = "0.24", features = ["rustls-tls-native-roots"] }
rustls = { version = "0.23", features = ["dangerous_configuration", "ring"] }
```

- `tungstenite` 提供阻塞式 WS 客户端(与现有 `reqwest::blocking` 风格一致)。
- `--insecure` / 自签证书场景:用 `rustls::ClientConfig` 自定义 `ServerCertVerifier`(不做校验),经 `tungstenite::client_tls_with_config` 传入 `Connector::Rustls`。

### 4.2 新增 `cli/src/exec.rs`

函数划分:

- `fn ws_url(session: &Session, command: &str, cols: i64, rows: i64) -> Result<String>`
  - `session.panel_url` 的 `http:// → ws://`,`https:// → wss://`,拼 `/api/v2/hosts/terminal/local`(**无需拼接安全入口路径**)
  - 查询参数:`command`(percent-encoding)、`cols`、`rows`(纯数字;服务端默认 80x40,非数字会被拒绝)
- `fn build_handshake(session: &Session) -> Vec<(&str, String)>`
  - `Cookie`:拼接 `session.cookies` 全部 `name=value`(含 `psession`、`SecurityEntrance` 等)
  - `CurrentNode`:取 `--node` / `PANEL_NODE` / 默认 `local`(与 `client.rs:43` 一致)
- `fn ensure_local_conn(profile: &str, user: &str, pwd: &str, port: u16) -> Result<()>`
  - 通过 `POST api/v2/settings/ssh`(`SaveLocalConn`)写入本地 SSH 连接,请求体见 4.4(**password 需 base64 编码**)
- `fn compose_command(cwd: Option<&str>, command: &str, token: &str) -> Result<String>`
  - **预校验**:命令非空、不含 `\n` / `\r`(含则直接报错,不发起连接);超长(如 > 4000 字符)打印告警
  - `--cwd` 拼接:`cd -- '<dir>' && <command>`(dir 做 POSIX 单引号转义,防注入)
  - 追加哨兵:`<command>; echo __1PCLI_<token>_$?`
- `fn run(profile: &str, opts: &ExecOptions) -> Result<i32>`
  - 若 `opts.sync_ssh`,先读 `LINUX_SSH_USER` / `LINUX_SSH_PWD`(缺失则报错),调用 `ensure_local_conn` 后继续
  - 连接 WS → 读取 `TextMessage` → 反序列化 `WsMsg` → base64 解码 `data`;无法解析的文本帧按原始输出处理
  - **处理 `Close` 帧**:收到 `Message::Close(Some(frame))` 时,`frame.reason` 即服务端错误文本(如 SSH 未配置/凭据错误),按 §4.6 上报(见 §2.3);**意外断连**(无 reason 的 Close / IO 错误)时先 flush 已收集输出再报错退出 1
  - (可选)长命令期间周期发送 `heartbeat` 帧保活,服务端原样回显,可忽略回显
  - 累积到 `Vec<u8>`(**默认上限 10MB,超出截断并标记**;打印用 `String::from_utf8_lossy`,防二进制/非 UTF-8 输出 panic)
  - 哨兵匹配在**归一化流**(剥离 ANSI 转义与 `\r\n`)上做正则 `__1PCLI_<token>_(\d{1,3})`,**单次命中即完成**,捕获组即命令退出码(见 §2.5)
  - `--timeout` 用全局 deadline(`Instant::now() + Duration`)兜底(默认 30s,可按需调大;超时打印已收集内容、返回 124;主动关闭 WS 会终止远端命令,`nohup` 类可存活)
  - 完成后发送 Close 帧并返回;返回码语义见 §3
- 辅助:
  - `clean_output(raw, token)`:默认清洗——剥离 ANSI 转义、PTY 回显输入行(以 ` clear &&` 开头且含哨兵字面量 `$?` 的行)、哨兵输出行及其后的提示符残余;`--raw` 跳过
  - `tail_lines(cleaned, n)`:只保留最后 n 行并标注截断
  - `to_json(result)`:序列化 `{"exit_code","duration_ms","truncated","error","output"}`
  - 哨兵 token 用现有 `rand` crate 生成(8 字符 hex,无需新增依赖)
- 单元测试:
  - `ws_url` 对 http/https/带端口/自签 的 URL 构建正确
  - `WsMsg` JSON 反序列化与 base64 解码
  - 哨兵判定:回显行(字面量 `$?`)不误判、真实输出(数字)单次命中、折行/ANSI 拆断的归一化用例、0 次命中(超时路径)
  - `compose_command`:cwd 单引号转义(含空格/引号的目录)、预校验拒绝换行与空命令
  - `clean_output` / `tail_lines` / `to_json` 用例
  - `ensure_local_conn` 的请求体构建(不真正发请求)

### 4.3 配置读取(`cli/src/config.rs`)

- 新增常量 `ENV_LINUX_SSH_USER = "LINUX_SSH_USER"`、`ENV_LINUX_SSH_PWD = "LINUX_SSH_PWD"`。
- 在 `ConfigOverrides` / `PanelConfig` 中新增可选字段 `linux_ssh_user` / `linux_ssh_pwd`,用现有 `env_get` 读取(与 `PANEL_URL` 等一致,`.env` 会自动加载)。
- 同步把 `LINUX_SSH_USER` / `LINUX_SSH_PWD` 两个占位符加到 `.env.example`,注释说明用途(仅 `exec --sync-ssh` 使用)。
- **注意 `.env.example` 现状(2026-08 复核)**:仓库根的 `.env.example` 当前处于**已删除未提交**状态(`git status` 为 `D .env.example`),但发布流程 `.github/workflows/release.yml:76-95` 依赖它打包自检;仓库内另有 `cli/.env.example`(同样需要加占位符)。落地本功能时需恢复根目录 `.env.example` 并在两处同步占位符,否则 release 会失败。本地 `.env` 已包含这两个变量。`.gitignore` 只忽略 `.env`,`.env.example` 应当入库。

### 4.4 本地 SSH 连接自动配置(`--sync-ssh`)

面板"本地 SSH 连接"保存在 agent 设置 `LocalSSHConn`(`agent/app/service/setting.go:297`),`hosts/terminal/local` 依赖它(`agent/app/service/setting.go:328`)。REST 接口(路由注册见 `agent/router/ro_setting.go:39-43`):

- 写入:`POST api/v2/settings/ssh`,请求体(`dto.SSHConnData`,`agent/app/dto/setting.go:79`)。**`password` 必须先 base64 编码**——`SaveConnInfo`(`agent/app/service/setting.go:241`)会先 `base64.StdEncoding.DecodeString` 再用明文拨号(key 模式的 `privateKey` / `passPhrase` 同理):
  ```json
  {
    "addr": "127.0.0.1",
    "port": 22,
    "user": "<LINUX_SSH_USER>",
    "authMode": "password",
    "password": "<base64(LINUX_SSH_PWD)>"
  }
  ```
- **保存即校验**:`SaveConnInfo` 在写库前会真实 `gossh.Dial`(`agent/utils/ssh/ssh.go` `NewClient`,超时 5s)验证连通性,SSH 密码错误时该 POST 直接返回错误——凭据校验已内建,无需额外预检。
- (可选)预检:`POST api/v2/settings/ssh/check/info`(`CheckLocalConnByInfo`)用**同样的请求体**(password 同样 base64)先测连。注意 `POST api/v2/settings/ssh/check`(`CheckLocalConn`)测的是**已保存**的连接,不能用于写入前校验。
- (可选)探测是否已配置:`GET api/v2/settings/ssh/conn`(`LoadLocalConn`)返回 `addr` 为空即未配置,可给出比 WS 报错更友好的前置提示;响应中 `password`/`privateKey`/`passPhrase` 为 base64 回显,**勿打印**。
- 复用现有 `PanelClient.post_json`(`cli/src/client.rs:116`),走会话 Cookie + CSRF(POST 需要 `X-CSRF-Token`,已有实现),路径 `api/v2/settings/ssh`。

### 4.5 注册子命令(`cli/src/main.rs`)

- `mod exec;`
- `Commands` 增加 `Exec { command: String, timeout: i64, cols: i64, rows: i64, json: bool, tail: Option<usize>, raw: bool, cwd: Option<String>, sync_ssh: bool, ssh_port: u16 }`
- 分发:`Commands::Exec { .. } => std::process::exit(exec::run(...))`(需直接以退出码结束)

### 4.6 错误处理

| 场景 | 表现 |
|---|---|
| 会话失效(握手返回 401,可能是 JSON 或 HTML) | `会话已失效,请重新运行 login` |
| 面板维护中(SystemStatus ≠ Free,握手 407) | 提示面板正在升级/维护,稍后重试 |
| 命令含换行/为空/`--cwd` 路径非法(预校验) | 直接报错返回 1,不发起连接 |
| 本地 SSH 连接未配置且未传 `--sync-ssh` | WS 升级成功后收到 **Close 帧**,reason 为 `failed to set up the connection. Please check the host information: no such ssh conn info in db`,提示「可用 --sync-ssh + LINUX_SSH_USER/LINUX_SSH_PWD 自动配置,或到面板『设置 → 终端 → SSH 本地连接』配置」 |
| 本地 SSH 凭据错误(已配置但密码已变) | 同上 Close 帧,reason 含 SSH 拨号错误(如 `ssh: unable to authenticate`);提示可重跑 `--sync-ssh` 覆盖更新 |
| `--sync-ssh` 但缺少 `LINUX_SSH_USER` / `LINUX_SSH_PWD` | 报错:请在 .env 中配置 LINUX_SSH_USER / LINUX_SSH_PWD |
| `--sync-ssh` 写入失败(如密码错误,`SaveConnInfo` 真实拨号校验不通过) | 报 `settings/ssh` 返回的错误,不继续执行 |
| 命令完成但退出码非 0 | 照常打印清洗后输出(或 `--json`),CLI 以该退出码退出 |
| 超时未见哨兵 | 打印已收集内容 + 返回码 124;关闭 WS 会终止远端命令(`nohup`/后台化的可存活) |
| 意外断连(无 reason 的 Close / IO 错误,如 agent 重启) | 先 flush 已收集输出,再报错返回 1 |
| 连接失败 / TLS 校验失败 | `anyhow` 报错信息 |

## 5. 已知边界与风险

1. **前置条件**:`hosts/terminal/local` 依赖面板已保存"本地 SSH 连接"(`agent/app/service/setting.go:328` `GetLocalConnForSSH` 在 `Addr` 为空时报 `no such ssh conn info in db`)。`exec` 可用 `--sync-ssh` 结合 `LINUX_SSH_USER` / `LINUX_SSH_PWD` 自动写入;否则需先到面板 UI 配置。
2. **单行命令**:命令以整行传给 shell,含换行会破坏 shell 解析;多行脚本列为后续增强(可改为连接后通过 `cmd` 帧发送命令,规避 URL 长度限制)。
3. **输出默认清洗**(见 3.1):剥离 ANSI 转义、PTY 回显行、哨兵行与提示符残余;`--raw` 保留原始 PTY 流(含 ` clear &&<cmd>` 回显、`clear` 清屏序列等)。清洗是启发式的,极端 PS1 场景可能残留杂项。
4. **哨兵启发式**:匹配"token+数字"形态(回显中的字面量 `$?` 不参与判定),且必须在归一化流(剥离 ANSI/`\r\n`)上匹配,否则 PTY 折行可能拆断哨兵(见 §2.5)。若用户命令恰好输出同形随机串(概率可忽略)或 shell 被中断,交由超时兜底。
5. **版本差异**:已用 `git diff v2.2.5 origin/dev-v2` 复核,`core/utils/terminal/ws_session.go`、`agent/app/api/v2/terminal.go`、设置服务及 `/hosts/terminal/*` 路由在两版本**无差异**(dev-v2 仅改防火墙路由),终端方案对两版本通用;仍可实测以 `1panel-cli api list --filter terminal` 复核。
6. **面板全局中间件的边缘拦截**:`BindDomain` 设置了绑定域名时,用 127.0.0.1 访问会被拒(err_domain HTML);demo 模式(`IsDemo`)下终端被禁用;`GlobalLoading` 在面板升级中返回 407。CLI 常规本机部署不受影响,但错误信息要能区分这些情况。
7. **凭据安全**:`LINUX_SSH_PWD` 只在 `--sync-ssh` 时经 `settings/ssh` 发给面板(body 中为 base64,非加密);`settings/ssh/conn` 响应中的密码以 base64 返回,勿打印到日志。`.env` 含真实凭据,已被 `.gitignore` 忽略。
8. **交互式命令会挂到超时**:`sudo` 要密码、`top`/`vim` 等 TUI 会一直无输出直到 `--timeout` 到期。指引:用非交互参数(`-y`、`</dev/null`)、配置免密 sudo;长任务用 3.1 的轮询模式,不要靠拉大 `--timeout` 硬等。

## 6. 验证

- `cargo build`(在 `cli/` 目录)
- 单元测试:`cargo test`(URL 构建 / WsMsg 解析 / 哨兵判定与退出码解析 / 命令组装与预校验 / 清洗与截断 / 请求体构建)
- 手工冒烟(使用本机已安装的 1Panel 服务,配置见 0.2):
  ```sh
  # 用 .env 中的面板凭据登录
  1panel-cli login
  1panel-cli status
  # 先自动配置本地 SSH 连接再执行
  1panel-cli exec 'echo hello && hostname' --sync-ssh
  1panel-cli exec 'df -h && free -m'
  # 切目录执行脚本 + 超时语义
  1panel-cli exec 'bash /tmp/demo.sh' --cwd /tmp --timeout 120
  1panel-cli exec 'sleep 5; echo done' --timeout 3   # 应超时返回 124
  # 退出码透传与结构化输出
  1panel-cli exec 'exit 7' --json; echo "exit=$?"     # json 含 exit_code:7,CLI 退出码 7
  1panel-cli exec 'ls /nonexistent' >/dev/null; echo "exit=$?"  # 非零透传
  # 轮询模式(长任务)
  1panel-cli exec 'nohup bash /tmp/slow.sh > /tmp/slow.log 2>&1 &'
  1panel-cli exec 'tail -n 5 /tmp/slow.log' --tail 20
  # 复核终端接口路由在目标版本存在
  1panel-cli api list --filter terminal
  ```

## 7. 文档落盘

- `commands.md`:新增 `exec` 一节
- `SKILL.md`:常用命令区补充 `exec` 示例与前置条件提示
- `references/compatibility-dev-v2.md`:如需,记录终端 WS 接口在 v2.2.5 / dev-v2 的可用性
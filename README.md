# tech-1panel-cli — 1Panel 面板管理 Skill + CLI

通过命令行工具 `1panel-cli` 管理 1Panel 面板的 Agent Skill(支持 opencode /
Claude Code 等遵循 Agent Skills 标准的工具),附带 Rust 编写的 CLI 源码与
GitHub Actions 自动构建发布。

## 仓库结构

```
tech-1panel-cli/
├── SKILL.md               # 技能主体(触发条件、环境检查、命令速查、关键约定)
├── commands.md            # 完整命令速查表(会话 / apps / db / redis / web / firewall / api)
├── .env.example           # 面板账号配置模板
├── cli/                   # 1panel-cli Rust 源码
│   ├── src/               #   源码模块
│   ├── Cargo.toml
│   ├── README.md          #   CLI 使用文档
│   └── .env.example
└── .github/workflows/
    └── release.yml        # 构建二进制 → 打包 1panel-skill.zip → 发布到 Releases
```

## 发布产物

GitHub Actions(打 `v*` tag 或手动触发)会构建 Linux x86_64 / aarch64 两种
二进制,并打包为符合规范结构的 `1panel-skill.zip`:

```
1panel-skill.zip
└── 1panel-skill/
    ├── SKILL.md
    ├── commands.md
    ├── README.md
    ├── .env.example       # 面板账号配置模板(根目录副本)
    └── bin/
        ├── .env.example   # 配置模板(复制为 .env 并填写,与二进制同目录)
        └── 1panel-cli     # 可执行文件
```

源码(`cli/` 目录)同样包含 `.env.example`。发布到仓库 Releases:
`1panel-skill.zip`(x86_64)、`1panel-skill-aarch64.zip`(aarch64)。

## 使用

1. 下载对应架构的 zip 并解压
2. 将 `SKILL.md` 所在目录放入 skill 路径(如 `~/.config/opencode/skills/1panel-cli/`)
3. 复制 `.env.example` 为 `.env` 并填写 PANEL_URL / PANEL_USERNAME / PANEL_PASSWORD
4. 首次使用运行 `./bin/1panel-cli doctor` 检查环境

## 本地构建 CLI

```sh
cd cli
cargo build --release
# 产物: cli/target/release/1panel-cli
```

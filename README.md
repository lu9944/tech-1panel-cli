# 1panel-cli — 1Panel 面板管理 Skill

opencode / Claude Code 等支持 Agent Skills 标准的 Agent 专用技能,用于通过
`1panel-cli` 命令行工具管理 1Panel 面板。

## 内容

- `SKILL.md` — 技能主体(触发条件、环境检查、命令速查、关键约定)
- `commands.md` — 完整命令速查表(会话 / apps / db / redis / web / firewall / api)

## 安装

将本目录加入 opencode 的 skill 路径(`~/.config/opencode/opencode.jsonc`):

```jsonc
{
  "skills": {
    "paths": ["/path/to/tech-1panel-cli"]
  }
}
```

或复制到 `~/.config/opencode/skills/1panel-cli/`。

## 前置依赖

- 1Panel 面板(已安装 openresty 等)
- `1panel-cli` 二进制: `/root/code/1panel-cli/target/release/1panel-cli`
- 配置文件: `/root/code/1panel-cli/.env`

首次使用先执行 `1panel-cli doctor` 检查环境。

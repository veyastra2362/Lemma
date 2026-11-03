# Agent Note: 接入决策笔记体系（AGENTS.md + verify-notes）

Status: implemented

## Problem

`AGENTS.md` 已经有 94 行硬约束：命令、分层纪律、5 条踩坑、5 条不变式、安全边界。它们回答的是**现在必须怎样**，回答不了**当初为什么这样定、放弃了什么**。

两处典型缺口：

- 「构建与踩过的坑」第 6 条只写了「WebView2 的 IPC 有竞争，前端统一走 `withRetry`」，没写试过哪些别的做法（重试几次、放在哪一层重试、为什么不直接等 `__TAURI_INTERNALS__` 就绪）——想改这段的人只能重走一遍。
- 「只允许一个键盘钩子」在正文里给了理由，但备选方案（用 `RegisterHotKey` 硬凑双击 Ctrl、或注册两个钩子分担）只在结尾 blockquote 里一句带过，代价没摊开。

新会话读到规则能照做；**想推翻规则时看不到代价**，这正是笔记要补的那一层。

## Decision

接入**全局安装**的 `write-notes-like-deepseek` 技能（`C:\Users\ASUS\.agents\skills\write-notes-like-deepseek\`）：

- `AGENTS.md` 末尾追加「决策笔记」一节：四条规则 + 目录约定 + 全局 SKILL.md 路径 + 分工说明；
- `package.json` 增加 `verify-notes`（tree + format + archived 三线串跑）与 `archive-note`；
- **不装项目级副本**：全局那份已被 44 个 agent 目录共享，再来一份会迟早漂移；
- `AGENTS.md` 原有内容**一字未改**——它继续承担「硬约束」，笔记承担「理由与取舍」。

分工写进了 `AGENTS.md`：规则要改，先改笔记再改那里。

## Alternatives considered

- **把理由直接补进 `AGENTS.md`**：最强理由是不引入新目录、不开新流程，一份文件说清所有事，也不需要多跑一条命令。但 `AGENTS.md` 存在的意义是「精炼到能当规则用」，19 条约束每条都摊开备选与代价会膨胀到没人读；而且它是多分支高频冲突文件，理由写进去等于把冲突面扩大。最终只放一条指针。
- **项目级安装技能副本**：随仓库提交、路径相对、换机器不坏，是官方默认姿势。但本机已全局装好且被 44 个 agent 目录共享，第二份副本 23 个文件、约 120KB，两份迟早不同步；实例化时还遇到代理不可用导致克隆失败，维护成本已经显现。
- **只改 `AGENTS.md`、不加 `verify-notes`**：改动最小。但「提交前跑校验」没有固定入口，每个会话都得自己拼三条 `npx` 命令，实际会被跳过——这正是散文规矩失效的老路。
- **同步接 CI**：仓库虽有 `.github/`，但当前是单人本地开发，红绿灯没人看；等真的出现多人 PR 再复制 `templates/verify-notes.yml`。

## Consequences

- 收益：新会话启动即读到规则，不必每次口头提醒；「为什么」有了唯一落点，推翻旧决定前会先撞见当时的备选与代价。
- 代价 1：`AGENTS.md` 与 npm 脚本里含本机绝对路径（`C:\Users\ASUS\...`），换机器或改用户名要同步改两处；已在 `AGENTS.md` 注明换机器时按技能名 `write-notes-like-deepseek` 重新定位。
- 代价 2：`verify-notes` 首跑要用 `npx` 拉 `tsx`，离线时失败；Node ≥ 23.6 可直接 `node <script>.ts` 绕过。
- 代价 3：`AGENTS.md` 与笔记现在有内容重叠的风险（谁写「必须怎样」）。约定是 AGENTS.md 写规则、笔记写理由；这条约定本身也可能被后人不当回事。
- 遗留：未接 CI；`proposed` / `rejected` / `archived` 三个生命周期尚无用例，归档封印机制还没被真实跑过。

## Testing

- `pnpm verify-notes` 退出码 0：tree / format / archived 三线全过。
- `AGENTS.md` 里写的 `SKILL.md` 路径已核对存在。

# Agent Note: 项目更名 Trans → Lemma（含数据目录迁移）

Status: implemented

## Problem

`Trans` 是工程期的占位名，三处不合格：

1. **太泛**：GitHub 上同名项目成片，搜不到，也没法占住唯一标识；
2. **不体现核心**：它只说明"翻译"，而这个系统的骨架是**沿一个词往下钻、再退回来**；
3. **要公开了**：仓库即将 Public，名字会固化进安装包、`identifier`、二进制名、数据目录，越晚改代价越大。

改名本身不难，难的是 `identifier` 一改，Tauri 的 `app_data_dir()` 就从
`%APPDATA%\dev.lunar.trans` 变成 `%APPDATA%\dev.lunar.lemma`——用户已有的设置和
128 MB 词库留在旧目录里，表现是"设置全丢、词库没了"。

## Decision

采用 **Lemma（词元）**，并做全量改名 + 一次性数据迁移。

- 名字取自语言学 `lemma`：一个词所有屈折形式的词典原形。正好是这个系统的数据模型——
  点到的 `thresholds` / `went` 最终归到一个词元，再从词元继续下探。
- 改动面：`productName`、`identifier`、Cargo 包名（`lemma` / `lemma_lib`）、二进制名
  （`lemma.exe`）、npm 包名、窗口标题、托盘文案、CLI 帮助、性能脚本、文档。
- **`core::paths` 增加 `IDENTIFIER` 常量**，与 `tauri.conf.json` 的 `identifier` 必须一致：
  CLI（无 Tauri）与 GUI 各自算路径，不同源就会出现"GUI 写 A、CLI 读 B"的诡异分叉。
  为此加了单测断言词库路径包含 `IDENTIFIER`。
- **启动时一次性迁移**：只搬 `settings.json` 与 `dict.redb` 两个已知文件
  （`rename` 失败退回 `copy`），不整目录拷贝；目标已存在则跳过，绝不覆盖。
- 迁移写在 `logs::init` 之后、其它初始化之前，结果进日志（`migrate <file>: ok/failed`），
  失败可诊断而不是静默。

## Alternatives considered

- **保留 `Trans`，只改显示名不动 `identifier`**：最强理由是零迁移风险——用户数据原地不动，
  也没有"搬一半失败"的可能；实现成本几乎为零。否决原因：`identifier` 会永久固化进安装包
  与数据目录，现在不改，以后每个用户升级时都要付一次迁移代价，而且仓库已经叫 Lemma，
  代码里叫 Trans 会让后来者反复困惑"这是不是两个东西"。
- **`Ferry`（摆渡）/ `Retrace`（溯）/ `Inkle`（落墨）**：`Ferry` 语义最直白（在两种语言间摆渡），
  `Retrace` 最贴合"退层"这个动作。否决原因：`Ferry` 有同名项目、`Inkle` 有同名公司，
  而 `Lemma` 唯一、短、且与数据模型（词元归一）同构。
- **改名但不迁移，让用户自己重新导入词库**：实现最省事，导入也只要 3 秒。否决原因：
  `settings.json` 里的触发方式/热键/卡片配置无法"重新导入"，用户会实打实丢配置，
  而且丢得莫名其妙——这种"改名顺手清空设置"的事故一旦发生，信任成本远高于几十行迁移代码。
- **整目录 `rename` 一次性搬走**：代码最少。否决原因：旧目录里可能还留着 `probe/`、`logs/`、
  半截下载缓存，整体搬动会把无关文件一起带进新目录，也会让"搬一半失败"的恢复变得不可推理。

## Consequences

- 收益：项目标识唯一且与数据模型同构；CLI 与 GUI 的路径来源收敛到一个常量，后续再改标识只改两处。
- 代价 1：迁移是**一次性且不可逆**的（`rename` 会把文件从旧目录移走）。用户若回退到旧版本，
  旧版本会读不到设置——已在迁移里选择"只搬两个已知文件"，把影响面压到最小。
- 代价 2：旧目录会残留空壳（`logs/`、`probe/`），需要用户手动删；暂不做清理，避免误删。
- 代价 3：`identifier` 变更意味着新旧版本**不能共存**（同一台机器上是两个不同的应用）。
- 遗留：安装包名与 GitHub Release 命名规则（`Lemma_0.1.0_x64-setup.exe`）尚未与实际发布内容对齐。

## Testing

- `cargo test`：55 项通过，含 `paths::tests::dict_lives_under_the_current_identifier`
  （断言词库路径包含 `IDENTIFIER`）与 `legacy_dir_differs_from_the_current_one`。
- 首次以新名启动的日志应出现：
  ```
  migrate settings.json: ok
  migrate dict.redb: ok
  dict loaded 770611 entries
  ```
- `lemma.exe --lookup threshold` 能命中，说明 CLI 路径与迁移后的目录一致。

/**
 * 极简文案层。
 *
 * M0/M1 只有中文一种语言，所以先不引 i18next：这里只是把文案收口，
 * 等真的需要第二语言时把 `t()` 换成 i18next 即可，调用点不用动。
 */
const zhCN = {
  appName: "Trans",
  hudTagline: "高频深度阅读辅助",
  hudEmptyTitle: "取词链路未接入",
  hudEmptyBody: "M1 会在这里显示选区原文、词性与释义。现在先验证呼出链路与延迟预算。",
  hudHintEscape: "Esc 关闭",
  hudHintSettings: "托盘图标 → 设置",
  hudTriggerDoubleControl: "双击 Ctrl 呼出 / 收起",
  hudTriggerHotkeySuffix: "呼出 / 收起",
  hudTriggerUnavailable: "触发方式未就绪",
  hudCapturePlaceholder: "选中文字，或把鼠标停在词上，再双击 Ctrl 取词",
  hudCaptureNoResult: "没取到文本",
  hudCaptureFrom: "来自",
  latency: "热键 → 首帧",
  settingsTitle: "设置",
  settingsSectionTrigger: "呼出方式",
  settingsTriggerDoubleControl: "双击 Ctrl",
  settingsTriggerDoubleControlHint: "推荐。不占用组合键，也不和输入法冲突。",
  settingsTriggerHotkey: "自定义热键",
  settingsHotkeyRecording: "按下组合键…（Esc 取消）",
  settingsHotkeyRecord: "点击录制",
  settingsHotkeyRule: "至少包含 Ctrl / Alt 之一",
  settingsHotkeySaveFailed: "保存失败",
  cardNotInDict: "词库未收录",
  settingsSectionCapture: "取词模式",
  settingsCaptureAuto: "自动（选区优先，其次光标词）",
  settingsCaptureAutoHint: "推荐。有选区就读选区，没有就取光标下的词。",
  settingsCaptureSelection: "只用选区",
  settingsCaptureSelectionHint:
    "浏览器推荐：实测选区 8ms 且稳定，悬停取词在 Chrome 上不稳定。",
  settingsCaptureHover: "只用光标词",
  settingsCaptureHoverHint: "原生应用里悬停即查；浏览器上不可靠。",
  settingsSectionCard: "卡片显示内容",
  settingsSectionSize: "HUD 最大高度",
  settingsSectionPerf: "性能基线",
  settingsStartup: "冷启动到托盘就绪",
  settingsPresents: "累计呼出次数",
  settingsLastPaint: "最近一次呼出延迟",
  settingsSectionRoadmap: "里程碑",
  noData: "暂无数据",
} as const;

export type MessageKey = keyof typeof zhCN;

/** 深度下探时被折叠的层数提示（带参数，所以不放进字典键里）。 */
export function tDeckCollapsed(count: number): string {
  return `上面还有 ${count} 层`;
}

export function t(key: MessageKey): string {
  return zhCN[key];
}

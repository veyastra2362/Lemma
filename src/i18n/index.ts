/**
 * 极简文案层。
 *
 * M0 只有中文一种语言，所以先不引 i18next：这里只是把文案收口，
 * 等真的需要第二语言时把 `t()` 换成 i18next 即可，调用点不用动。
 */
const zhCN = {
  appName: "Trans",
  hudTagline: "高频深度阅读辅助",
  hudEmptyTitle: "取词链路未接入",
  hudEmptyBody: "M1 会在这里显示选区原文、词性与释义。现在先验证呼出链路与延迟预算。",
  hudHintHotkeyUnavailable: "热键全部被占用",
  hudHintEscape: "Esc 关闭",
  hudHintSettings: "托盘图标 → 设置",
  legacyLatency: "热键 → 首帧",
  settingsTitle: "设置",
  settingsSectionPerf: "性能基线",
  settingsHotkey: "当前呼出热键",
  settingsStartup: "冷启动到托盘就绪",
  settingsPresents: "累计呼出次数",
  settingsLastPaint: "最近一次呼出延迟",
  settingsSectionRoadmap: "里程碑",
  becauseNoData: "暂无数据",
} as const;

export type MessageKey = keyof typeof zhCN;

export function t(key: MessageKey): string {
  return zhCN[key];
}

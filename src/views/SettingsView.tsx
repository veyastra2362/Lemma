import { useCallback, useEffect, useState } from "react";

import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t } from "../i18n";
import { ipc, isTauri } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { CaptureMode, CardSections, ModelStatus, Settings } from "../types";
import { formatHotkey } from "../utils/hotkey";

const CAPTURE_OPTIONS: { value: CaptureMode; label: string; hint: string }[] = [
  { value: "auto", label: t("settingsCaptureAuto"), hint: t("settingsCaptureAutoHint") },
  {
    value: "selectionOnly",
    label: t("settingsCaptureSelection"),
    hint: t("settingsCaptureSelectionHint"),
  },
  {
    value: "hoverOnly",
    label: t("settingsCaptureHover"),
    hint: t("settingsCaptureHoverHint"),
  },
];

const SECTION_OPTIONS: { key: keyof CardSections; label: string }[] = [
  { key: "phonetic", label: "音标" },
  { key: "translation", label: "中文释义" },
  { key: "definition", label: "英文释义" },
  { key: "tags", label: "考试标签" },
  { key: "exchange", label: "词形变化" },
];

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-white/10 px-3 py-2">
      <div className="text-[11px] text-[var(--hud-muted)]">{label}</div>
      <div className="mt-0.5 font-mono text-[15px]">{value}</div>
    </div>
  );
}

/** 文本输入：本地草稿，失焦或回车才提交，避免每敲一个字符就写盘 + 打网络。 */
function TextField({
  label,
  hint,
  value,
  onCommit,
  secret,
}: {
  label: string;
  hint?: string;
  value: string;
  onCommit: (next: string) => void;
  secret?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);

  const commit = () => {
    if (draft !== value) {
      onCommit(draft);
    }
  };

  return (
    <label className="block rounded-lg border border-white/10 px-3 py-2">
      <span className="block text-[11px] text-[var(--hud-muted)]">{label}</span>
      <input
        type={secret ? "password" : "text"}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            commit();
          }
        }}
        className="mt-1 w-full bg-transparent font-mono text-[12px] text-[var(--hud-text)] outline-none placeholder:text-[var(--hud-muted)]/50"
        placeholder={hint}
      />
    </label>
  );
}

function Radio({
  active,
  label,
  hint,
  onClick,
}: {
  active: boolean;
  label: string;
  hint?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-start gap-2 rounded-lg border px-3 py-2 text-left transition ${
        active
          ? "border-[var(--hud-accent)] bg-white/5"
          : "border-white/10 hover:border-white/25"
      }`}
    >
      <span
        className="mt-1 size-2 shrink-0 rounded-full"
        style={{
          background: active ? "var(--hud-accent)" : "oklch(1 0 0 / 20%)",
        }}
      />
      <span className="min-w-0">
        <span className="block text-[13px]">{label}</span>
        {hint && (
          <span className="mt-0.5 block text-[11px] leading-snug text-[var(--hud-muted)]">
            {hint}
          </span>
        )}
      </span>
    </button>
  );
}

export function SettingsView() {
  useRuntimeInfo();
  const stats = useAppStore((state) => state.stats);
  const setStats = useAppStore((state) => state.setStats);
  const latencyMs = useAppStore((state) => state.latencyMs);
  const settings = useAppStore((state) => state.settings);
  const setSettings = useAppStore((state) => state.setSettings);

  const [recordingTarget, setRecordingTarget] = useState<null | "hud" | "reader" | "ocr">(null);
  const [models, setModels] = useState<ModelStatus | null>(null);
  const recording = recordingTarget !== null;
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc.perfStats().then(setStats).catch(() => undefined);
  }, [setStats]);

  /** 改一项、其余保持不变。失败时 Rust 已回滚，这里只负责显示原因。 */
  const apply = useCallback(
    (patch: Partial<Settings>) => {
      if (!settings) {
        return;
      }
      setError(null);
      const next: Settings = { ...settings, ...patch };
      // 乐观更新：界面立刻反映，等待 Rust 的结果再校正。
      setSettings(next);
      void ipc
        .updateSettings(next)
        .then(setSettings)
        .catch((reason: unknown) => {
          setError(reason instanceof Error ? reason.message : String(reason));
          void ipc.getSettings().then(setSettings).catch(() => undefined);
        });
    },
    [settings, setSettings],
  );

  // 模型状态：装没装、装在哪
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .modelStatus()
      .then(setModels)
      .catch(() => setModels(null));
  }, []);

  // Esc 关闭设置窗口（录制热键时由录制逻辑优先处理）。
  useEffect(() => {
    if (recording) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && isTauri()) {
        event.preventDefault();
        void ipc.closeSettings();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [recording]);

  // 录制期间用捕获阶段拦下按键，避免组合键被页面的其它处理器吃掉。
  useEffect(() => {
    if (!recordingTarget) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecordingTarget(null);
        return;
      }
      const combo = formatHotkey(event);
      if (!combo) {
        setError(t("settingsHotkeyRule"));
        return;
      }
      if (recordingTarget === "ocr") {
        setRecordingTarget(null);
        apply({ ocrHotkey: combo });
      } else if (recordingTarget === "reader") {
        setRecordingTarget(null);
        apply({ readerHotkey: combo });
      } else {
        setRecordingTarget(null);
        apply({ trigger: "hotkey", hotkey: combo });
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recordingTarget, apply]);

  const startup = stats ? `${stats.startup_ms.toFixed(0)} ms` : t("noData");
  const presents = stats ? String(stats.hud_presents) : t("noData");
  const lastPaint =
    latencyMs !== null
      ? `${latencyMs.toFixed(1)} ms`
      : stats?.last_first_paint_ms != null
        ? `${stats.last_first_paint_ms.toFixed(1)} ms`
        : t("noData");

  const isDoubleControl = settings?.trigger === "doubleControl";
  const sections = settings?.cardSections;

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-xl border text-[var(--hud-text)]"
      style={{ borderColor: "var(--hud-border)", background: "var(--hud-surface)" }}
    >
      <header
        data-tauri-drag-region
        className="flex shrink-0 items-center gap-2 border-b border-white/8 px-4 py-2.5"
      >
        <span className="text-[13px] font-medium">
          {t("appName")} · {t("settingsTitle")}
        </span>
        <button
          type="button"
          onClick={() => void ipc.closeSettings()}
          title={t("settingsClose")}
          aria-label={t("settingsClose")}
          className="ml-auto grid size-6 place-items-center rounded-md text-[var(--hud-muted)] transition hover:bg-white/10 hover:text-[var(--hud-text)]"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path
              d="M2 2 L10 10 M10 2 L2 10"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
            />
          </svg>
        </button>
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {/* ---- 呼出方式 ---- */}
        <section>
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionTrigger")}
          </h2>
          {/*
            合成一张卡片：默认就是「双击 Ctrl」，录制热键只是可选的覆盖。
            之前是两个互斥的选项卡，看起来像"必须二选一"，而绝大多数人只用默认那个。
          */}
          <div
            className={`rounded-lg border px-3 py-2 ${
              isDoubleControl
                ? "border-[var(--hud-accent)] bg-white/5"
                : "border-white/10"
            }`}
          >
            <div className="flex items-center gap-2">
              <span
                className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                  isDoubleControl ? "bg-[var(--hud-accent)]" : "bg-white/25"
                }`}
              />
              <span className="text-[13px]">{t("settingsTriggerDoubleControl")}</span>
              <span className="rounded bg-white/8 px-1.5 py-0.5 text-[10px] text-[var(--hud-muted)]">
                {isDoubleControl ? t("settingsTriggerDefault") : t("settingsTriggerHotkeyActive")}
              </span>
              {/* 默认模式下左边已经写了「双击 Ctrl」，这里不再重复；只在改用热键时显示当前键位 */}
              {!isDoubleControl && (
                <span className="ml-auto font-mono text-[12px] text-[var(--hud-accent)]">
                  {settings?.hotkey ?? "—"}
                </span>
              )}
              {isDoubleControl && <span className="ml-auto" />}
              <button
                type="button"
                onClick={() => {
                  setError(null);
                  setRecordingTarget(recordingTarget === "hud" ? null : "hud");
                }}
                className="shrink-0 rounded-md border border-white/15 px-2 py-0.5 text-[11px] hover:border-white/35"
              >
                {recordingTarget === "hud"
                  ? t("settingsHotkeyRecording")
                  : t("settingsHotkeyRecord")}
              </button>
              {!isDoubleControl && (
                <button
                  type="button"
                  onClick={() => {
                    setError(null);
                    apply({ trigger: "doubleControl" });
                  }}
                  className="shrink-0 rounded-md border border-white/15 px-2 py-0.5 text-[11px] hover:border-white/35"
                >
                  {t("settingsTriggerResetDefault")}
                </button>
              )}
            </div>
            <div className="mt-1 pl-3.5 text-[11px] leading-relaxed text-[var(--hud-muted)]">
              {isDoubleControl
                ? t("settingsTriggerDoubleControlHint")
                : t("settingsHotkeyRule")}
            </div>
          </div>
        </section>

        {/* ---- 取词模式 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionCapture")}
          </h2>
          <div className="space-y-1.5">
            {CAPTURE_OPTIONS.map((option) => (
              <Radio
                key={option.value}
                active={settings?.captureMode === option.value}
                label={option.label}
                hint={option.hint}
                onClick={() => apply({ captureMode: option.value })}
              />
            ))}
          </div>
        </section>

        {/* ---- 本地模型 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionModels")}
          </h2>
          <p className="mb-2 text-[11px] leading-relaxed text-[var(--hud-muted)]">
            {t("settingsModelsHint")}
          </p>
          <div className="space-y-1.5">
            {[
              { kind: "ocr" as const, label: t("settingsModelOcr"), ready: models?.ocrReady, dir: models?.ocrDir },
              { kind: "tts" as const, label: t("settingsModelTts"), ready: models?.ttsReady, dir: models?.ttsDir },
            ].map((row) => (
              <div
                key={row.kind}
                className="rounded-lg border border-white/10 px-3 py-2"
              >
                <div className="flex items-center gap-2">
                  <span className="text-[12px]">{row.label}</span>
                  <span
                    className={`rounded px-1.5 py-0.5 text-[10px] ${
                      row.ready
                        ? "bg-emerald-500/15 text-emerald-300"
                        : "bg-amber-500/15 text-amber-300"
                    }`}
                  >
                    {row.ready ? t("settingsModelReady") : t("settingsModelMissing")}
                  </span>
                </div>
                <div className="mt-1 flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-[var(--hud-muted)]">
                    {row.dir ?? ""}
                  </span>
                <button
                  type="button"
                  onClick={() => {
                    setError(null);
                    void ipc.openModelDir(row.kind).catch((reason: unknown) => {
                      setError(reason instanceof Error ? reason.message : String(reason));
                    });
                  }}
                  className="shrink-0 rounded-md border border-white/15 px-2 py-0.5 text-[11px] hover:border-white/35"
                >
                  {t("settingsModelOpen")}
                </button>
                </div>
              </div>
            ))}
          </div>
        </section>

        {/* ---- 朗读 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionSpeech")}
          </h2>
          <div className="rounded-lg border border-white/10 px-3 py-2">
            <div className="flex items-center justify-between">
              <span className="text-[12px]">{t("settingsSpeechSpeed")}</span>
              <span className="font-mono text-[12px] text-[var(--hud-accent)]">
                {(settings?.speechSpeed ?? 1).toFixed(2)}×
              </span>
            </div>
            <input
              type="range"
              min={0.6}
              max={1.4}
              step={0.05}
              value={settings?.speechSpeed ?? 1}
              onChange={(event) =>
                apply({ speechSpeed: Number(event.target.value) })
              }
              className="mt-2 w-full accent-[var(--hud-accent)]"
            />
            <p className="mt-1 text-[11px] text-[var(--hud-muted)]">
              {t("settingsSpeechSpeedHint")}
            </p>
          </div>
        </section>

        {/* ---- 朗读按钮 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionSpeakButtons")}
          </h2>
          <div className="space-y-1.5">
            {[
              { key: "speakOriginal" as const, label: t("settingsSpeakOriginal") },
              { key: "speakTranslation" as const, label: t("settingsSpeakTranslation") },
            ].map((row) => (
              <label
                key={row.key}
                className="flex cursor-pointer items-center gap-2 rounded-lg border border-white/10 px-3 py-2"
              >
                <input
                  type="checkbox"
                  checked={settings?.[row.key] ?? true}
                  onChange={(event) => {
                    setError(null);
                    apply({ [row.key]: event.target.checked });
                  }}
                  className="h-3.5 w-3.5 accent-[var(--hud-accent)]"
                />
                <span className="text-[12px]">{row.label}</span>
              </label>
            ))}
          </div>
        </section>

        {/* ---- 截图识别 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionOcr")}
          </h2>
          <div className="space-y-1.5">
            <Radio
              active={settings?.ocrHotkeyEnabled !== false}
              label={t("settingsOcrToggle")}
              hint={t("settingsOcrToggleHint")}
              onClick={() => apply({ ocrHotkeyEnabled: true })}
            />
            <Radio
              active={settings?.ocrHotkeyEnabled === false}
              label={t("settingsReaderOff")}
              hint={t("settingsOcrToggleOffHint")}
              onClick={() => apply({ ocrHotkeyEnabled: false })}
            />
            {settings && settings.ocrHotkeyEnabled !== false && (
              <div className="flex items-center gap-2 rounded-lg border border-white/10 px-3 py-2">
                <span className="text-[12px] text-[var(--hud-muted)]">
                  {t("settingsOcrHotkeyLabel")}
                </span>
                <span className="ml-auto font-mono text-[12px] text-[var(--hud-accent)]">
                  {settings.ocrHotkey}
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setError(null);
                    setRecordingTarget(recordingTarget === "ocr" ? null : "ocr");
                  }}
                  className="rounded-md border border-white/15 px-2 py-0.5 text-[11px] hover:border-white/35"
                >
                  {recordingTarget === "ocr"
                    ? t("settingsHotkeyRecording")
                    : t("settingsHotkeyRecord")}
                </button>
              </div>
            )}
          </div>
        </section>

        {/* ---- 截图对照窗口（原「双栏」） ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionReader")}
          </h2>
          <p className="mb-2 text-[11px] leading-relaxed text-[var(--hud-muted)]">
            {t("settingsReaderIntro")}
          </p>
          <div className="space-y-1.5">
            <Radio
              active={settings?.readerOpenMode === "off"}
              label={t("settingsReaderOff")}
              hint={t("settingsReaderOffHint")}
              onClick={() => apply({ readerOpenMode: "off" })}
            />
            <Radio
              // 旧的 hotkey 模式已移除（那是"取词后开纯文本双栏"，与 HUD 重复），
              // 存量配置里是 hotkey 的一律按"打开"处理
              active={settings?.readerOpenMode !== "off"}
              label={t("settingsReaderHud")}
              hint={t("settingsReaderHudHint")}
              onClick={() => apply({ readerOpenMode: "hud" })}
            />
          </div>
        </section>

        {/* ---- 翻译 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionTranslate")}
          </h2>
          <div className="space-y-1.5">
            <Radio
              active={settings?.translateProvider === "google"}
              label={t("settingsTranslateGoogle")}
              hint={t("settingsTranslateGoogleHint")}
              onClick={() => apply({ translateProvider: "google" })}
            />
            <Radio
              active={settings?.translateProvider === "openAi"}
              label={t("settingsTranslateOpenai")}
              hint={t("settingsTranslateOpenaiHint")}
              onClick={() => apply({ translateProvider: "openAi" })}
            />
          </div>

          <div className="mt-2 space-y-1.5">
            <div className="flex gap-1.5">
              {(
                [
                  { value: "zh" as const, label: t("settingsTargetZh") },
                  { value: "en" as const, label: t("settingsTargetEn") },
                ]
              ).map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => apply({ targetLang: option.value })}
                  className={`rounded-md border px-3 py-1 text-[12px] transition ${
                    settings?.targetLang === option.value
                      ? "border-[var(--hud-accent)] text-[var(--hud-accent)]"
                      : "border-white/12 text-[var(--hud-muted)] hover:border-white/30"
                  }`}
                >
                  {option.label}
                </button>
              ))}
            </div>

            {settings?.translateProvider === "openAi" && settings && (
              <div className="space-y-1.5">
                <TextField
                  label={t("settingsOpenaiBaseUrl")}
                  hint="https://api.openai.com/v1"
                  value={settings.openaiBaseUrl}
                  onCommit={(next) => apply({ openaiBaseUrl: next })}
                />
                <TextField
                  label={t("settingsOpenaiKey")}
                  hint={t("settingsOpenaiKeyHint")}
                  value={settings.openaiApiKey}
                  secret
                  onCommit={(next) => apply({ openaiApiKey: next })}
                />
                <TextField
                  label={t("settingsOpenaiModel")}
                  hint="gpt-4o-mini"
                  value={settings.openaiModel}
                  onCommit={(next) => apply({ openaiModel: next })}
                />
              </div>
            )}
          </div>
        </section>

        {/* ---- 卡片内容 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionCard")}
          </h2>
          <div className="flex flex-wrap gap-1.5">
            {SECTION_OPTIONS.map((option) => {
              const active = sections?.[option.key] ?? false;
              return (
                <button
                  key={option.key}
                  type="button"
                  onClick={() =>
                    sections &&
                    apply({
                      cardSections: { ...sections, [option.key]: !active },
                    })
                  }
                  className={`rounded-md border px-2 py-1 text-[12px] transition ${
                    active
                      ? "border-[var(--hud-accent)] text-[var(--hud-accent)]"
                      : "border-white/12 text-[var(--hud-muted)] hover:border-white/30"
                  }`}
                >
                  {option.label}
                </button>
              );
            })}
          </div>
        </section>

        {/* ---- 配色 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionAccent")}
          </h2>
          <div className="flex gap-1.5">
            {[
              { value: "clay" as const, label: t("settingsAccentClay") },
              { value: "olive" as const, label: t("settingsAccentOlive") },
            ].map((option) => (
              <button
                key={option.value}
                type="button"
                onClick={() => {
                  setError(null);
                  apply({ accent: option.value });
                }}
                className={`flex-1 rounded-[var(--hud-r-ctl)] border px-3 py-2 text-left text-[12px] ${
                  settings?.accent === option.value
                    ? "border-[var(--hud-accent)] bg-[var(--hud-raised)]"
                    : "border-[var(--hud-border-soft)] hover:border-[var(--hud-border)]"
                }`}
              >
                <span className="flex items-center gap-2">
                  <span
                    className="h-3 w-3 shrink-0 rounded-full border"
                    style={{
                      backgroundColor:
                        option.value === "clay" ? "#bd7454" : "#98a67d",
                      borderColor: "var(--hud-border)",
                    }}
                  />
                  {option.label}
                </span>
              </button>
            ))}
          </div>
        </section>

        {/* ---- HUD 最大高度 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 flex items-center gap-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionSize")}
            <span className="ml-auto font-mono text-[12px] text-[var(--hud-accent)]">
              {settings?.hudMaxHeight ?? 480} px
            </span>
          </h2>
          <input
            type="range"
            min={160}
            max={720}
            step={20}
            value={settings?.hudMaxHeight ?? 480}
            onChange={(event) =>
              apply({ hudMaxHeight: Number(event.target.value) })
            }
            className="w-full accent-[var(--hud-accent)]"
          />
        </section>

        {error && (
          <p className="mt-2 rounded-md border border-red-400/30 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
            {t("settingsHotkeySaveFailed")}：{error}
          </p>
        )}

        {/* ---- 界面 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionLooks")}
          </h2>
          <button
            type="button"
            onClick={() => apply({ showHints: !(settings?.showHints ?? true) })}
            className={`flex w-full items-start gap-2 rounded-lg border px-3 py-2 text-left transition ${
              settings?.showHints ?? true
                ? "border-[var(--hud-accent)] bg-white/5"
                : "border-white/10 hover:border-white/25"
            }`}
          >
            <span
              className="mt-1 grid size-3 shrink-0 place-items-center rounded-[3px] border"
              style={{
                borderColor: (settings?.showHints ?? true)
                  ? "var(--hud-accent)"
                  : "oklch(1 0 0 / 25%)",
              }}
            >
              {(settings?.showHints ?? true) && (
                <span
                  className="block size-1.5 rounded-[1px]"
                  style={{ background: "var(--hud-accent)" }}
                />
              )}
            </span>
            <span className="min-w-0">
              <span className="block text-[13px]">{t("settingsShowHints")}</span>
              <span className="mt-0.5 block text-[11px] leading-snug text-[var(--hud-muted)]">
                {t("settingsShowHintsHint")}
              </span>
            </span>
          </button>
        </section>

        {/* ---- 性能 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionPerf")}
          </h2>
          <div className="grid grid-cols-3 gap-2">
            <Stat label={t("settingsStartup")} value={startup} />
            <Stat label={t("settingsPresents")} value={presents} />
            <Stat label={t("settingsLastPaint")} value={lastPaint} />
          </div>
        </section>

      </main>
    </div>
  );
}

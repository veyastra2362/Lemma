#!/usr/bin/env node
/**
 * 决策笔记校验入口。
 *
 * 为什么不直接在 package.json 里写脚本路径：`write-notes-like-deepseek` 通常装在
 * `%USERPROFILE%\.agents\skills\` 下，写死绝对路径会把本机用户名带进公开仓库，
 * 换机器也会失效。这里按顺序探测，找不到再报可操作的提示。
 *
 * 用法：
 *   node scripts/notes-verify.mjs            # tree + format + archived 三线串跑
 *   node scripts/notes-verify.mjs --archive <笔记路径> [--superseded-by <新笔记>]
 *
 * 优先级：环境变量 NOTES_SKILL_DIR → 用户级全局 → 项目级 .agents/skills → .claude/skills
 */
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SKILL_NAME = "write-notes-like-deepseek";
const VERIFY_STEPS = [
  "verify-agent-note-tree.ts",
  "verify-agent-note-format.ts",
  "verify-archived-agent-notes.ts",
];

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");

function resolveSkillDir() {
  const candidates = [
    process.env.NOTES_SKILL_DIR,
    join(homedir(), ".agents", "skills", SKILL_NAME),
    join(repoRoot, ".agents", "skills", SKILL_NAME),
    join(repoRoot, ".claude", "skills", SKILL_NAME),
  ].filter(Boolean);

  for (const dir of candidates) {
    if (existsSync(join(dir, "scripts", VERIFY_STEPS[0]))) {
      return dir;
    }
  }
  return null;
}

/** Node 23.6+ 能直接跑 .ts（类型剥离），否则退回 npx tsx。 */
function runScript(file, args) {
  const direct = spawnSync(process.execPath, [file, ...args], {
    stdio: "inherit",
    cwd: repoRoot,
  });
  if (direct.error?.code === "ERR_UNKNOWN_FILE_EXTENSION") {
    const npx = process.platform === "win32" ? "npx.cmd" : "npx";
    return spawnSync(npx, ["--yes", "tsx", file, ...args], {
      stdio: "inherit",
      cwd: repoRoot,
    });
  }
  return direct;
}

const skillDir = resolveSkillDir();
if (!skillDir) {
  console.error(
    [
      `找不到技能 ${SKILL_NAME}。`,
      "请任选一种方式后重试：",
      `  1. 安装到用户级：~/.agents/skills/${SKILL_NAME}/`,
      `  2. 安装到项目级：<repo>/.agents/skills/${SKILL_NAME}/`,
      "  3. 设置环境变量 NOTES_SKILL_DIR 指向技能目录",
    ].join("\n"),
  );
  process.exit(2);
}

const scriptsDir = join(skillDir, "scripts");

// --archive：转发给归档脚本，其余参数原样传下去。
if (process.argv[2] === "--archive") {
  const result = runScript(join(scriptsDir, "archive-agent-note.ts"), process.argv.slice(3));
  process.exit(result.status ?? 1);
}

for (const step of VERIFY_STEPS) {
  const result = runScript(join(scriptsDir, step), []);
  if (result.status !== 0) {
    console.error(`\n笔记校验失败：${step}`);
    process.exit(result.status ?? 1);
  }
}

console.log("\n笔记校验通过（tree / format / archived）。");

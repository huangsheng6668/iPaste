#!/usr/bin/env bash
#
# iPaste macOS 一键安装脚本
#
# 背景：iPaste 的 macOS 安装包未经 Apple 签名/公证，从浏览器下载后
# 会被 Gatekeeper 拦截，提示「已损坏，无法打开」。本脚本负责挂载 DMG、
# 拷贝应用到 /Applications、清除隔离标记（quarantine），让应用可以正常打开。
#
# 用法：
#   bash install-macos.sh                        # 自动从 ~/Downloads 找最新的 dmg
#   bash install-macos.sh /path/to/iPaste.dmg    # 指定 dmg 路径
#
# 通过 GitHub Releases 直接执行：
#   bash <(curl -fsSL https://github.com/huangsheng6668/iPaste/releases/latest/download/install-macos.sh)

set -euo pipefail

APP_NAME="iPaste"
DEST="/Applications/${APP_NAME}.app"
VOLUME_PREFIX="/Volumes/${APP_NAME}"

log()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m==>\033[0m %s\n' "$*" >&2; exit 1; }

# --- 1. 确定 DMG 路径 -------------------------------------------------------
DMG="${1:-}"
if [[ -z "${DMG}" ]]; then
  case "$(uname -m)" in
    arm64)   pattern="iPaste_*_aarch64.dmg" ;;
    x86_64)  pattern="iPaste_*_x64.dmg" ;;
    *)       pattern="iPaste_*.dmg" ;;
  esac
  DMG="$(ls -t "${HOME}/Downloads"/${pattern} 2>/dev/null | head -1 || true)"
  [[ -z "${DMG}" ]] && die "未在 ~/Downloads 找到 ${pattern}，请通过参数指定 dmg 路径"
fi
[[ -f "${DMG}" ]] || die "找不到 DMG 文件：${DMG}"

log "使用安装包：${DMG}"

# --- 2. 挂载 DMG ------------------------------------------------------------
if hdiutil info | grep -q "${VOLUME_PREFIX}"; then
  warn "检测到已挂载的 iPaste 卷，先卸载旧卷"
  hdiutil detach "${VOLUME_PREFIX}" >/dev/null || true
fi

MOUNT_DIR="$(hdiutil attach "${DMG}" -nobrowse -readonly | awk -F'\t' '/\/Volumes\// {print $NF; exit}')"
[[ -n "${MOUNT_DIR}" ]] || die "挂载 DMG 失败"

APP_SRC="${MOUNT_DIR}/${APP_NAME}.app"
if [[ ! -d "${APP_SRC}" ]]; then
  hdiutil detach "${MOUNT_DIR}" >/dev/null 2>&1 || true
  die "DMG 中未找到 ${APP_NAME}.app"
fi

# --- 3. 替换旧版本 ----------------------------------------------------------
if [[ -d "${DEST}" ]]; then
  log "检测到已安装的旧版本，先退出并替换"
  osascript -e "tell application \"${APP_NAME}\" to quit" >/dev/null 2>&1 || true
  sleep 1
  rm -rf "${DEST}"
fi

# --- 4. 拷贝并清除隔离标记 --------------------------------------------------
log "拷贝应用到 /Applications ..."
cp -R "${APP_SRC}" /Applications/

log "清除隔离标记（com.apple.quarantine）..."
xattr -dr com.apple.quarantine "${DEST}" 2>/dev/null || true

hdiutil detach "${MOUNT_DIR}" >/dev/null 2>&1 || true

# --- 5. 启动 ----------------------------------------------------------------
log "安装完成：${DEST}"
open "${DEST}"
log "iPaste 已启动。首次使用时请在「系统设置 → 隐私与安全性 → 辅助功能」中授予权限（自动粘贴需要）。"

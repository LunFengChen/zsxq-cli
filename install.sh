#!/usr/bin/env bash
# zsxq-cli 一键安装：二进制 + shell 补全
#   curl -fsSL https://raw.githubusercontent.com/LunFengChen/zsxq-cli/master/install.sh | bash
set -euo pipefail

REPO="LunFengChen/zsxq-cli"
BIN_DIR="${ZSXQ_BIN_DIR:-$HOME/.local/bin}"

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$os-$arch" in
    linux-x86_64)  asset="zsxq-cli-linux-x86_64" ;;
    linux-amd64)   asset="zsxq-cli-linux-x86_64" ;;
    darwin-arm64)  asset="zsxq-cli-macos-arm64" ;;
    *) echo "不支持的平台: $os-$arch（可 cargo build --release 源码构建）" >&2; exit 1 ;;
esac

url="https://github.com/$REPO/releases/latest/download/$asset"
mkdir -p "$BIN_DIR"
tmp=$(mktemp)
echo "下载 $url"
curl -fsSL "$url" -o "$tmp"
chmod +x "$tmp"
mv -f "$tmp" "$BIN_DIR/zsxq"
ZSXQ="$BIN_DIR/zsxq"
echo "已安装 $ZSXQ ($("$ZSXQ" --version))"

# ---- 补全：探测存在的 shell，装对应的注册脚本 ----
installed=()

# fish：文件式，丢进 completions 目录即自动加载
if command -v fish >/dev/null 2>&1 || [ -d "$HOME/.config/fish" ]; then
    mkdir -p "$HOME/.config/fish/completions"
    COMPLETE=fish "$ZSXQ" > "$HOME/.config/fish/completions/zsxq.fish"
    installed+=("fish(新开终端生效)")
fi

# zsh / bash：往 rc 文件追加一行 source <(...)（幂等）
add_rc_line() {
    local rc="$1" line="$2"
    [ -f "$rc" ] || touch "$rc"
    grep -qF "$line" "$rc" 2>/dev/null || echo "$line" >> "$rc"
}
if [ "$(basename "${SHELL:-}")" = "zsh" ] || [ -f "$HOME/.zshrc" ]; then
    add_rc_line "$HOME/.zshrc" 'source <(COMPLETE=zsh zsxq)'
    installed+=("zsh(重开终端或 source ~/.zshrc)")
fi
if [ "$(basename "${SHELL:-}")" = "bash" ] || [ -f "$HOME/.bashrc" ]; then
    add_rc_line "$HOME/.bashrc" 'source <(COMPLETE=bash zsxq)'
    installed+=("bash(重开终端或 source ~/.bashrc)")
fi
[ ${#installed[@]} -gt 0 ] && echo "补全已装: ${installed[*]}"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "提示: $BIN_DIR 不在 PATH，加一行:  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.bashrc" ;;
esac

echo "完成。运行 zsxq auth login 登录。"

# zsxq-cli — 知识星球命令行客户端

自研 zsxq web API 客户端，权限等同网页浏览：**不需要星球开 Skill**，
长文章可拉全文转 markdown。覆盖星球/主题/评论/精华/置顶/标签/专栏/文件/
搜索/收藏/成员信息，支持点赞、评论、发帖、收藏等写操作。

## 安装

Linux / macOS 一键安装（二进制到 `~/.local/bin`，并自动配置 fish/bash/zsh 补全）：

```bash
curl -fsSL https://raw.githubusercontent.com/LunFengChen/zsxq-cli/master/install.sh | bash
```

Windows：到 [Releases](https://github.com/LunFengChen/zsxq-cli/releases) 下
`zsxq-cli-windows-x86_64.exe` 手动放入 PATH（install.sh 不适用）。

或源码编译（rustup 环境）：`cargo build --release`，产物在 `target/release/zsxq`。

## 登录

```bash
zsxq auth login                 # 交互式：粘贴 cookie 整串或裸 token
zsxq auth login --name work     # 登录为名为 work 的账号（支持多账号）
zsxq auth list                  # 列出所有账号（* 为激活）
zsxq auth use work              # 切换激活账号
zsxq auth status                # 查看登录态
zsxq auth logout [名字]          # 删除账号（默认删激活）
zsxq -a work groups             # 临时用 work 账号跑一条命令
```

账号存于 `~/.config/zsxq/accounts.json`（0600），每个账号有独立 token + 设备指纹。

cookie 获取：浏览器登录 `wx.zsxq.com` → F12 → Application → Cookies →
复制 `zsxq_access_token` 的值，或整串 cookie 直接粘进来（自动提取）。

## 用法

```bash
zsxq me                            # 我的资料
zsxq groups                        # 列星球
zsxq group-info <gid>              # 星球详情（简介/星主/统计）
zsxq members <gid> -n 100          # 成员名单
zsxq topics <gid> -n 5             # 枚举主题（5页，--scope all|digests|latest|questions|tasks）
zsxq topic <tid>                   # 帖子全文（markdown，长文章自动展开）
zsxq comments <tid> -n 50          # 评论（含点赞数、二级回复）
zsxq likes <tid>                   # 点赞名单
zsxq rewards <tid>                 # 赞赏名单
zsxq search "关键词" [--gid <gid>]  # 全局/星球内搜索
zsxq digests <gid> [--kw 词]       # 精华
zsxq columns <gid>                 # 专栏（星主分类）
zsxq column-topics <gid> <cid>     # 专栏下全部主题
zsxq hashtags <gid>                # 标签
zsxq tag-topics <hid> -n 3         # 标签下主题
zsxq sticky <gid>                  # 置顶
zsxq files <gid>                   # 文件列表
zsxq favorites -n 30               # 我的收藏
zsxq download <fid> [out.zip]      # 下载附件
```

写操作（作用在自己账号上）：

```bash
zsxq like <tid> / unlike <tid>           # 赞 / 取消赞主题
zsxq like-comment <cid> / unlike-comment # 赞 / 取消赞评论
zsxq comment <tid> "内容" [--reply <cid>] # 评论 / 楼中楼回复
zsxq uncomment <cid>                     # 删自己的评论
zsxq favorite <tid> / unfavorite <tid>   # 收藏 / 取消收藏
zsxq post <gid> "内容" [--title 标题]     # 发帖
zsxq unpost <tid>                        # 删自己的帖
```

加 `--json` 输出原始 JSON 数组，方便接 jq：

```bash
zsxq --json groups | jq '.[].name'           # 只取星球名
zsxq --json topic <tid> | jq -r '.talk.text' # 拿未转换的原始字段
```

手动安装（非 install.sh）想开补全：`COMPLETE=<fish|zsh|bash> zsxq`
生成对应注册脚本，写入各自 shell 的加载位置即可。

## 开发

```bash
cargo test                         # 单元测试（cookie 解析、富文本/HTML 转换）
cargo clippy --all-targets         # lint
cargo fmt                          # 格式化
```

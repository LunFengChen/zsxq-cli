mod auth;
mod client;
mod fmt;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

use client::ZsxqClient;
use fmt::{rich_to_md, rich_to_plain};

#[derive(Parser)]
#[command(
    name = "zsxq",
    version,
    about = "知识星球 web 端逆向 CLI",
    long_about = "权限等同网页浏览：不需要星球开 Skill，长文章可拉全文。\n\
                  认证：zsxq auth login（粘贴 cookie 或 zsxq_access_token）"
)]
struct Cli {
    /// 原始 JSON 输出
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 登录态管理
    Auth {
        #[command(subcommand)]
        sub: AuthCmd,
    },
    /// 列出加入的星球
    Groups,
    /// 当前用户资料
    Me,
    /// 我的收藏
    Favorites {
        #[arg(short = 'n', long, default_value_t = 20)]
        count: u32,
    },
    /// 星球详情（简介/星主/编号/统计）
    GroupInfo { gid: String },
    /// 星球成员名单
    Members {
        gid: String,
        #[arg(short = 'n', long, default_value_t = 50)]
        count: u32,
    },
    /// 枚举星球主题
    Topics {
        gid: String,
        /// all|digests|latest|by_owner|questions|tasks
        #[arg(short, long, default_value = "all")]
        scope: String,
        /// 页数（每页20条）
        #[arg(short = 'n', long, default_value_t = 2)]
        pages: u32,
    },
    /// 主题详情（长文章自动拉全文 markdown）
    Topic { tid: String },
    /// 主题评论（含点赞数与回复）
    Comments {
        tid: String,
        #[arg(short = 'n', long, default_value_t = 20)]
        count: u32,
    },
    /// 主题点赞名单
    Likes {
        tid: String,
        #[arg(short = 'n', long, default_value_t = 50)]
        count: u32,
    },
    /// 主题赞赏名单
    Rewards {
        tid: String,
        #[arg(short = 'n', long, default_value_t = 50)]
        count: u32,
    },
    /// 搜索主题（默认全局，--gid 星球内）
    Search {
        keyword: String,
        #[arg(long, default_value = "")]
        gid: String,
    },
    /// 星球精华（/topics/digests 端点已被风控，走 topics?scope=digests）
    Digests {
        gid: String,
        /// 关键词客户端过滤
        #[arg(long, default_value = "")]
        kw: String,
        /// 页数（每页20条）
        #[arg(short = 'n', long, default_value_t = 2)]
        pages: u32,
    },
    /// 星球专栏（星主分类）
    Columns { gid: String },
    /// 专栏下主题
    ColumnTopics { gid: String, cid: String },
    /// 星球标签
    Hashtags { gid: String },
    /// 标签下主题
    TagTopics {
        hid: String,
        #[arg(short = 'n', long, default_value_t = 1)]
        pages: u32,
    },
    /// 星球置顶
    Sticky { gid: String },
    /// 星球文件列表
    Files { gid: String },
    /// 下载附件
    Download { fid: String, dest: Option<String> },
    /// 赞主题
    Like { tid: String },
    /// 取消赞主题
    Unlike { tid: String },
    /// 赞评论
    LikeComment { cid: String },
    /// 取消赞评论
    UnlikeComment { cid: String },
    /// 发评论（--reply 回复某条评论）
    Comment {
        tid: String,
        text: String,
        /// 被回复的评论 id（楼中楼）
        #[arg(long)]
        reply: Option<String>,
    },
    /// 删除自己的评论
    Uncomment { cid: String },
    /// 收藏主题
    Favorite { tid: String },
    /// 取消收藏
    Unfavorite { tid: String },
    /// 发帖（talk 纯文本，可选 --title）
    Post {
        gid: String,
        text: String,
        #[arg(long)]
        title: Option<String>,
    },
    /// 删除自己的主题
    Unpost { tid: String },
}

#[derive(Subcommand)]
enum AuthCmd {
    /// 登录：粘贴整串 cookie 或裸 token，验证后存 ~/.config/zsxq/token
    Login {
        /// 可直接给 token/cookie；不给则提示粘贴
        token: Option<String>,
    },
    /// 当前登录状态
    Status,
    /// 退出登录（删除本地 token）
    Logout,
}

fn api() -> Result<ZsxqClient> {
    let token = auth::load_or_err()?;
    ZsxqClient::new(&token, &auth::load_or_gen_aduid())
}

fn jv(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn body_of(t: &Value) -> Value {
    // 专栏/标签等接口的 topic 把 text/owner 直接放顶层，此时主体即 topic 自身
    ["talk", "question", "answer", "task"]
        .iter()
        .find_map(|k| t.get(*k).filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or_else(|| t.clone())
}

fn brief(t: &Value) -> String {
    let body = body_of(t);
    let title = {
        let x = rich_to_plain(t.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        if x.is_empty() {
            rich_to_plain(body.get("text").and_then(|v| v.as_str()).unwrap_or(""))
        } else {
            x
        }
    };
    let stats = |k: &str| t.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    format!(
        "{}\t{}\t{}\t赞{} 评{}",
        t.get("topic_id").and_then(|v| v.as_i64()).unwrap_or(0),
        t.get("create_time")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .get(..16)
            .unwrap_or(""),
        title
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>(),
        stats("likes_count"),
        stats("comments_count"),
    )
}

/// 单条评论输出：[置顶][作者] 正文 (赞N)
fn print_comment(cm: &Value, indent: &str) {
    let sticky = cm.get("sticky").and_then(|v| v.as_bool()).unwrap_or(false);
    let likes = cm.get("likes_count").and_then(|v| v.as_i64()).unwrap_or(0);
    println!(
        "{}{}[{}] {}{}",
        indent,
        if sticky { "[置顶]" } else { "" },
        cm.get("owner")
            .and_then(|o| o.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        rich_to_md(cm.get("text").and_then(|v| v.as_str()).unwrap_or("")),
        if likes > 0 {
            format!("  (赞{})", likes)
        } else {
            String::new()
        },
    );
}

/// 列表输出：--json 时输出整个 JSON 数组（可被下游 parse），否则逐行格式化。
fn out_list(items: &[Value], json: bool, f: impl Fn(&Value) -> String) {
    if json {
        println!("{}", jv(&json!(items)));
    } else {
        for i in items {
            println!("{}", f(i));
        }
    }
}

fn main() -> Result<()> {
    // Unix: 下游管道关闭时（`| head`）走默认 SIGPIPE 退出，而不是 println! panic
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();

    if let Cmd::Auth { sub } = &cli.cmd {
        return auth_cmd(sub);
    }

    let c = api()?;
    match cli.cmd {
        Cmd::Auth { .. } => unreachable!(),
        Cmd::Me => {
            let u = c.me()?;
            if cli.json {
                println!("{}", jv(&u));
            } else {
                println!(
                    "{}\t{}\t{}",
                    u.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0),
                    u.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    u.get("unique_id").and_then(|v| v.as_str()).unwrap_or("")
                );
            }
        }
        Cmd::Favorites { count } => {
            let fs = c.favorites(count)?;
            out_list(&fs, cli.json, |f| {
                let t = f.get("topic").cloned().unwrap_or(json!({}));
                let gname = t
                    .get("group")
                    .and_then(|g| g.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                format!(
                    "{}\t{}\t{}",
                    f.get("favor_time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .get(..16)
                        .unwrap_or(""),
                    gname,
                    brief(&t)
                )
            });
        }
        Cmd::GroupInfo { gid } => {
            let g = c.group_info(&gid)?;
            let st = c.group_stats(&gid).unwrap_or(json!({}));
            if cli.json {
                let mut g = g;
                g["statistics"] = st;
                println!("{}", jv(&g));
            } else {
                let sc = |k: &str| st.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
                let members = g
                    .get("statistics")
                    .and_then(|s| s.get("members"))
                    .and_then(|m| m.get("count"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                println!(
                    "{}  {}  (编号{})\n星主: {}\n创建: {}\n成员: {}  主题: {}  精华: {}  问答: {}  作业: {}\n\n{}",
                    g.get("group_id").and_then(|v| v.as_i64()).unwrap_or(0),
                    g.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    g.get("number").and_then(|v| v.as_i64()).unwrap_or(0),
                    g.get("owner")
                        .and_then(|o| o.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    g.get("create_time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .get(..10)
                        .unwrap_or(""),
                    members,
                    sc("topics_count"),
                    sc("digests_count"),
                    sc("answers_count"),
                    sc("tasks_count"),
                    g.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                );
            }
        }
        Cmd::Members { gid, count } => {
            let ms = c.members(&gid, count)?;
            out_list(&ms, cli.json, |m| {
                format!(
                    "{}\t{}\t{}",
                    m.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0),
                    m.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    m.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                )
            });
        }
        Cmd::Groups => {
            let gs = c.groups()?;
            out_list(&gs, cli.json, |g| {
                let members = g
                    .get("statistics")
                    .and_then(|s| s.get("members"))
                    .and_then(|m| m.get("count"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                format!(
                    "{}\t{}\t{}人",
                    g.get("group_id").and_then(|v| v.as_i64()).unwrap_or(0),
                    g.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    members
                )
            });
        }
        Cmd::Topics { gid, scope, pages } => {
            let mut end: Option<String> = None;
            let mut all: Vec<Value> = Vec::new();
            for _ in 0..pages {
                let items = c.topics_page(&gid, &scope, 20, end.as_deref())?;
                if items.is_empty() {
                    break;
                }
                end = items
                    .last()
                    .and_then(|t| t.get("create_time"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                all.extend(items);
            }
            out_list(&all, cli.json, brief);
        }
        Cmd::Topic { tid } => {
            let t = c.topic(&tid)?;
            if t.is_null() {
                anyhow::bail!("主题不存在或无权限");
            }
            if cli.json {
                println!("{}", jv(&t));
            } else {
                let body = body_of(&t);
                let title = rich_to_plain(t.get("title").and_then(|v| v.as_str()).unwrap_or(""));
                println!("# {}\n", title);
                let st = |k: &str| t.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
                println!(
                    "- tid: `{}`\n- 作者: {}\n- 时间: {}\n- 类型: {}\n- 赞: {}  评论: {}  阅读: {}  读者: {}  赞赏: {}\n\n---\n",
                    tid,
                    body.get("owner")
                        .and_then(|o| o.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    t.get("create_time").and_then(|v| v.as_str()).unwrap_or(""),
                    t.get("type").and_then(|v| v.as_str()).unwrap_or(""),
                    st("likes_count"),
                    st("comments_count"),
                    st("reading_count"),
                    st("readers_count"),
                    st("rewards_count"),
                );
                println!("{}", c.topic_full_text(&t));
                let files = body
                    .get("files")
                    .or_else(|| t.get("files"))
                    .and_then(|f| f.as_array())
                    .cloned()
                    .unwrap_or_default();
                if !files.is_empty() {
                    println!("\n## 附件\n");
                    for f in files {
                        println!(
                            "- {} ({}B) file_id={}",
                            f.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            f.get("size").and_then(|v| v.as_i64()).unwrap_or(0),
                            f.get("file_id").and_then(|v| v.as_i64()).unwrap_or(0)
                        );
                    }
                }
            }
        }
        Cmd::Comments { tid, count } => {
            let cms = c.comments(&tid, count)?;
            if cli.json {
                println!("{}", jv(&json!(cms)));
            } else {
                for cm in &cms {
                    print_comment(cm, "");
                    // 二级评论内嵌在 replied_comments
                    if let Some(rs) = cm.get("replied_comments").and_then(|v| v.as_array()) {
                        for r in rs {
                            print_comment(r, "  ↳ ");
                        }
                    }
                }
            }
        }
        Cmd::Likes { tid, count } => {
            let ls = c.likes(&tid, count)?;
            out_list(&ls, cli.json, |l| {
                format!(
                    "{}\t{}",
                    l.get("create_time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .get(..16)
                        .unwrap_or(""),
                    l.get("owner")
                        .and_then(|o| o.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
            });
        }
        Cmd::Rewards { tid, count } => {
            let rs = c.rewards(&tid, count)?;
            out_list(&rs, cli.json, |r| {
                format!(
                    "{}\t{}\t{}元",
                    r.get("create_time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .get(..16)
                        .unwrap_or(""),
                    r.get("owner")
                        .and_then(|o| o.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    r.get("number").and_then(|v| v.as_f64()).unwrap_or(0.0),
                )
            });
        }
        Cmd::Search { keyword, gid } => {
            let items = c.search(&keyword, if gid.is_empty() { None } else { Some(&gid) })?;
            let topics: Vec<Value> = items
                .iter()
                .map(|it| it.get("topic").cloned().unwrap_or(it.clone()))
                .collect();
            out_list(&topics, cli.json, brief);
        }
        Cmd::Digests { gid, kw, pages } => {
            let mut end: Option<String> = None;
            let mut all: Vec<Value> = Vec::new();
            for _ in 0..pages {
                let items = c.topics_page(&gid, "digests", 20, end.as_deref())?;
                if items.is_empty() {
                    break;
                }
                end = items
                    .last()
                    .and_then(|t| t.get("create_time"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                all.extend(items);
            }
            if !kw.is_empty() {
                all.retain(|t| brief(t).contains(kw.as_str()));
            }
            out_list(&all, cli.json, brief);
        }
        Cmd::Columns { gid } => {
            let cols = c.columns(&gid)?;
            out_list(&cols, cli.json, |c| {
                format!(
                    "{}\t{}\t{}篇",
                    c.get("column_id").and_then(|v| v.as_i64()).unwrap_or(0),
                    c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    c.get("statistics")
                        .and_then(|s| s.get("topics_count"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                )
            });
        }
        Cmd::ColumnTopics { gid, cid } => {
            out_list(&c.column_topics(&gid, &cid)?, cli.json, brief);
        }
        Cmd::Hashtags { gid } => {
            let hs = c.hashtags(&gid)?;
            out_list(&hs, cli.json, |h| {
                format!(
                    "{}\t{}\t{}",
                    h.get("hashtag_id").and_then(|v| v.as_i64()).unwrap_or(0),
                    h.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    h.get("topics_count").and_then(|v| v.as_i64()).unwrap_or(0)
                )
            });
        }
        Cmd::TagTopics { hid, pages } => {
            let mut end: Option<String> = None;
            let mut all: Vec<Value> = Vec::new();
            for _ in 0..pages {
                let items = c.tag_topics(&hid, end.as_deref())?;
                if items.is_empty() {
                    break;
                }
                end = items
                    .last()
                    .and_then(|t| t.get("create_time"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                all.extend(items);
            }
            out_list(&all, cli.json, brief);
        }
        Cmd::Sticky { gid } => out_list(&c.sticky(&gid)?, cli.json, brief),
        Cmd::Files { gid } => {
            let fs: Vec<Value> = c
                .group_files(&gid, 20)?
                .iter()
                .map(|f| f.get("file").cloned().unwrap_or(f.clone()))
                .collect();
            out_list(&fs, cli.json, |f| {
                format!(
                    "{}\t{}",
                    f.get("file_id").and_then(|v| v.as_i64()).unwrap_or(0),
                    f.get("name").and_then(|v| v.as_str()).unwrap_or("")
                )
            });
        }
        Cmd::Download { fid, dest } => {
            let dest = PathBuf::from(dest.unwrap_or_else(|| fid.clone()));
            let n = c.download(&fid, &dest)?;
            println!("{} ({}B)", dest.display(), n);
        }
        Cmd::Like { tid } => {
            c.like_topic(&tid)?;
            println!("已赞 {}", tid);
        }
        Cmd::Unlike { tid } => {
            c.unlike_topic(&tid)?;
            println!("已取消赞 {}", tid);
        }
        Cmd::LikeComment { cid } => {
            c.like_comment(&cid)?;
            println!("已赞评论 {}", cid);
        }
        Cmd::UnlikeComment { cid } => {
            c.unlike_comment(&cid)?;
            println!("已取消赞评论 {}", cid);
        }
        Cmd::Comment { tid, text, reply } => {
            let cm = c.add_comment(&tid, &text, reply.as_deref())?;
            let cid = cm.get("comment_id").and_then(|v| v.as_i64()).unwrap_or(0);
            println!("已评论 comment_id={}", cid);
        }
        Cmd::Uncomment { cid } => {
            c.del_comment(&cid)?;
            println!("已删除评论 {}", cid);
        }
        Cmd::Favorite { tid } => {
            c.favorite(&tid)?;
            println!("已收藏 {}", tid);
        }
        Cmd::Unfavorite { tid } => {
            c.unfavorite(&tid)?;
            println!("已取消收藏 {}", tid);
        }
        Cmd::Post { gid, text, title } => {
            let t = c.post_topic(&gid, &text, title.as_deref())?;
            let tid = t.get("topic_id").and_then(|v| v.as_i64()).unwrap_or(0);
            println!("已发布 topic_id={}", tid);
        }
        Cmd::Unpost { tid } => {
            c.del_topic(&tid)?;
            println!("已删除主题 {}", tid);
        }
    }
    Ok(())
}

fn auth_cmd(sub: &AuthCmd) -> Result<()> {
    match sub {
        AuthCmd::Login { token } => {
            let input = match token {
                Some(t) => t.clone(),
                None => {
                    eprint!("粘贴 cookie 整串或 zsxq_access_token，回车确认: ");
                    std::io::stderr().flush()?;
                    let mut s = String::new();
                    std::io::stdin().read_line(&mut s)?;
                    s
                }
            };
            let tok = auth::extract_token(&input).ok_or_else(|| {
                anyhow::anyhow!("解析失败：没在里面找到 zsxq_access_token，也没看到像 token 的值")
            })?;
            // 验证
            let c = ZsxqClient::new(&tok, &auth::load_or_gen_aduid())?;
            let gs = c.groups().map_err(|e| {
                anyhow::anyhow!(
                    "token 验证失败（{}）：cookie 过期或不是 zsxq_access_token",
                    e
                )
            })?;
            auth::save(&tok)?;
            println!(
                "已登录并保存到 {}（{} 个星球）",
                auth::token_path().display(),
                gs.len()
            );
        }
        AuthCmd::Status => match auth::load() {
            Some(tok) => {
                println!("token 来源: {}", auth::token_path().display());
                let c = ZsxqClient::new(&tok, &auth::load_or_gen_aduid())?;
                match c.groups() {
                    Ok(gs) => println!("有效 ✓  可访问 {} 个星球", gs.len()),
                    Err(e) => println!("已失效: {}", e),
                }
            }
            None => println!("未登录：`zsxq auth login`"),
        },
        AuthCmd::Logout => {
            auth::remove()?;
            println!("已删除 {}", auth::token_path().display());
        }
    }
    Ok(())
}

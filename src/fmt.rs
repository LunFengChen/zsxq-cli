//! zsxq 富文本 → markdown：
//! 1) 正文 `<e type="text_bold|web|hashtag" .../>` 标签还原
//! 2) articles.zsxq.com quill HTML → markdown

use percent_encoding::percent_decode_str;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

// 闭合的 <e ... > 或 <e ... />
static E_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<e\s+([^>\n]*?)\s*>").unwrap());
// 预览截断残留的未闭合 <e ...（到行尾）；尾部 > 可选
static E_TAG_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<e\s+([^>\n]*)>?").unwrap());
static E_ATTR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(\w+)="([^"]*)""#).unwrap());
// 截断文本里引号未闭合的尾部属性，如 ` title="%E4%BD`
static E_ATTR_TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(\w+)="([^"]*)$"#).unwrap());
static TAIL_ESC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"%[0-9A-Fa-f]?$").unwrap());

fn attrs(s: &str) -> HashMap<String, String> {
    // 闭合引号的完整属性优先；截断文本里未闭合的尾部属性兜底
    let mut m: HashMap<String, String> = E_ATTR
        .captures_iter(s)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();
    if let Some(c) = E_ATTR_TAIL.captures(s) {
        m.entry(c[1].to_string())
            .or_insert_with(|| c[2].to_string());
    }
    m
}

fn unq(s: &str) -> String {
    let d = percent_decode_str(s)
        .decode_utf8()
        .unwrap_or_else(|_| Cow::from(s));
    TAIL_ESC.replace(&d, "").to_string()
}

fn get(a: &HashMap<String, String>, k: &str) -> String {
    a.get(k).map(|v| unq(v)).unwrap_or_default()
}

fn rep(c: &regex::Captures) -> String {
    let a = attrs(&c[1]);
    let title = get(&a, "title");
    let href = get(&a, "href");
    match a.get("type").map(|s| s.as_str()).unwrap_or("") {
        "text_bold" => {
            if title.is_empty() {
                String::new()
            } else {
                format!("**{}**", title)
            }
        }
        "web" => {
            if href.is_empty() {
                title
            } else if title.is_empty() || title.ends_with("...") {
                href
            } else {
                format!("[{}]({})", title, href)
            }
        }
        _ => {
            if !title.is_empty() {
                title
            } else {
                href
            }
        }
    }
}

/// 正文里的 <e .../> 富文本标签 → markdown。
pub fn rich_to_md(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let t = E_TAG.replace_all(text, rep as fn(&regex::Captures) -> String);
    E_TAG_OPEN
        .replace_all(&t, rep as fn(&regex::Captures) -> String)
        .to_string()
}

/// <e> 标签还原纯文本（标题用）。
pub fn rich_to_plain(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let plain = |c: &regex::Captures| -> String {
        let a = attrs(&c[1]);
        let t = get(&a, "title");
        if !t.is_empty() {
            t
        } else {
            get(&a, "href")
        }
    };
    E_TAG_OPEN
        .replace_all(text, plain as fn(&regex::Captures) -> String)
        .to_string()
}

/// inline_form 文章页 HTML → markdown。
/// 正文在 .content.ql-editor（quill 渲染），退 .milkdown-preview。
pub fn article_html_to_md(html: &str) -> String {
    let doc = Html::parse_document(html);
    let sel = Selector::parse(".content.ql-editor").unwrap();
    let root = doc.select(&sel).next().or_else(|| {
        doc.select(&Selector::parse(".milkdown-preview").unwrap())
            .next()
    });
    let Some(root) = root else {
        return String::new();
    };
    let mut out = String::new();
    for c in root.children() {
        conv(c, &mut out);
    }
    squeeze_blank(&out)
}

fn conv(n: ego_tree::NodeRef<'_, scraper::Node>, out: &mut String) {
    use scraper::Node;
    match n.value() {
        Node::Text(t) => out.push_str(t.text.as_ref()),
        Node::Element(_) => {
            let el = ElementRef::wrap(n).unwrap();
            let name = el.value().name().to_string();
            match name.as_str() {
                "p" | "div" => {
                    children(el, out);
                    out.push_str("\n\n");
                }
                "h1" | "h2" | "h3" | "h4" => {
                    let mut s = String::new();
                    children(el, &mut s);
                    if !s.trim().is_empty() {
                        let lv: usize = name[1..].parse().unwrap_or(1);
                        out.push_str(&"#".repeat(lv));
                        out.push(' ');
                        out.push_str(s.trim());
                        out.push_str("\n\n");
                    }
                }
                "a" => {
                    let href = el.value().attr("href").unwrap_or("");
                    let mut s = String::new();
                    children(el, &mut s);
                    let t = if s.trim().is_empty() { href } else { s.trim() };
                    out.push_str(&format!("[{}]({})", t, href));
                }
                "img" => {
                    let src = el.value().attr("src").unwrap_or("");
                    out.push_str(&format!("![]({})", src));
                }
                "strong" | "b" => {
                    let mut s = String::new();
                    children(el, &mut s);
                    out.push_str(&format!("**{}**", s));
                }
                "em" | "i" => {
                    let mut s = String::new();
                    children(el, &mut s);
                    out.push_str(&format!("*{}*", s));
                }
                "br" => out.push('\n'),
                "ul" | "ol" => {
                    children(el, out);
                    out.push('\n');
                }
                "li" => {
                    let mut s = String::new();
                    children(el, &mut s);
                    out.push_str("- ");
                    out.push_str(s.trim());
                    out.push('\n');
                }
                "pre" => {
                    let code: String = el.text().collect();
                    let lang = el
                        .select(&Selector::parse("code").unwrap())
                        .next()
                        .and_then(|c| c.value().attr("class"))
                        .and_then(|cl| {
                            cl.split_whitespace()
                                .find(|c| c.starts_with("language-"))
                                .map(|c| c[9..].to_string())
                        })
                        .unwrap_or_default();
                    out.push_str(&format!("```{}\n{}\n```\n\n", lang, code.trim_end()));
                }
                "code" => {
                    let mut s = String::new();
                    children(el, &mut s);
                    out.push('`');
                    out.push_str(&s);
                    out.push('`');
                }
                "blockquote" => {
                    let mut s = String::new();
                    children(el, &mut s);
                    for l in s.trim().lines() {
                        out.push_str("> ");
                        out.push_str(l);
                        out.push('\n');
                    }
                    out.push('\n');
                }
                "hr" => out.push_str("\n---\n\n"),
                "script" | "style" => {}
                _ => children(el, out),
            }
        }
        _ => {}
    }
}

fn children(el: ElementRef, out: &mut String) {
    for c in el.children() {
        conv(c, out);
    }
}

fn squeeze_blank(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut nl = 0;
    for c in s.chars() {
        if c == '\n' {
            nl += 1;
            if nl <= 2 {
                out.push(c);
            }
        } else {
            nl = 0;
            out.push(c);
        }
    }
    out.trim_matches('\n').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_tag() {
        let s = "前<e type=\"text_bold\" title=\"%E9%87%8D%E7%82%B9\"/>后";
        assert_eq!(rich_to_md(s), "前**重点**后");
    }

    #[test]
    fn web_tag() {
        let s = "<e type=\"web\" href=\"https%3A%2F%2Fx.com%2Fa\" title=\"%E9%93%BE%E6%8E%A5\"/>";
        assert_eq!(rich_to_md(s), "[链接](https://x.com/a)");
    }

    #[test]
    fn web_tag_no_title_gives_url() {
        let s = "<e type=\"web\" href=\"https%3A%2F%2Fx.com\"/>";
        assert_eq!(rich_to_md(s), "https://x.com");
    }

    #[test]
    fn hashtag_tag() {
        let s = "<e type=\"hashtag\" hid=\"123\" title=\"%23%E8%80%81%E5%8F%B8%E6%9C%BA%23\"/>";
        assert_eq!(rich_to_md(s), "#老司机#");
    }

    #[test]
    fn truncated_unclosed_tag() {
        // 预览截断：标签没闭合、属性可能截在 % 编码中间
        let s = "正文<e type=\"text_bold\" title=\"%E4%BD%A0%E5%A5%BD%E";
        assert_eq!(rich_to_md(s), "正文**你好**");
    }

    #[test]
    fn plain_strips_to_title() {
        let s = "<e type=\"text_bold\" title=\"%E6%A0%87%E9%A2%98\"/> 正文";
        assert_eq!(rich_to_plain(s), "标题 正文");
    }

    #[test]
    fn ql_editor_article() {
        let html = r##"<html><body><div class="content ql-editor">
            <h1>大标题</h1>
            <p>第一段<strong>加粗</strong>字</p>
            <p><a href="https://a.com">链接名</a></p>
            <ul><li>条目一</li><li>条目二</li></ul>
            <pre><code class="language-rust">fn main() {}</code></pre>
            </div></body></html>"##;
        let md = article_html_to_md(html);
        assert!(md.contains("# 大标题"));
        assert!(md.contains("第一段**加粗**字"));
        assert!(md.contains("[链接名](https://a.com)"));
        assert!(md.contains("- 条目一"));
        assert!(md.contains("```rust\nfn main() {}\n```"));
    }

    #[test]
    fn empty_inputs() {
        assert_eq!(rich_to_md(""), "");
        assert_eq!(rich_to_plain(""), "");
        assert_eq!(article_html_to_md("<p>no editor</p>"), "");
    }
}

//! api.zsxq.com web 端逆向客户端。
//!
//! 签名：X-Signature = sha1("<path含query> <unix秒> <request_id>")
//! 头：X-Request-Id / X-Version(v2=2.99.0 v3=3.25.0) / X-Timestamp / X-Aduid
//! 认证：Cookie zsxq_access_token；权限等同网页浏览（不需要星球开 Skill）。
//! 注意：/v2/topics/{id} 已被风控(1059)，详情走 /v2/topics/{id}/info。

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client as Http;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fmt::{article_html_to_md, rich_to_md};

const HOST: &str = "https://api.zsxq.com";

fn browser_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        "accept",
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    h.insert(
        "sec-ch-ua",
        HeaderValue::from_static("\"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\""),
    );
    h.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
    h.insert("sec-ch-ua-platform", HeaderValue::from_static("\"Linux\""));
    h.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    h.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    h.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    h.insert("origin", HeaderValue::from_static("https://wx.zsxq.com"));
    h.insert("referer", HeaderValue::from_static("https://wx.zsxq.com/"));
    h.insert("User-Agent", HeaderValue::from_static(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"));
    h
}

pub struct ZsxqClient {
    http: Http,
    token: String,
    aduid: String,
}

impl ZsxqClient {
    pub fn new(token: &str, aduid: &str) -> Result<Self> {
        let http = Http::builder().default_headers(browser_headers()).build()?;
        Ok(Self {
            http,
            token: token.to_string(),
            aduid: aduid.to_string(),
        })
    }

    fn sign(&self, url_path: &str) -> HeaderMap {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let rid = uuid::Uuid::new_v4().to_string();
        let mut hasher = Sha1::new();
        hasher.update(format!("{} {} {}", url_path, ts, rid).as_bytes());
        let sig: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        let mut h = HeaderMap::new();
        h.insert("X-Request-Id", HeaderValue::from_str(&rid).unwrap());
        h.insert(
            "X-Version",
            HeaderValue::from_static(if url_path.contains("/v3/") {
                "3.25.0"
            } else {
                "2.99.0"
            }),
        );
        h.insert("X-Signature", HeaderValue::from_str(&sig).unwrap());
        h.insert(
            "X-Timestamp",
            HeaderValue::from_str(&ts.to_string()).unwrap(),
        );
        h.insert("X-Aduid", HeaderValue::from_str(&self.aduid).unwrap());
        h.insert(
            COOKIE,
            HeaderValue::from_str(&format!("zsxq_access_token={}", self.token)).unwrap(),
        );
        h
    }

    /// GET api.zsxq.com{path}，params 为 (key, value, encode?)。
    /// encode=true 时按 requests 同款规则 percent-encode（+→%2B :→%3A）——
    /// 签名必须对着编码后的 query 串算，跟浏览器一致。
    pub fn get(&self, path: &str, params: &[(&str, &str, bool)]) -> Result<Value> {
        self.request("GET", path, params, None)
    }

    /// POST JSON body；zsxq 写接口的 body 必须包一层 {"req_data": {...}}，
    /// 否则服务端读不到字段（报"空的评论内容"之类）。
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        Self::checked(self.request("POST", path, &[], Some(body))?)
    }

    /// DELETE。
    fn delete(&self, path: &str) -> Result<Value> {
        Self::checked(self.request("DELETE", path, &[], None)?)
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        params: &[(&str, &str, bool)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let mut url = format!("{}{}", HOST, path);
        if !params.is_empty() {
            let qs = params
                .iter()
                .map(|(k, v, e)| format!("{}={}", k, if *e { urlenc(v) } else { v.to_string() }))
                .collect::<Vec<_>>()
                .join("&");
            url = format!("{}?{}", url, qs);
        }
        let url_path = url[HOST.len()..].replace('\'', "%27");
        // 风控有间歇性 1059/抽风，最多自动重试 2 次
        let mut last = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(800));
            }
            let mut req = self
                .http
                .request(method.parse()?, &url)
                .headers(self.sign(&url_path));
            if let Some(b) = body {
                req = req.json(b);
            }
            let d: Value = req.send()?.json().context("响应非 JSON")?;
            if d.get("succeeded") == Some(&json!(true)) {
                return Ok(d);
            }
            // 业务参数错误（非风控）没有重试意义，直接抛
            if d.get("code").and_then(|v| v.as_i64()) != Some(1059) {
                return Ok(d);
            }
            last = err_msg(&d);
        }
        Ok(json!({"succeeded": false, "info": last}))
    }

    /// 校验：succeeded=false 时报错（含 1059 风控文案）。
    fn checked(d: Value) -> Result<Value> {
        if d.get("succeeded") == Some(&json!(true)) {
            return Ok(d);
        }
        bail!("{}", err_msg(&d))
    }

    fn resp(&self, path: &str, params: &[(&str, &str, bool)]) -> Result<Value> {
        Self::checked(self.get(path, params)?)
            .map(|d| d.get("resp_data").cloned().unwrap_or(json!({})))
    }

    fn topics_of(d: &Value) -> Vec<Value> {
        d.get("topics")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    // ---------- 星球 / 主题 ----------

    pub fn groups(&self) -> Result<Vec<Value>> {
        let d = self.resp(
            "/v2/groups",
            &[("scope", "all", true), ("count", "100", true)],
        )?;
        Ok(d.get("groups")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub fn topics_page(
        &self,
        gid: &str,
        scope: &str,
        count: u32,
        end_time: Option<&str>,
    ) -> Result<Vec<Value>> {
        let c = count.to_string();
        let mut p: Vec<(&str, &str, bool)> =
            vec![("scope", scope, true), ("count", c.as_str(), true)];
        if let Some(e) = end_time {
            p.push(("end_time", e, true));
        }
        let d = self.resp(&format!("/v2/groups/{}/topics", gid), &p)?;
        Ok(Self::topics_of(&d))
    }

    pub fn topic(&self, tid: &str) -> Result<Value> {
        match self.resp(&format!("/v2/topics/{}/info", tid), &[]) {
            Ok(d) => Ok(d.get("topic").cloned().unwrap_or(json!(null))),
            Err(_) => {
                let d = self.resp(&format!("/v2/topics/{}", tid), &[])?;
                Ok(d.get("topic").cloned().unwrap_or(json!(null)))
            }
        }
    }

    pub fn comments(&self, tid: &str, count: u32) -> Result<Vec<Value>> {
        let c = count.to_string();
        let d = self.resp(
            &format!("/v2/topics/{}/comments", tid),
            &[("count", c.as_str(), true)],
        )?;
        Ok(d.get("comments")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub fn sticky(&self, gid: &str) -> Result<Vec<Value>> {
        let d = self.resp(
            &format!("/v2/groups/{}/topics/sticky", gid),
            &[("count", "20", true)],
        )?;
        Ok(Self::topics_of(&d))
    }

    // ---------- 标签 ----------

    pub fn hashtags(&self, gid: &str) -> Result<Vec<Value>> {
        let d = self.resp(
            &format!("/v2/users/self/groups/{}/hashtags", gid),
            &[("sort_type", "by_time", true)],
        )?;
        Ok(d.get("hashtags")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub fn tag_topics(&self, hid: &str, end_time: Option<&str>) -> Result<Vec<Value>> {
        let mut p: Vec<(&str, &str, bool)> = vec![("count", "20", true)];
        if let Some(e) = end_time {
            p.push(("end_time", e, true));
        }
        let d = self.resp(&format!("/v2/hashtags/{}/topics", hid), &p)?;
        Ok(Self::topics_of(&d))
    }

    // ---------- 专栏（星主分类） ----------

    pub fn columns(&self, gid: &str) -> Result<Vec<Value>> {
        let d = self.resp(
            &format!("/v2/groups/{}/columns", gid),
            &[("count", "100", true)],
        )?;
        Ok(d.get("columns")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// 专栏下主题：服务端一次返回全部（忽略 count，实测 51 条全给）。
    pub fn column_topics(&self, gid: &str, cid: &str) -> Result<Vec<Value>> {
        let d = self.resp(
            &format!("/v2/groups/{}/columns/{}/topics", gid, cid),
            &[("count", "100", true)],
        )?;
        Ok(Self::topics_of(&d))
    }

    // ---------- 写操作（交互，实测通过） ----------

    /// 赞主题：body 服务端基本不校验，{"type":"like"} 即可。
    pub fn like_topic(&self, tid: &str) -> Result<()> {
        self.post(
            &format!("/v2/topics/{}/likes", tid),
            &json!({"type": "like"}),
        )
        .map(|_| ())
    }

    pub fn unlike_topic(&self, tid: &str) -> Result<()> {
        self.delete(&format!("/v2/topics/{}/likes", tid))
            .map(|_| ())
    }

    pub fn like_comment(&self, cid: &str) -> Result<()> {
        self.post(
            &format!("/v2/comments/{}/likes", cid),
            &json!({"req_data": {"type": "like"}}),
        )
        .map(|_| ())
    }

    pub fn unlike_comment(&self, cid: &str) -> Result<()> {
        self.delete(&format!("/v2/comments/{}/likes", cid))
            .map(|_| ())
    }

    /// 评论/回复（reply=被回复的 comment_id），返回 comment 对象。
    pub fn add_comment(&self, tid: &str, text: &str, reply: Option<&str>) -> Result<Value> {
        let mut rd = json!({"text": text});
        if let Some(r) = reply {
            rd["replied_comment_id"] = json!(r);
        }
        let d = self.post(
            &format!("/v2/topics/{}/comments", tid),
            &json!({"req_data": rd}),
        )?;
        Ok(d.get("resp_data")
            .and_then(|r| r.get("comment"))
            .cloned()
            .unwrap_or(json!({})))
    }

    /// 删除自己的评论。
    pub fn del_comment(&self, cid: &str) -> Result<()> {
        self.delete(&format!("/v2/comments/{}", cid)).map(|_| ())
    }

    pub fn favorite(&self, tid: &str) -> Result<()> {
        self.post("/v2/favorites", &json!({"req_data": {"topic_id": tid}}))
            .map(|_| ())
    }

    pub fn unfavorite(&self, tid: &str) -> Result<()> {
        self.delete(&format!("/v2/favorites/{}", tid)).map(|_| ())
    }

    /// 发帖（talk 纯文本），返回 topic 对象。
    pub fn post_topic(&self, gid: &str, text: &str, title: Option<&str>) -> Result<Value> {
        let mut rd = json!({"type": "talk", "text": text});
        if let Some(t) = title {
            rd["title"] = json!(t);
        }
        let d = self.post(
            &format!("/v2/groups/{}/topics", gid),
            &json!({"req_data": rd}),
        )?;
        Ok(d.get("resp_data")
            .and_then(|r| r.get("topic"))
            .cloned()
            .unwrap_or(json!({})))
    }

    /// 删除自己的主题。
    pub fn del_topic(&self, tid: &str) -> Result<()> {
        self.delete(&format!("/v2/topics/{}", tid)).map(|_| ())
    }

    // ---------- 我 / 星球信息 / 互动 ----------

    /// 当前用户资料。
    pub fn me(&self) -> Result<Value> {
        let d = self.resp("/v2/users/self", &[])?;
        Ok(d.get("user").cloned().unwrap_or(d))
    }

    /// 我的收藏：items 为 {favor_time, topic{...}}。
    pub fn favorites(&self, count: u32) -> Result<Vec<Value>> {
        let c = count.to_string();
        let d = self.resp("/v2/users/self/favorites", &[("count", c.as_str(), true)])?;
        Ok(d.get("favorites")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// 星球详情（名称/简介/编号/星主）。statistics 在独立端点。
    pub fn group_info(&self, gid: &str) -> Result<Value> {
        let d = self.resp(&format!("/v2/groups/{}", gid), &[])?;
        Ok(d.get("group").cloned().unwrap_or(d))
    }

    /// 星球统计（topics_count/digests_count/answers_count/tasks_count）。
    pub fn group_stats(&self, gid: &str) -> Result<Value> {
        self.resp(&format!("/v2/groups/{}/statistics", gid), &[])
    }

    /// 星球成员（只读名单）。
    pub fn members(&self, gid: &str, count: u32) -> Result<Vec<Value>> {
        let c = count.to_string();
        let d = self.resp(
            &format!("/v2/groups/{}/members", gid),
            &[("count", c.as_str(), true)],
        )?;
        Ok(d.get("members")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// 主题点赞名单。
    pub fn likes(&self, tid: &str, count: u32) -> Result<Vec<Value>> {
        let c = count.to_string();
        let d = self.resp(
            &format!("/v2/topics/{}/likes", tid),
            &[("count", c.as_str(), true)],
        )?;
        Ok(d.get("likes")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// 主题赞赏名单。
    pub fn rewards(&self, tid: &str, count: u32) -> Result<Vec<Value>> {
        let c = count.to_string();
        let d = self.resp(
            &format!("/v2/topics/{}/rewards", tid),
            &[("count", c.as_str(), true)],
        )?;
        Ok(d.get("rewards")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    // ---------- 搜索 ----------

    pub fn search(&self, kw: &str, gid: Option<&str>) -> Result<Vec<Value>> {
        let d = match gid {
            Some(g) => self.resp(
                &format!("/v2/search/groups/{}/topics", g),
                &[("keyword", kw, true), ("count", "20", true)],
            )?,
            None => self.resp(
                "/v2/search/topics",
                &[("keyword", kw, true), ("count", "20", true)],
            )?,
        };
        Ok(Self::topics_of(&d))
    }

    // ---------- 文件 ----------

    pub fn group_files(&self, gid: &str, count: u32) -> Result<Vec<Value>> {
        let c = count.to_string();
        let d = self.resp(
            &format!("/v2/groups/{}/files", gid),
            &[("count", c.as_str(), true)],
        )?;
        Ok(d.get("files")
            .cloned()
            .unwrap_or(json!([]))
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub fn file_download_url(&self, fid: &str) -> Result<String> {
        let d = self.resp(&format!("/v2/files/{}/download_url", fid), &[])?;
        d.get("download_url")
            .and_then(|u| u.as_str())
            .map(|s| s.to_string())
            .context("download_url 为空")
    }

    pub fn download(&self, fid: &str, dest: &Path) -> Result<u64> {
        let url = self.file_download_url(fid)?;
        let mut r = self.http.get(&url).send()?.error_for_status()?;
        let mut f = std::fs::File::create(dest)?;
        r.copy_to(&mut f)?;
        f.flush()?;
        Ok(dest.metadata()?.len())
    }

    // ---------- 长文章 ----------

    /// articles.zsxq.com/inline_form/id_<aid>.html：服务端渲染，只要 cookie。
    pub fn article_markdown(&self, article_id: &str) -> Result<String> {
        let url = format!(
            "https://articles.zsxq.com/inline_form/id_{}.html",
            article_id
        );
        let r = self
            .http
            .get(&url)
            .header(COOKIE, format!("zsxq_access_token={}", self.token))
            .send()?
            .error_for_status()?;
        let html = r.text()?;
        let md = article_html_to_md(&html);
        if md.is_empty() {
            bail!("文章 HTML 解析为空（页面结构变化或未登录）");
        }
        Ok(md)
    }

    /// 主题正文：普通帖转 <e> 标签；文章帖拉全文。
    pub fn topic_full_text(&self, topic: &Value) -> String {
        let body = ["talk", "question", "answer", "task"]
            .iter()
            .find_map(|k| topic.get(*k))
            .cloned()
            .unwrap_or(json!({}));
        if let Some(aid) = body
            .get("article")
            .and_then(|a| a.get("article_id"))
            .and_then(|x| x.as_str())
        {
            if let Ok(md) = self.article_markdown(aid) {
                return md;
            }
        }
        rich_to_md(body.get("text").and_then(|t| t.as_str()).unwrap_or(""))
    }
}

/// 错误文案：优先 info，其次 error（部分接口错误放 error 字段），最后 code。
fn err_msg(d: &Value) -> String {
    for k in ["info", "error"] {
        if let Some(s) = d.get(k).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    d.get("code")
        .map(|c| format!("请求失败 (code={})", c))
        .unwrap_or_else(|| "请求失败".into())
}

fn urlenc(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

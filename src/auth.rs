//! 认证：cookie/裸 token 解析 + 多账号持久化。
//!
//! 存储：~/.config/zsxq/accounts.json（0600）
//!   {"active": "<name>", "accounts": {"<name>": {"token": "...", "aduid": "..."}}}
//! 旧版单 token 文件（token/aduid）首次访问自动迁移为 default 账号。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zsxq")
}

fn store_path() -> PathBuf {
    dir().join("accounts.json")
}

/// 存储文件路径（展示用）。
pub fn store_display() -> String {
    store_path().display().to_string()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub token: String,
    pub aduid: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    active: Option<String>,
    accounts: HashMap<String, Account>,
}

fn load_store() -> Store {
    if let Ok(s) = fs::read_to_string(store_path()) {
        if let Ok(st) = serde_json::from_str(&s) {
            return st;
        }
    }
    // 迁移旧版单 token 文件
    let legacy_token = dir().join("token");
    if let Ok(t) = fs::read_to_string(&legacy_token) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            let aduid = fs::read_to_string(dir().join("aduid"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
            let mut st = Store::default();
            st.accounts
                .insert("default".into(), Account { token: t, aduid });
            st.active = Some("default".into());
            let _ = save_store(&st);
            let _ = fs::remove_file(&legacy_token);
            let _ = fs::remove_file(dir().join("aduid"));
            return st;
        }
    }
    Store::default()
}

fn save_store(st: &Store) -> Result<()> {
    let p = store_path();
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(&p, serde_json::to_string_pretty(st)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// 当前激活账号：(name, account)。
pub fn active() -> Option<(String, Account)> {
    let st = load_store();
    st.active
        .and_then(|n| st.accounts.get(&n).map(|a| (n, a.clone())))
}

/// 按名取账号。
pub fn account(name: &str) -> Option<Account> {
    load_store().accounts.get(name).cloned()
}

/// 登录态解析：--account 指定 > 激活账号。
pub fn resolve(name: Option<&str>) -> Result<(String, Account)> {
    if let Some(n) = name {
        return account(n)
            .map(|a| (n.to_string(), a))
            .with_context(|| format!("账号 '{}' 不存在，`zsxq auth list` 查看", n));
    }
    active().context("未登录：`zsxq auth login` 粘贴 cookie/token")
}

/// 新登录/覆盖账号，设为激活。
pub fn login(name: &str, token: &str) -> Result<()> {
    let mut st = load_store();
    let aduid = st
        .accounts
        .get(name)
        .map(|a| a.aduid.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    st.accounts.insert(
        name.to_string(),
        Account {
            token: token.to_string(),
            aduid,
        },
    );
    st.active = Some(name.to_string());
    save_store(&st)
}

/// (名字列表, 激活名)
pub fn list() -> (Vec<String>, Option<String>) {
    let st = load_store();
    let mut names: Vec<String> = st.accounts.keys().cloned().collect();
    names.sort();
    (names, st.active)
}

/// 切换激活账号。
pub fn set_active(name: &str) -> Result<()> {
    let mut st = load_store();
    if !st.accounts.contains_key(name) {
        bail!("账号 '{}' 不存在，`zsxq auth list` 查看", name);
    }
    st.active = Some(name.to_string());
    save_store(&st)
}

/// 删除账号；删的是激活号则自动切到剩余第一个。
pub fn remove(name: &str) -> Result<()> {
    let mut st = load_store();
    if st.accounts.remove(name).is_none() {
        bail!("账号 '{}' 不存在", name);
    }
    if st.active.as_deref() == Some(name) {
        let mut rest: Vec<String> = st.accounts.keys().cloned().collect();
        rest.sort();
        st.active = rest.first().cloned();
    }
    save_store(&st)
}

/// 输入可以是：整串 cookie（自动提取 zsxq_access_token）、
/// `zsxq_access_token=xxx`、或裸 token。
pub fn extract_token(input: &str) -> Option<String> {
    let s = input.trim().trim_matches('"').trim_matches('\'');
    for part in s.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("zsxq_access_token=") {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    // 裸 token：形如 UUID_XXXX，无空白/分号/等号
    if !s.contains(' ') && !s.contains(';') && !s.contains('=') && s.len() >= 20 {
        return Some(s.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_token;

    const TOK: &str = "DEADBEEF-0000-4DF1-813A-TESTTOKEN000_CAFE0011F00D0000";

    #[test]
    fn full_cookie_string() {
        let c = format!(
            "_ga=GA1.1.1.1; zsxq_access_token={}; abtest_env=product",
            TOK
        );
        assert_eq!(extract_token(&c).unwrap(), TOK);
    }

    #[test]
    fn cookie_pair_only() {
        let s = format!("zsxq_access_token={}", TOK);
        assert_eq!(extract_token(&s).unwrap(), TOK);
    }

    #[test]
    fn bare_token_and_quotes() {
        assert_eq!(extract_token(TOK).unwrap(), TOK);
        assert_eq!(extract_token(&format!("\"{}\"", TOK)).unwrap(), TOK);
    }

    #[test]
    fn rejects_garbage() {
        assert!(extract_token("hello world").is_none());
        assert!(extract_token("").is_none());
        assert!(extract_token("other_token=abc123").is_none());
        // 短裸串（<20 字符）不应当 token
        assert!(extract_token("abcdefghij").is_none());
    }
}

//! 认证：cookie/裸 token 解析 + 本地持久化（~/.config/zsxq/token，0600）。

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

pub fn token_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zsxq")
        .join("token")
}

/// 设备指纹 id，签名头 X-Aduid 用（持久化，模拟同一台设备）。
pub fn aduid_path() -> PathBuf {
    token_path().with_file_name("aduid")
}

pub fn load() -> Option<String> {
    fs::read_to_string(token_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn load_or_err() -> Result<String> {
    load().context("未登录：先 `zsxq auth login` 粘贴 cookie/token")
}

pub fn save(token: &str) -> Result<()> {
    let p = token_path();
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(&p, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn remove() -> Result<()> {
    let p = token_path();
    if p.exists() {
        fs::remove_file(p)?;
    }
    Ok(())
}

pub fn load_or_gen_aduid() -> String {
    if let Ok(s) = fs::read_to_string(aduid_path()) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = save_aduid(&id);
    id
}

fn save_aduid(id: &str) -> Result<()> {
    let p = aduid_path();
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(p, id)?;
    Ok(())
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

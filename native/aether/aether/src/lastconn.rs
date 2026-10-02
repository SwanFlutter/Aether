use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LastConnection {
    pub peer: String,
    #[serde(default)]
    pub profile: String,
    // >>> AETHER-APP-PATCH scan-once-after-a-slow-cache
    /// RTTی که نشستِ قبل با اکراه پذیرفت، وقتی این endpoint جواب داد ولی
    /// کندتر از بودجه بود. صفر یعنی هیچ — یعنی این کش سریع بود.
    ///
    /// # چرا روی دیسک می‌ماند
    ///
    /// لاگِ ۱۷ سپتامبر (`loge1.txt`) دو نشستِ پشت‌سرهم دارد که هر دو همان
    /// `162.159.192.163:859` را برداشتند — ۹۲۶ms و بعد ۶۱۱ms — و هیچ‌کدام اسکن
    /// نکرد، در حالی که خودِ لاگ وعده داده بود: «the next session will look for
    /// something faster». آن وعده جایی برای ماندن نداشت؛ این فیلد همان جاست.
    #[serde(default)]
    pub slow_rtt_ms: u64,
    // <<< AETHER-APP-PATCH scan-once-after-a-slow-cache
}

pub fn load(path: &str) -> Option<LastConnection> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

pub fn save(path: &str, peer: &str, profile: &str) {
    save_slow(path, peer, profile, 0)
}

/// همانِ `save`، و کنارش این که این endpoint با اکراه پذیرفته شده یا نه.
pub fn save_slow(path: &str, peer: &str, profile: &str, slow_rtt_ms: u64) {
    let conn = LastConnection {
        peer: peer.to_string(),
        profile: profile.to_string(),
        slow_rtt_ms,
    };
    match toml::to_string_pretty(&conn) {
        Ok(text) => {
            if let Err(e) = std::fs::write(path, text) {
                log::debug!("[lastconn] failed to save {path}: {e}");
            }
        }
        Err(e) => log::debug!("[lastconn] failed to encode: {e}"),
    }
}

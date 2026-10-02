//! ارائه‌دهنده‌های هوش مصنوعی — جمینای، OpenAI، و هر سازگارِ OpenAI با نشانی دلخواه.
//!
//! # چرا یک ثبت‌نام و نه یک سوییچ ساده
//!
//! لایهٔ AI تا ۱.۲.۶ فقط جمینای می‌شناخت: میزبان در `ai_http::HOST` ثابت نوشته
//! شده بود و کلید در `GEMINI_KEY`. کاربرانی که کلیدشان از Gemini نیست (یا
//! Gemini از IP خروجی‌شان را رد می‌کند) هیچ مسیری نداشتند. این فایل همان
//! میزبانِ ثابت را به دادهٔ کاربر تبدیل می‌کند — با همان قاعده‌ای که کل
//! برنامه دارد: هر ورودیِ کاربری **پیش از رسیدن به شبکه** اعتبارسنجی می‌شود.
//!
//! # قاعده‌های امنیتیِ نشانی
//!
//! * فقط `https`. یک کلید API روی `http` یعنی ارسالِ صریحِ راز در مسیر، و
//!   مسیرِ اینجا از تونل کاربر بیرون می‌رود.
//! * بدون `user:pass@`، بدون query، بدون fragment. هر سه فقط راه‌های
//!   دورزدنِ بازبینیِ میزبان‌اند.
//! * پورت اختیاری و معتبر.
//!
//! خودِ درخواست هرگز بیرون از تونل زده نمی‌شود — همان مسیر `ai_gate` که برای
//! جمینای بود، برای همهٔ ارائه‌دهنده‌ها برجا می‌ماند.

use serde::{Deserialize, Serialize};

/// شناسهٔ ارائه‌دهندهٔ پیش‌فرضِ برنامه — همانی که تا ۱.۲.۶ تنها وجود داشت.
pub const GEMINI_ID: &str = "gemini";
/// شناسهٔ ارائه‌دهندهٔ آمادهٔ OpenAI.
pub const OPENAI_ID: &str = "openai";
/// شناسهٔ ارائه‌دهندهٔ آمادهٔ Anthropic (Claude).
pub const CLAUDE_ID: &str = "claude";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderKind {
    /// REST API گوگل: `x-goog-api-key`، مسیر `models/:generateContent`،
    /// و **فهرست مجاز مدل‌ها** (`ai_model_policy`).
    Gemini,
    /// هر چیزی که `/models` و `/chat/completions` را مثل OpenAI حرف می‌زند —
    /// خودِ api.openai.com، OpenRouter، و پنل‌های سازگارِ خودمیزبان.
    ///
    /// `alias` برای «OPENAI»: SCREAMING_SNAKE_CASE از نامِ variant خودِ
    /// «OpenAI» مقدار `OPEN_AI` می‌سازد و یک رابطِ فراموش‌کار که یک‌بار
    /// `OPENAI` بفرستد، با خطای deserialization بی‌معنی برای کاربر برمی‌گشت.
    #[serde(alias = "OPENAI")]
    OpenAi,
    /// REST API آنتروپیک: `x-api-key` + `anthropic-version`، مسیر
    /// `/v1/models` و `/v1/messages`.
    Anthropic,
}

/// یک ارائه‌دهندهٔ ثبت‌شده. `host` و `base_path` از `baseUrl` در لحظهٔ
/// ثبت ساخته می‌شوند و هرگز خام به شبکه نمی‌روند.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub display_name: String,
    pub kind: ProviderKind,
    pub host: String,
    /// پیشوندِ مسیر، با `/` آغازین و بدون `/` انتهایی. `""` یعنی ریشه.
    pub base_path: String,
    /// ارائه‌دهندهٔ آماده‌ای که کاربر نمی‌تواند حذفش کند.
    #[serde(default)]
    pub builtin: bool,
}

/// دو ارائه‌دهندهٔ آمادهٔ برنامه. ترتیب، ترتیبِ نمایش است.
pub fn presets() -> Vec<Provider> {
    vec![
        Provider {
            id: GEMINI_ID.to_string(),
            display_name: "Google Gemini".to_string(),
            kind: ProviderKind::Gemini,
            host: "generativelanguage.googleapis.com".to_string(),
            base_path: "/v1beta".to_string(),
            builtin: true,
        },
        Provider {
            id: OPENAI_ID.to_string(),
            display_name: "OpenAI".to_string(),
            kind: ProviderKind::OpenAi,
            host: "api.openai.com".to_string(),
            base_path: "/v1".to_string(),
            builtin: true,
        },
        Provider {
            id: CLAUDE_ID.to_string(),
            display_name: "Claude".to_string(),
            kind: ProviderKind::Anthropic,
            host: "api.anthropic.com".to_string(),
            base_path: "/v1".to_string(),
            builtin: true,
        },
    ]
}

/// همان قاعده‌ای که رابط کاربری می‌گوید: حرف کوچک، رقم، خط تیره یا زیرخط.
/// دو تا ۳۲ نویسه — تا `aiKey:{id}` از سقفِ نام‌های SecretStore بیرون نزند.
fn valid_id(id: &str) -> bool {
    (2..=32).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// یک `baseUrl` را به (host, base_path) می‌شکند.
///
/// پارس‌کنندهٔ دستی و نه یک crate: کل برنامه هیچ وابستگی URL ندارد و افزودن
/// یکی برای تفکیکِ چهار جزءِ یک نشانی، همان کاری است که `ai_http` برای HTTP
/// نمی‌کند.
pub fn parse_base_url(raw: &str) -> Result<(String, String), String> {
    let value = raw.trim();
    let rest = match value {
        v if v.starts_with("https://") => &v[8..],
        v if v.starts_with("http://") => {
            return Err("Only https:// addresses are accepted for an AI provider.".to_string())
        }
        _ => return Err("The provider address must start with https://".to_string()),
    };
    if rest.contains('@') {
        return Err("The provider address must not carry credentials.".to_string());
    }
    let rest = match rest.split_once('#') {
        Some((head, _)) => head,
        None => rest,
    };
    if rest.contains('?') {
        return Err("The provider address must not carry a query string.".to_string());
    }
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{p}")),
        None => (rest, String::new()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        // `[::1]:8443` — اگر `:` پایانی بخشی از آدرس IPv6 بود، میزبان در
        // براکت است و `rsplit_once` براکتِ بسته را جدا می‌کند؛ پس براکت را
        // پیش از هر چیزی بررسی می‌کنیم.
        Some((h, p)) if !h.is_empty() && !h.ends_with(']') => {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || p.len() > 5 {
                return Err("The provider address has an invalid port.".to_string());
            }
            let number: u16 = p
                .parse()
                .map_err(|_| "The provider address has an invalid port.".to_string())?;
            if number == 0 {
                return Err("The provider address has an invalid port.".to_string());
            }
            (h.to_string(), Some(number))
        }
        _ => (authority.to_string(), None),
    };
    let host = host.to_ascii_lowercase();
    if host.len() < 3 || host.len() > 253 {
        return Err("The provider address has no usable host.".to_string());
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
    {
        return Err("The provider address has an invalid host.".to_string());
    }
    let authority = match port {
        Some(p) => format!("{host}:{p}"),
        None => host,
    };
    let base_path = path.trim_end_matches('/').to_string();
    Ok((authority, base_path))
}

/// نامِ رازِ این ارائه‌دهنده در SecretStore.
///
/// جمینای همان `geminiApiKey` قدیمی را نگه می‌دارد تا کلیدِ ذخیره‌شدهٔ هیچ
/// نصبِ موجودی با ارتقا گم نشود.
pub fn secret_name(id: &str) -> String {
    if id == GEMINI_ID {
        crate::secret_store::GEMINI_KEY.to_string()
    } else {
        format!("aiKey:{id}")
    }
}

/// فهرستِ کامل: پیش‌ساخته‌ها + ثبت‌شده‌های کاربر، بدون تکرارِ شناسه.
pub fn merge(custom: &[Provider]) -> Vec<Provider> {
    let mut out = presets();
    for p in custom {
        if !out.iter().any(|e| e.id == p.id) {
            out.push(p.clone());
        }
    }
    out
}

/// یک ارائه‌دهندهٔ تازه را از ورودیِ خامِ رابط کاربری می‌سازد.
///
/// خطاها جمله‌اند و مستقیم به کاربر نشان داده می‌شوند — همان قراردادِ
/// `AiSession`: هیچ `Err`ی نباید به یک «مشکلی پیش آمد» تبدیل شود.
pub fn build(id: &str, display_name: &str, kind: ProviderKind, base_url: &str) -> Result<Provider, String> {
    let id = id.trim().to_ascii_lowercase();
    if !valid_id(&id) {
        return Err(
            "Provider ID must be 2-32 lowercase letters, digits, hyphens or underscores."
                .to_string(),
        );
    }
    if presets().iter().any(|p| p.id == id) {
        return Err(format!("\"{id}\" is reserved for a built-in provider."));
    }
    let (host, base_path) = parse_base_url(base_url)?;
    let display = display_name.trim().to_string();
    let display = if display.is_empty() { id.clone() } else { display };
    Ok(Provider {
        id,
        display_name: display,
        kind,
        host,
        base_path,
        builtin: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_https_endpoint_with_prefix() {
        assert_eq!(
            parse_base_url("https://api.myp-provider.com/v1/").unwrap(),
            ("api.myp-provider.com".to_string(), "/v1".to_string())
        );
    }

    #[test]
    fn accepts_a_port_and_a_root_path() {
        assert_eq!(
            parse_base_url("https://10.0.0.5:8443").unwrap(),
            ("10.0.0.5:8443".to_string(), String::new())
        );
    }

    #[test]
    fn rejects_everything_that_could_leak_a_key_or_bypass_the_host_check() {
        assert!(parse_base_url("http://api.example.com/v1").is_err());
        assert!(parse_base_url("api.example.com/v1").is_err());
        assert!(parse_base_url("https://user:pass@api.example.com").is_err());
        assert!(parse_base_url("https://api.example.com?k=1").is_err());
        assert!(parse_base_url("https://api.example.com:0").is_err());
        assert!(parse_base_url("https://api.example.com:99999").is_err());
    }

    #[test]
    fn builtins_keep_their_old_secret_name() {
        assert_eq!(secret_name(GEMINI_ID), crate::secret_store::GEMINI_KEY);
        assert_eq!(secret_name("myp"), "aiKey:myp");
    }

    #[test]
    fn a_custom_provider_survives_the_round_trip() {
        let p = build(
            "MyP",
            "",
            ProviderKind::OpenAi,
            "https://api.myp.example/v1",
        )
        .unwrap();
        assert_eq!(p.id, "myp");
        assert_eq!(p.display_name, "myp");
        assert_eq!(p.host, "api.myp.example");
        assert_eq!(p.base_path, "/v1");
        assert!(build("gemini", "x", ProviderKind::OpenAi, "https://a.example").is_err());
        assert!(build("A", "x", ProviderKind::OpenAi, "https://a.example").is_err());
    }

    #[test]
    fn merge_never_duplicates_an_id() {
        let custom = vec![
            presets()[0].clone(),
            build("myp", "My", ProviderKind::OpenAi, "https://a.example").unwrap(),
        ];
        let all = merge(&custom);
        assert_eq!(all.len(), 4);
        assert_eq!(all.iter().filter(|p| p.id == GEMINI_ID).count(), 1);
    }

    #[test]
    fn the_openai_kind_accepts_both_spellings() {
        let exact: ProviderKind = serde_json::from_str("\"OPEN_AI\"").unwrap();
        assert_eq!(exact, ProviderKind::OpenAi);
        let loose: ProviderKind = serde_json::from_str("\"OPENAI\"").unwrap();
        assert_eq!(loose, ProviderKind::OpenAi);
        let claude: ProviderKind = serde_json::from_str("\"ANTHROPIC\"").unwrap();
        assert_eq!(claude, ProviderKind::Anthropic);
    }
}

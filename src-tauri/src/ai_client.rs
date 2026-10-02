//! پورت ۱:۱ از `ai/GeminiClient.kt` — کلاینت نازک و بی‌وابستگیِ REST API،
//! حالا با چهرهٔ چند-ارائه‌دهنده: جمینای و هر چیزی که `/models` و
//! `/chat/completions` را مثل OpenAI حرف می‌زند.
//!
//! JSON با `serde_json` ساخته و پارس می‌شود، که از قبل در این مخزن هست (پروفایل
//! و snapshot با همان سریالایز می‌شوند)، پس قابلیت‌های هوش مصنوعی **صفر**
//! وابستگی تازه به نصابی اضافه می‌کنند که کاربرانش هر دلیلی دارند نگران محتوایش
//! باشند. در سمت اندروید همین نقش را `org.json` پلتفرم بازی می‌کرد.
//!
//! هر فراخوانی بلوکه‌کننده است و **باید** بیرون از رشتهٔ رابط کاربری اجرا شود؛
//! [`crate::ai_session`] همه را روی رشتهٔ کارگر خودش می‌برد.

use crate::ai_http;
use crate::ai_model_policy as policy;
use crate::ai_provider::{Provider, ProviderKind};
use crate::log::DiagnosticsLog;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// یک مدل که کلیدِ کاربر مجاز به دیدنش است — در هر دو قالبِ API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    /// شناسهٔ خالی، مثل `gemini-3.8-flash` (پیشوند `models/` کنده شده).
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub input_token_limit: u32,
    pub output_token_limit: u32,
    /// وقتی true است این مدل می‌تواند پاسخ گفت‌وگو بدهد.
    pub chat_capable: bool,
    /// رایگان-شدنِ مدل، آن‌طور که خودِ سرویس اعلام کرده.
    ///
    /// جمینای: همهٔ مدل‌های فهرست مجاز روی تیر رایگان جواب می‌دهند، پس true.
    /// OpenAI-compatible: نشانهٔ `:free` در شناسه (قرارداد OpenRouter) یا
    /// `pricing` صفر. وقتی هیچ نشانه‌ای نیست false است — «رایگان» ادعایی است
    /// که فقط سرویس می‌تواند بکند، حدسِ ما جای آن نیست.
    pub free: bool,
}

/// یک نوبت از یک گفت‌وگو.
#[derive(Debug, Clone)]
pub struct AiTurn {
    pub from_user: bool,
    pub text: String,
}

/// چه چیزی غلط شد، در همان درشت‌دانگی که رابط کاربری واقعاً به آن واکنش می‌دهد.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiErrorKind {
    /// کلید رد شد (۴۰۱/۴۰۳، یا «API key not valid» خودِ گوگل).
    BadKey,
    /// سهمیه یا محدودیت نرخ (۴۲۹).
    RateLimit,
    /// شناسهٔ مدل برای این کلید ناشناخته است (۴۰۴).
    NoSuchModel,
    /// درخواست هرگز به گوگل نرسید: پروکسی رد کرد، TLS شکست، تایم‌اوت.
    Transport,
    /// به گوگل رسیدیم و او سمت خودش شکست (۵xx).
    ///
    /// از [`AiErrorKind::Transport`] جدا شده چون این دو به کلمات مخالف نیاز
    /// دارند. یک ۵۰۰ به کاربر به‌عنوان «نتوانستیم از تونل به گوگل برسیم» گزارش
    /// می‌شد، که مردم را می‌فرستاد تونلی را دوباره بررسی کنند که بی‌عیب کار
    /// می‌کرد — درخواست تا خودِ گوگل رفته و برگشته بود. این هم تنها ردهٔ شکستی
    /// است که یک retry ساده درستش می‌کند، و همین است که «دوباره تلاش کن» همیشه
    /// کار می‌کرد و هیچ چیز دیگری نه.
    ServerError,
    /// گوگل جواب داد، با چیزی که این کلاینت نمی‌توانست استفاده کند.
    Protocol,
    /// پاسخ وسط تولید بریده شد (`finishReason: MAX_TOKENS`).
    ///
    /// ردهٔ خودش را دارد چون شکستِ پشتِ یک پاسخ چتِ نصفه و یک پاسخ مشاور که
    /// به‌عنوان JSON پارس نمی‌شود، همین است: متن خراب نیست، **ناقص** است، و
    /// راه‌حلش بودجهٔ خروجی بزرگ‌تر است نه مدل دیگر یا کلید دیگر.
    Truncated,
    /// گوگل بنا به دلایل ایمنی از پاسخ‌دادن خودداری کرد.
    Blocked,
}

impl AiErrorKind {
    /// کدام شکست‌ها را یک تلاش دوم می‌تواند به‌طور موجه درست کند.
    ///
    /// کلیدِ ردشده، مدل ناشناخته و ردِ ایمنی جبری‌اند: تکرارشان وقت و سهمیهٔ
    /// کاربر را هدر می‌دهد تا به همان پاسخ برسد. یک ۵xx، یک محدودیت نرخ و یک
    /// سوکتِ افتاده نه.
    fn worth_retrying(self) -> bool {
        matches!(
            self,
            AiErrorKind::ServerError | AiErrorKind::RateLimit | AiErrorKind::Transport
        )
    }
}

/// خطایی که همیشه یک جملهٔ قابل‌نمایش دارد.
///
/// یک نتیجهٔ ساختاریافته و نه صرفاً `anyhow`: هر شکستی اینجا باید در نهایت به
/// یک جمله روی صفحه تبدیل شود. پرت‌کردنِ خطا، این ترجمه را به شش نقطهٔ فراخوانی
/// مختلف هل می‌داد و تضمین می‌کرد یکی‌شان یک `SSLPeerUnverifiedException` خام را
/// به کسی نشان دهد که فقط می‌خواست بداند MTU چیست.
#[derive(Debug, Clone)]
pub struct AiError {
    pub message: String,
    pub kind: AiErrorKind,
    /// انتظارِ پیشنهادیِ سرور، وقتی شکست یکی همراه داشت. backoff را می‌راند.
    pub retry_after_seconds: Option<f64>,
}

impl AiError {
    fn new(message: impl Into<String>, kind: AiErrorKind) -> Self {
        Self {
            message: message.into(),
            kind,
            retry_after_seconds: None,
        }
    }
}

pub type AiResult<T> = Result<T, AiError>;

const MAX_ATTEMPTS: u32 = 3;
const BASE_BACKOFF_MS: u64 = 700;
const MAX_BACKOFF_MS: u64 = 6_000;
const MAX_MODEL_PAGES: u32 = 5;
/// یک نشستِ HTTP کامل. طولانی چون درخواست از **دو** تونل رد می‌شود.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// سرآیند احراز هویتِ این ارائه‌دهنده.
fn auth(provider: &Provider, api_key: &str) -> (&'static str, String) {
    match provider.kind {
        ProviderKind::Gemini => ("x-goog-api-key", api_key.to_string()),
        ProviderKind::OpenAi => ("Authorization", format!("Bearer {api_key}")),
        ProviderKind::Anthropic => ("x-api-key", api_key.to_string()),
    }
}

/// هدرهای الحاقیِ ثابتِ این ارائه‌دهنده.
///
/// آنتروپیک `anthropic-version` را در **هر** درخواست می‌خواهد و بدون آن ۴۰۰
/// برمی‌گرداند؛ نسخهٔ تاریخ‌دار همان قراردادِ بیدار‌نبودنِ API است، پس اینجا
/// ثابت می‌ماند تا یک تغییر بی‌سر‌و‌صدا رفتارِ همهٔ نصب‌ها را عوض نکند.
fn extra_headers(provider: &Provider) -> &'static [(&'static str, &'static str)] {
    match provider.kind {
        ProviderKind::Gemini | ProviderKind::OpenAi => &[],
        ProviderKind::Anthropic => &[("anthropic-version", "2023-06-01")],
    }
}

/// مدل‌هایی که *این* کلید می‌تواند استفاده کند را فهرست می‌کند.
///
/// کل نکتهٔ این قابلیت: یک کلید رایگان، یک کلید با صورت‌حساب فعال و یک کلید از
/// منطقه‌ای که مدلی در آن عرضه نشده، سه فهرست متفاوت می‌بینند؛ پس یک انتخابگرِ
/// ثابت‌نوشته مدل‌هایی را پیشنهاد می‌کرد که برای نیمی از کاربران ۴۰۴ می‌دهند. ما
/// از خودِ کلید می‌پرسیم چه می‌تواند بکند.
pub fn list_models(provider: &Provider, api_key: &str, socks_port: u16) -> AiResult<Vec<AiModel>> {
    match provider.kind {
        ProviderKind::Gemini => list_gemini(provider, api_key, socks_port),
        ProviderKind::OpenAi => list_openai(provider, api_key, socks_port),
        ProviderKind::Anthropic => list_anthropic(provider, api_key, socks_port),
    }
}

fn list_gemini(provider: &Provider, api_key: &str, socks_port: u16) -> AiResult<Vec<AiModel>> {
    let api = format!("{}/models", provider.base_path);
    let mut collected: Vec<AiModel> = Vec::new();
    let mut page_token: Option<String> = None;
    let mut page = 0u32;
    loop {
        let suffix = match &page_token {
            Some(t) if !t.is_empty() => format!("&pageToken={t}"),
            _ => String::new(),
        };
        let body = call(
            provider,
            "GET",
            &format!("{api}?pageSize=200{suffix}"),
            socks_port,
            api_key,
            None,
        )?;
        let json: Value = serde_json::from_str(&body).map_err(|_| {
            AiError::new("Google's reply was not valid JSON.", AiErrorKind::Protocol)
        })?;
        if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
            for item in models {
                collected.push(to_model(item));
            }
        }
        page_token = json
            .get("nextPageToken")
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string());
        page += 1;
        // پیمایش را محدود کن: یک nextPageToken فراری نباید «بازکردن انتخابگر
        // مدل» را به یک حلقهٔ بی‌کران روی یک اتصال حجمی تبدیل کند.
        if page_token.is_none() || page >= MAX_MODEL_PAGES {
            break;
        }
    }

    // فهرست مجاز **اینجا** اعمال می‌شود و نه در انتخابگر: شناسه‌ای که هرگز وارد
    // برنامه نشود، نمی‌تواند انتخاب، کَش یا فرستاده شود. این تشریح فقط مالِ
    // جمیناست — یک endpoint سازگارِ OpenAI فهرستِ خودش را می‌دهد و آن فهرست
    // دقیقاً همان چیزی است که کلیدِ کاربر مستحقِ دیدنش است.
    let chat_capable: Vec<AiModel> = collected
        .iter()
        .filter(|m| m.chat_capable)
        .cloned()
        .collect();
    let offered = policy::filter(&chat_capable);
    DiagnosticsLog::i(
        "ai",
        &format!(
            "models: {} returned by the key, {} offered after the allow-list",
            collected.len(),
            offered.len()
        ),
    );
    Ok(offered)
}

/// `GET {base}/models` سازگارِ OpenAI.
///
/// سه قالب در طبیعت دیده شده: `{"data":[...]}` (OpenAI، OpenRouter)، آرایهٔ خام
/// در ریشه (بسیاری از پنل‌های خودمیزبان)، و `{"models":[...]}`. هر سه پذیرفته
/// می‌شوند چون فرستنده‌شان یک *سازگار* است و سازگاری یعنی دقیقاً همین
/// پراکندگی. چیزی که پارس نشود خطای Protocol می‌گیرد، نه فهرستِ خالیِ بی‌صدا —
/// کاربر باید بفهمد کلیدش جواب داده ولی پاسخ را نفهمیده‌ایم.
fn list_openai(provider: &Provider, api_key: &str, socks_port: u16) -> AiResult<Vec<AiModel>> {
    let body = call(
        provider,
        "GET",
        &format!("{}/models", provider.base_path),
        socks_port,
        api_key,
        None,
    )?;
    let json: Value = serde_json::from_str(&body).map_err(|_| {
        AiError::new("The provider's reply was not valid JSON.", AiErrorKind::Protocol)
    })?;
    let items = json
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| json.as_array())
        .or_else(|| json.get("models").and_then(|m| m.as_array()))
        .cloned()
        .ok_or_else(|| {
            AiError::new(
                "The provider returned no model list in a form this app understands.",
                AiErrorKind::Protocol,
            )
        })?;
    let mut out: Vec<AiModel> = Vec::new();
    for item in &items {
        let Some(model) = to_openai_model(item) else {
            continue;
        };
        if !out.iter().any(|m: &AiModel| m.id == model.id) {
            out.push(model);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    DiagnosticsLog::i("ai", &format!("models: {} offered by the endpoint", out.len()));
    Ok(out)
}

/// یک سطرِ `/models` را به مدل تبدیل می‌کند؛ `None` یعنی «این یک مدلِ گفت‌وگو
/// نیست» — نه با قطعیت، بلکه چون نشانه‌هایش (embedding, tts, whisper, …) می‌گوید
/// به `chat/completions` نمی‌خورد.
fn to_openai_model(item: &Value) -> Option<AiModel> {
    let id = item
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if id.is_empty() {
        return None;
    }
    let lower = id.to_lowercase();
    const NOT_CHAT: [&str; 12] = [
        "embedding", "moderation", "whisper", "transcri", "tts", "speech", "image", "dall",
        "sora", "audio", "rerank", "ocr",
    ];
    if NOT_CHAT.iter().any(|n| lower.contains(n)) {
        return None;
    }
    // رایگان: قرارداد `:free` یا `pricing` صفرِ OpenRouter. بدون نشانه، false —
    // «رایگان» ادعایی است که فقط سرویس می‌تواند بکند.
    let free = lower.ends_with(":free")
        || item
            .get("pricing")
            .and_then(|p| {
                let zero = |k: &str| {
                    p.get(k)
                        .and_then(|v| {
                            v.as_str()
                                .and_then(|s| s.parse::<f64>().ok())
                                .or_else(|| v.as_f64())
                        })
                        == Some(0.0)
                };
                Some(zero("prompt") && zero("completion"))
            })
            .unwrap_or(false);
    Some(AiModel {
        display_name: item
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&id)
            .to_string(),
        description: item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        input_token_limit: item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        output_token_limit: 0,
        chat_capable: true,
        free,
        id,
    })
}

/// `GET {base}/models` آنتروپیک.
///
/// قالب `{"data":[{id, display_name}]}` همانِ OpenAI است، ولی فهرست آنتروپیک
/// فقط مدل‌های گفت‌وگو را می‌دهد — پس نه فیلترِ NOT_CHAT لازم است و نه نشانه‌ای
/// از رایگان بودن وجود دارد (همهٔ مدل‌هایش پولی‌اند؛ «رایگان» ادعایی است که فقط
/// سرویس می‌تواند بکند، پس false). صفحه‌بندی `after_id` عمداً دنبال نمی‌شود: فهرست آنتروپیک یک‌جا
/// جا می‌شود و یک حلقه‌ای که برای سرویسی بی‌نیاز است، فقط سطحِ شکست دارد.
fn list_anthropic(provider: &Provider, api_key: &str, socks_port: u16) -> AiResult<Vec<AiModel>> {
    let body = call(
        provider,
        "GET",
        &format!("{}/models", provider.base_path),
        socks_port,
        api_key,
        None,
    )?;
    let json: Value = serde_json::from_str(&body).map_err(|_| {
        AiError::new("The provider's reply was not valid JSON.", AiErrorKind::Protocol)
    })?;
    let items = json
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| {
            AiError::new(
                "The provider returned no model list in a form this app understands.",
                AiErrorKind::Protocol,
            )
        })?;
    let mut out: Vec<AiModel> = Vec::new();
    for item in items {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").trim();
        if id.is_empty() || out.iter().any(|m| m.id == id) {
            continue;
        }
        out.push(AiModel {
            id: id.to_string(),
            display_name: item
                .get("display_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(id)
                .to_string(),
            description: String::new(),
            input_token_limit: 0,
            output_token_limit: 0,
            chat_capable: true,
            free: false,
        });
    }
    DiagnosticsLog::i("ai", &format!("models: {} offered by the endpoint", out.len()));
    Ok(out)
}

/// یک گفت‌وگو را می‌فرستد و پاسخ مدل را به‌صورت متن ساده برمی‌گرداند.
///
/// * `system` — دستور سیستمی؛ شخصیت برنامه، زبانی که باید در آن پاسخ دهد و
///   قرارداد ماشین‌خوانِ تغییر تنظیمات را حمل می‌کند ([`crate::ai_prompts`]).
/// * `history` — نوبت‌های پیشین، قدیمی‌ترین اول. کامل فرستاده می‌شود چون REST
///   API بی‌حالت است؛ هیچ گفت‌وگوی سمت‌سروری وجود ندارد که به آن اضافه شود.
/// * `json_output` — درخواست `application/json` به‌جای نثر.
///
///   مشاور از این استفاده می‌کند، که پاسخش پارس و به پیکربندی تبدیل می‌شود. با
///   این تنظیم، مدل نمی‌تواند شیء را در یک code fence بپیچد یا با یک جمله
///   معرفی‌اش کند — که بیشترِ چیزی است که استخراج‌کنندهٔ سهل‌گیرِ
///   [`crate::ai_prompts`] برای جان‌سالم‌بردن از آن نوشته شده بود.
#[allow(clippy::too_many_arguments)]
pub fn generate(
    provider: &Provider,
    api_key: &str,
    socks_port: u16,
    model: &str,
    system: &str,
    history: &[AiTurn],
    temperature: f64,
    max_output_tokens: u32,
    json_output: bool,
) -> AiResult<String> {
    if model.trim().is_empty() {
        return Err(AiError::new(
            "No model selected.",
            AiErrorKind::NoSuchModel,
        ));
    }
    match provider.kind {
        ProviderKind::Gemini => generate_gemini(
            provider,
            api_key,
            socks_port,
            model,
            system,
            history,
            temperature,
            max_output_tokens,
            json_output,
        ),
        ProviderKind::OpenAi => generate_openai(
            provider,
            api_key,
            socks_port,
            model,
            system,
            history,
            temperature,
            max_output_tokens,
            json_output,
        ),
        ProviderKind::Anthropic => generate_anthropic(
            provider,
            api_key,
            socks_port,
            model,
            system,
            history,
            temperature,
            max_output_tokens,
            json_output,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_gemini(
    provider: &Provider,
    api_key: &str,
    socks_port: u16,
    model: &str,
    system: &str,
    history: &[AiTurn],
    temperature: f64,
    max_output_tokens: u32,
    json_output: bool,
) -> AiResult<String> {
    if !policy::is_allowed(model) {
        // کمربند و بند شلوار روی فهرست مجاز: یک شناسهٔ مدل می‌تواند از فایل
        // تنظیماتی که بیلد قدیمی‌تری نوشته هم برسد، و آن مسیر از کشف رد نمی‌شود.
        return Err(AiError::new(
            format!("Model \"{model}\" is not one of the models this app supports."),
            AiErrorKind::NoSuchModel,
        ));
    }

    let contents: Vec<Value> = history
        .iter()
        .map(|turn| {
            json!({
                "role": if turn.from_user { "user" } else { "model" },
                "parts": [{ "text": turn.text }],
            })
        })
        .collect();

    let mut generation_config = json!({
        "temperature": temperature,
        "maxOutputTokens": max_output_tokens,
        // ------------------------------------------------------------------
        // ریشهٔ پاسخ‌های بریده / پارس‌نشدنی.
        //
        // maxOutputTokens بودجه‌ای برای **هر چیزی** است که مدل تولید می‌کند، و
        // روی مدلی که توان استدلال دارد، توکن‌های استدلال **اول** از همان بودجه
        // خرج می‌شوند. مشاور ۱۴۰۰ توکن می‌خواست با یک خلاصهٔ لاگ در پرامپت، مدل
        // همه را صرف فکرکردن می‌کرد، و پاسخ دیدنی خالی یا وسط شیء بریده
        // برمی‌گشت — دقیقاً همان جفتِ «۲۰۰ OK» و «پاسخ مشاور به‌عنوان JSON پارس
        // نشد» در لاگ میدانی.
        //
        // thinkingBudget = 0 استدلال را خاموش می‌کند تا تمام بودجه به پاسخ
        // برسد. برای این برنامه معاملهٔ درستی است: هر پرامپت اینجا افق‌کوتاه است
        // (یک تنظیم را توضیح بده، یک لاگ را روی فهرستی ثابت از پیچ‌ها بنگار) و
        // هیچ‌کدام از یک پاس استدلالی که به قیمت پاسخ تمام شود سود نمی‌برند. یک
        // منبع بی‌کران و بی‌حساب از تأخیر را هم از درخواستی که از دو تونل رد
        // می‌شود برمی‌دارد.
        //
        // بی‌قید‌و‌شرط فرستاده می‌شود: مدل‌هایی که thinking را پشتیبانی نمی‌کنند،
        // یک عضو ناشناخته در generationConfig را نادیده می‌گیرند و درخواست را رد
        // نمی‌کنند.
        "thinkingConfig": { "thinkingBudget": 0 },
    });
    if json_output {
        generation_config["responseMimeType"] = json!("application/json");
    }

    let payload = json!({
        "systemInstruction": { "parts": [{ "text": system }] },
        "contents": contents,
        "generationConfig": generation_config,
    });

    let path = format!(
        "{}/models/{}:generateContent",
        provider.base_path,
        policy::normalise(model)
    );
    let body = call(
        provider,
        "POST",
        &path,
        socks_port,
        api_key,
        Some(&payload.to_string()),
    )?;
    let json: Value = serde_json::from_str(&body)
        .map_err(|_| AiError::new("Google's reply was not valid JSON.", AiErrorKind::Protocol))?;

    // پیش از تولید رد شد: هیچ کاندیدی برای خواندن نیست و دلیلش جای کاملاً
    // دیگری زندگی می‌کند.
    if let Some(reason) = json
        .get("promptFeedback")
        .and_then(|f| f.get("blockReason"))
        .and_then(|r| r.as_str())
        .filter(|r| !r.is_empty() && *r != "BLOCK_REASON_UNSPECIFIED")
    {
        return Err(AiError::new(reason, AiErrorKind::Blocked));
    }

    let candidates = json.get("candidates").and_then(|c| c.as_array());
    let Some(candidate) = candidates.and_then(|c| c.first()) else {
        return Err(AiError::new(
            "The model returned no answer.",
            AiErrorKind::Blocked,
        ));
    };

    let mut text = String::new();
    if let Some(parts) = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
    {
        for part in parts {
            // بخش‌های استدلال با `thought: true` علامت‌گذاری می‌شوند و پاسخ
            // **نیستند**. به‌هم‌چسباندنشان پاسخ‌هایی می‌ساخت که با حرف‌زدنِ مدل با
            // خودش شروع می‌شدند، و JSONی که نثر جلویش بود. thinkingBudget=0
            // باید یعنی هیچ‌کدام وجود ندارند؛ این گاردِ مدل‌هایی است که نادیده‌اش
            // می‌گیرند.
            if part
                .get("thought")
                .and_then(|t| t.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            if let Some(chunk) = part.get("text").and_then(|t| t.as_str()) {
                text.push_str(chunk);
            }
        }
    }

    let finish = candidate
        .get("finishReason")
        .and_then(|f| f.as_str())
        .unwrap_or("");
    if text.trim().is_empty() {
        let kind = match finish {
            "SAFETY" | "PROHIBITED_CONTENT" => AiErrorKind::Blocked,
            "MAX_TOKENS" => AiErrorKind::Truncated,
            _ => AiErrorKind::Protocol,
        };
        let message = if finish.is_empty() {
            "The model returned an empty answer."
        } else {
            finish
        };
        return Err(AiError::new(message, kind));
    }
    // پاسخ داد، ولی بریده شد. به‌عنوان خطا گزارش می‌شود و نه موفقیت: نیم‌جمله در
    // یک حبابِ چت و نیم‌شیء در مشاور، هر دو شکست‌اند، و فراخوان می‌تواند با
    // بودجهٔ بزرگ‌تر تلاش کند چون حالا می‌داند این کدام شکست است. متن نیمه هم
    // سوار پیام می‌شود تا اگر خواست همان را نشان دهد.
    if finish == "MAX_TOKENS" {
        DiagnosticsLog::w(
            "ai",
            &format!("answer hit MAX_TOKENS after {} chars", text.len()),
        );
        return Err(AiError::new(text, AiErrorKind::Truncated));
    }
    Ok(text)
}

/// `POST {base}/chat/completions` سازگارِ OpenAI.
///
/// قراردادِ پرامپت ([`crate::ai_prompts`]) عوض نمی‌شود — فقط قالبِ مبادله.
/// `response_format: json_object` را همان `json_output` جمینای تضمین می‌کند؛
/// استخراج‌کنندهٔ سهل‌گیرِ `ai_prompts` هم سرِ جایش است، چون بعضی پنل‌های
/// سازگار این فیلد را بی‌صدا نادیده می‌گیرند.
#[allow(clippy::too_many_arguments)]
fn generate_openai(
    provider: &Provider,
    api_key: &str,
    socks_port: u16,
    model: &str,
    system: &str,
    history: &[AiTurn],
    temperature: f64,
    max_output_tokens: u32,
    json_output: bool,
) -> AiResult<String> {
    let mut messages: Vec<Value> = Vec::with_capacity(history.len() + 1);
    if !system.trim().is_empty() {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for turn in history {
        messages.push(json!({
            "role": if turn.from_user { "user" } else { "assistant" },
            "content": turn.text,
        }));
    }
    let mut payload = json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": max_output_tokens,
    });
    if json_output {
        payload["response_format"] = json!({ "type": "json_object" });
    }
    let body = call(
        provider,
        "POST",
        &format!("{}/chat/completions", provider.base_path),
        socks_port,
        api_key,
        Some(&payload.to_string()),
    )?;
    let json: Value = serde_json::from_str(&body).map_err(|_| {
        AiError::new("The provider's reply was not valid JSON.", AiErrorKind::Protocol)
    })?;
    let choice = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let Some(choice) = choice else {
        return Err(AiError::new(
            "The model returned no answer.",
            AiErrorKind::Blocked,
        ));
    };
    let text = content_text(choice.get("message").and_then(|m| m.get("content")));
    let finish = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .unwrap_or("");
    if text.trim().is_empty() {
        let kind = match finish {
            "content_filter" => AiErrorKind::Blocked,
            "length" => AiErrorKind::Truncated,
            _ => AiErrorKind::Protocol,
        };
        let message = if finish.is_empty() {
            "The model returned an empty answer."
        } else {
            finish
        };
        return Err(AiError::new(message, kind));
    }
    // مثل مسیر جمینای: پاسخِ بریده شکست است، نه موفقیتِ نصفه‌نیمه.
    if finish == "length" {
        DiagnosticsLog::w(
            "ai",
            &format!("answer hit max_tokens after {} chars", text.len()),
        );
        return Err(AiError::new(text, AiErrorKind::Truncated));
    }
    Ok(text)
}

/// `POST {base}/messages` آنتروپیک.
///
/// سه تفاوت با OpenAI و همه در همین تابع حبس شده‌اند:
///  * `system` عضوِ `messages` نیست؛ فیلدِ سطح-بالای خودش را می‌خواهد و یک
///    پیامِ `role:"system"` در تاریخچه، ۴۰۰ می‌گیرد.
///  * `max_tokens` الزامی است — نبودش خطاست، نه پیش‌فرضِ بی‌سقف.
///  * `response_format` وجود ندارد، پس `json_output` فقط با همان
///    استخراج‌کنندهٔ سهل‌گیرِ `ai_prompts` از کنارش رد می‌شویم؛ قراردادی که
///    برای پنل‌های سازگارِ بی‌وفاء نوشته شده بود، دقیقاً همین جا هم به درد
///    می‌خورد.
#[allow(clippy::too_many_arguments)]
fn generate_anthropic(
    provider: &Provider,
    api_key: &str,
    socks_port: u16,
    model: &str,
    system: &str,
    history: &[AiTurn],
    temperature: f64,
    max_output_tokens: u32,
    json_output: bool,
) -> AiResult<String> {
    let _ = json_output;
    let messages: Vec<Value> = history
        .iter()
        .map(|turn| {
            json!({
                "role": if turn.from_user { "user" } else { "assistant" },
                "content": turn.text,
            })
        })
        .collect();
    let mut payload = json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_output_tokens,
        // آنتروپیک دمای ۰..۱ می‌شناسد؛ مدلِ پیش‌فرضِ برنامه داخل این بازه است
        // و هر عددِ بزرگ‌تر، کلیدِ پایین‌تر به همان معناست.
        "temperature": temperature.clamp(0.0, 1.0),
    });
    if !system.trim().is_empty() {
        payload["system"] = json!(system);
    }
    let body = call(
        provider,
        "POST",
        &format!("{}/messages", provider.base_path),
        socks_port,
        api_key,
        Some(&payload.to_string()),
    )?;
    let json: Value = serde_json::from_str(&body).map_err(|_| {
        AiError::new("The provider's reply was not valid JSON.", AiErrorKind::Protocol)
    })?;
    let text = content_text(json.get("content"));
    let stop = json
        .get("stop_reason")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if text.trim().is_empty() {
        let kind = match stop {
            "refusal" => AiErrorKind::Blocked,
            "max_tokens" => AiErrorKind::Truncated,
            _ => AiErrorKind::Protocol,
        };
        let message = if stop.is_empty() {
            "The model returned no answer."
        } else {
            stop
        };
        return Err(AiError::new(message, kind));
    }
    if stop == "max_tokens" {
        DiagnosticsLog::w(
            "ai",
            &format!("answer hit max_tokens after {} chars", text.len()),
        );
        return Err(AiError::new(text, AiErrorKind::Truncated));
    }
    Ok(text)
}

/// `message.content` در دو قالب دیده شده: رشته، و آرایهٔ بخش‌ها با `text`.
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<String>(),
        _ => String::new(),
    }
}

// ---- لوله‌کشی --------------------------------------------------------------

/// یک تبادل HTTP، با هر شکستی از قبل ترجمه‌شده.
///
/// به آنچه لاگ **نمی‌شود** توجه کنید: کلید، رشتهٔ پرسمانِ مسیر و بدنهٔ درخواست.
/// فقط متد، بخشِ مدل‌دارِ مسیر و کد وضعیت به لاگ تشخیصی می‌روند، چون آن لاگ
/// صادرشدنی است و کاربران آن را در گزارش‌های عمومی می‌چسبانند.
fn call(
    provider: &Provider,
    method: &str,
    path: &str,
    socks_port: u16,
    api_key: &str,
    json_body: Option<&str>,
) -> AiResult<String> {
    if api_key.trim().is_empty() {
        return Err(AiError::new("No API key stored.", AiErrorKind::BadKey));
    }
    let mut attempt = 0u32;
    let mut last = AiError::new("No attempt was made.", AiErrorKind::Transport);
    while attempt < MAX_ATTEMPTS {
        attempt += 1;
        match attempt_call(provider, method, path, socks_port, api_key, json_body) {
            Ok(body) => return Ok(body),
            Err(error) => {
                let retryable = error.kind.worth_retrying();
                let wait = backoff_millis(attempt, error.retry_after_seconds);
                last = error;
                if attempt >= MAX_ATTEMPTS || !retryable {
                    break;
                }
                DiagnosticsLog::i(
                    "ai",
                    &format!(
                        "{:?} on attempt {attempt}/{MAX_ATTEMPTS}; retrying in {wait}ms",
                        last.kind
                    ),
                );
                // خوابِ بلوکه‌کننده اینجا درست است: هر فراخوانِ این تابع روی
                // رشتهٔ کارگرِ ai_session است و خواندن‌های سوکت در دو طرفش خیلی
                // بیشتر بلوکه می‌شوند. خوابیدن، کل سیاست retry را در یک جای
                // خواندنی نگه می‌دارد.
                std::thread::sleep(Duration::from_millis(wait));
            }
        }
    }
    Err(last)
}

/// یک تبادل HTTP، با هر شکستی از قبل طبقه‌بندی‌شده.
fn attempt_call(
    provider: &Provider,
    method: &str,
    path: &str,
    socks_port: u16,
    api_key: &str,
    json_body: Option<&str>,
) -> AiResult<String> {
    let (header, value) = auth(provider, api_key);
    let response = match ai_http::request(
        method,
        &provider.host,
        path,
        socks_port,
        header,
        &value,
        extra_headers(provider),
        json_body,
        REQUEST_TIMEOUT,
    ) {
        Ok(r) => r,
        Err(failure) => {
            DiagnosticsLog::w(
                "ai",
                &format!("request failed via 127.0.0.1:{socks_port}: {failure}"),
            );
            return Err(AiError::new(failure.to_string(), AiErrorKind::Transport));
        }
    };
    let logged_path = path.split('?').next().unwrap_or(path);
    DiagnosticsLog::i(
        "ai",
        &format!("{method} {logged_path} -> {}", response.code),
    );
    if response.ok() {
        return Ok(response.body);
    }
    if response.code == 0 {
        return Err(AiError::new(
            "No response through the tunnel.",
            AiErrorKind::Transport,
        ));
    }

    let parsed: Option<Value> = serde_json::from_str(&response.body).ok();
    let error_obj = parsed.as_ref().and_then(|v| v.get("error"));
    let api_message = error_obj
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .filter(|m| !m.is_empty())
        .map(|m| m.to_string());

    let mentions_key = api_message
        .as_deref()
        .map(|m| m.to_lowercase().contains("api key"))
        .unwrap_or(false);
    let kind = if response.code == 400 && mentions_key {
        AiErrorKind::BadKey
    } else if response.code == 401 || response.code == 403 {
        AiErrorKind::BadKey
    } else if response.code == 429 {
        AiErrorKind::RateLimit
    } else if response.code == 404 {
        AiErrorKind::NoSuchModel
    } else if response.code >= 500 {
        // ۵xx یعنی گوگل شکست خورده، **نه** تونل. رجوع به AiErrorKind::ServerError.
        AiErrorKind::ServerError
    } else {
        AiErrorKind::Protocol
    };

    Err(AiError {
        message: api_message.unwrap_or_else(|| format!("HTTP {}", response.code)),
        kind,
        retry_after_seconds: response
            .retry_after_seconds
            .or_else(|| retry_delay_from_error(error_obj)),
    })
}

/// `RetryInfo.retryDelay` خودِ گوگل را از بدنهٔ خطا می‌خواند.
///
/// بدنهٔ یک ۴۲۹ حاوی `details[].retryDelay: "2.379075806s"` است، یعنی سرور
/// دقیقاً می‌گوید چقدر صبر کنیم. لاگ میدانی پنج تا ۴۲۹ در ده ثانیه نشان می‌داد
/// چون هیچ‌کس آن را نخوانده بود. احترام‌گذاشتن به آن هم از یک backoff ثابت
/// سریع‌تر است و هم تفاوتِ «یک بار صبرکردن» و «سخت‌تر محدود‌شدن به‌خاطر کوبیدن».
fn retry_delay_from_error(error: Option<&Value>) -> Option<f64> {
    let details = error?.get("details")?.as_array()?;
    for item in details {
        let raw = item
            .get("retryDelay")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .trim();
        if raw.is_empty() {
            continue;
        }
        if let Ok(seconds) = raw.trim_end_matches('s').parse::<f64>() {
            if seconds >= 0.0 {
                return Some(seconds);
            }
        }
    }
    None
}

/// چقدر پیش از تلاش `attempt + 1` صبر شود.
///
/// عددِ خودِ سرور وقتی فرستاده باشد برنده است؛ وگرنه نمایی با یک سقف. سقف از
/// شکل منحنی مهم‌تر است: این کد وقتی اجرا می‌شود که کاربر به یک اسپینر نگاه
/// می‌کند، پس سیاستی که اجازه دارد یک دقیقه صبر کند، سیاستی است که شبیه هنگ است.
fn backoff_millis(attempt: u32, retry_after_seconds: Option<f64>) -> u64 {
    let suggested = retry_after_seconds
        .map(|s| (s * 1000.0) as u64)
        .unwrap_or(0);
    let exponential = BASE_BACKOFF_MS << (attempt - 1);
    suggested
        .max(exponential)
        .clamp(BASE_BACKOFF_MS, MAX_BACKOFF_MS)
}

fn to_model(item: &Value) -> AiModel {
    let raw = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let id = policy::normalise(raw);
    let supports_generate = item
        .get("supportedGenerationMethods")
        .and_then(|m| m.as_array())
        .map(|arr| arr.iter().any(|m| m.as_str() == Some("generateContent")))
        .unwrap_or(false);
    let display = item
        .get("displayName")
        .and_then(|d| d.as_str())
        .filter(|d| !d.is_empty())
        .unwrap_or(&id)
        .to_string();
    AiModel {
        id,
        display_name: display,
        description: item
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string(),
        input_token_limit: item
            .get("inputTokenLimit")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        output_token_limit: item
            .get("outputTokenLimit")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        chat_capable: supports_generate,
        // هر پنج مدلِ فهرست مجاز ردهٔ Flash‌اند و روی تیر رایگان جواب می‌دهند —
        // همان استدلالِ بالای `ai_model_policy::ALLOWED`.
        free: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_servers_own_delay_beats_our_curve_but_not_the_ceiling() {
        // بدون پیشنهاد سرور: نمایی.
        assert_eq!(backoff_millis(1, None), 700);
        assert_eq!(backoff_millis(2, None), 1_400);
        // پیشنهاد سرور بزرگ‌تر است، پس برنده می‌شود.
        assert_eq!(backoff_millis(1, Some(2.379_075_806)), 2_379);
        // ولی سقف، انتظارِ شبیه‌هنگ را می‌بُرد.
        assert_eq!(backoff_millis(1, Some(120.0)), MAX_BACKOFF_MS);
    }

    #[test]
    fn reads_google_retry_info_out_of_an_error_body() {
        let body: Value = serde_json::from_str(
            r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"2.5s"}]}}"#,
        )
        .unwrap();
        assert_eq!(retry_delay_from_error(body.get("error")), Some(2.5));
    }

    #[test]
    fn only_transient_failures_are_retried() {
        assert!(AiErrorKind::ServerError.worth_retrying());
        assert!(AiErrorKind::RateLimit.worth_retrying());
        assert!(AiErrorKind::Transport.worth_retrying());
        // این‌ها جبری‌اند: تکرارشان فقط سهمیه می‌سوزاند.
        assert!(!AiErrorKind::BadKey.worth_retrying());
        assert!(!AiErrorKind::NoSuchModel.worth_retrying());
        assert!(!AiErrorKind::Blocked.worth_retrying());
        assert!(!AiErrorKind::Truncated.worth_retrying());
    }

    #[test]
    fn each_service_gets_its_own_auth_shape() {
        let mut presets = crate::ai_provider::presets();
        let gemini = presets.remove(0);
        let openai = presets.remove(0);
        let claude = presets.remove(0);
        assert_eq!(auth(&gemini, "K").0, "x-goog-api-key");
        assert_eq!(auth(&openai, "K"), ("Authorization", "Bearer K".to_string()));
        assert_eq!(auth(&claude, "K").0, "x-api-key");
        // بدون این هدر، آنتروپیک هر درخواستی را ۴۰۰ می‌دهد.
        assert!(extra_headers(&gemini).is_empty());
        assert!(extra_headers(&openai).is_empty());
        assert_eq!(
            extra_headers(&claude),
            &[("anthropic-version", "2023-06-01")][..]
        );
    }

    #[test]
    fn a_model_row_needs_generate_content_to_be_chat_capable() {        let item: Value = serde_json::from_str(
            r#"{"name":"models/gemini-3.8-flash","displayName":"Flash","supportedGenerationMethods":["generateContent","countTokens"]}"#,
        )
        .unwrap();
        let model = to_model(&item);
        assert_eq!(model.id, "gemini-3.8-flash");
        assert!(model.chat_capable);

        let embed: Value = serde_json::from_str(
            r#"{"name":"models/text-embedding-004","supportedGenerationMethods":["embedContent"]}"#,
        )
        .unwrap();
        assert!(!to_model(&embed).chat_capable);
    }
}

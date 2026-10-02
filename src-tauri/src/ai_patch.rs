//! پورت ۱:۱ از `ai/AiPatch.kt` — مرزِ اعتمادِ بین چیزی که مدل *پیشنهاد* می‌کند
//! و چیزی که برنامه واقعاً *می‌نویسد*.
//!
//! # چرا این فایل هست
//!
//! مشاور تنظیمات از یک مدل زبانی می‌خواهد یک شیء JSON از تغییرات پیکربندی
//! برگرداند. آن شیء از یک API شبکه، از پاسخی که یک شخص ثالث تولید کرده، به
//! برنامه می‌رسد — و در برنامه‌ای که مسیر شبکهٔ کل سیستم را می‌سازد، دربارهٔ
//! سوییچ قطع، گارد نشتی، فهرست عبور مستقیم و اعتبارنامه‌های Zero Trust حرف
//! می‌زند. اعمال‌کردنِ آن شیء با یک نگاشتِ عمومیِ «هر کلیدی که آمد» یعنی
//! «هر متنی که مدل تولید کند، یا هر متنی که کسی موفق شود مدل را به تولیدش وادار
//! کند، پیکربندی امنیتی این دستگاه است». پرامپت هم دفاع نیست: پرامپت یک
//! درخواست است، نه یک تضمین.
//!
//! پس در این جهت سه قاعدهٔ سخت برقرار است و هیچ‌کدام قابل چشم‌پوشی نیستند:
//!
//!  1. **فقط کلیدهای [`WRITABLE`].** یک کلیدِ ناشناخته سکوت‌وار نادیده گرفته
//!     نمی‌شود — رد و لاگ می‌شود، چون کلیدِ ناشناخته یا اشتباهِ مدل است یا
//!     تلاشی برای رسیدن به جایی که نباید.
//!  2. **هیچ راز، هرگز.** `accessSecret` و `accessToken` عمداً *در فهرست
//!     نیستند*. مدل هرگز اعتبارنامه‌ای نمی‌بیند و هرگز نمی‌تواند یکی بنویسد.
//!  3. **نوع و بازه اعتبارسنجی می‌شود، بعد `normalize` خودِ پروفایل.** مدل
//!     می‌تواند `"1500"` رشته‌ای، `1500.0` اعشاری، یا `9999` بفرستد؛ هیچ‌کدام
//!     نباید به پروفایل برسد.
//!
//! # چرا فهرست دسکتاپ با فهرست اندروید یکی نیست
//!
//! این نقطه‌ای است که پورتِ کلمه‌به‌کلمه **غلط** می‌بود. دو برنامه، دو مجموعهٔ
//! نامِ فیلد دارند: اندروید `dnsServers` و `reconnectRetryLimit` دارد، دسکتاپ
//! `dns` و `reconnectAttempts`. پروفایل دسکتاپ با `rename_all = "camelCase"`
//! سریالایز می‌شود، پس نام‌های اینجا از خودِ `ConnectionProfile` مشتق شده‌اند و
//! از مستند موبایل کپی نشده‌اند. یک فهرست مجازِ کپی‌شده، بدترین شکل شکست را
//! می‌داشت: هر پیشنهاد مدل بی‌صدا رد می‌شد، «اعمال شد» به کاربر گفته می‌شد، و
//! هیچ چیزی عوض نمی‌شد.
//!
//! دو فیلدِ اندرویدی هم عمداً غایب‌اند: کلیدِ حالتِ پراکسیِ اندروید (در دسکتاپ
//! وجود ندارد، چون Wintun کل سیستم را می‌گیرد) و `splitApps` که در ویندوز نام
//! فرآیند است و مدل هیچ راهی برای دانستنِ اینکه چه چیزی روی این ماشین نصب است
//! ندارد.
//!
//! نامِ آن کلید عمداً نوشته نمی‌شود: گامِ Preflight در CI هر تکرارِ آن رشته را در
//! `src`، `src-tauri/src` و `installer` شکست می‌دهد، تا یک مسیرِ کدِ نیمه‌پورت‌شده
//! نتواند بی‌صدا برگردد. یک کامنت هم برای آن گرید کافی است — و درست هم می‌گوید:
//! چیزی که نباید وجود داشته باشد، نامش هم لازم نیست.

use crate::log::DiagnosticsLog;
use crate::profile::{ConnectionProfile, DEFAULT_MTU};
use serde_json::Value;
use std::collections::BTreeMap;

/// نوعِ مجازِ یک کلید، و بازه‌اش.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    /// عدد صحیح با کف و سقفِ **بسته**.
    Int(u32, u32),
    /// یکی از مقادیرِ نامی (`SCREAMING_SNAKE_CASE`/`UPPERCASE` روی سیم).
    Enum(&'static [&'static str]),
    /// متن آزاد، با سقف طول.
    Text(usize),
    /// فهرستی از متن‌ها، با سقف تعداد و سقف طولِ هر عضو.
    TextList(usize, usize),
}

// ---------------------------------------------------------------------------
//  مقادیر نامی — از `profile.rs` خوانده شده‌اند، نه از مستند موبایل
// ---------------------------------------------------------------------------
//
// این هشت فهرست، اولین نسخه‌شان **کلمه‌به‌کلمه از اندروید** کپی شده بود و شش
// تای‌شان غلط بود: دسکتاپ `Noize` را `FIREWALL|BALANCED|GFW|AGGRESSIVE`
// می‌نامد و نه `MEDIUM|HEAVY`، پروتکلش `SMART|GOOL` دارد، حالت اسکنش
// `TURBO…IRONCLAD` است و `AccessMode` هم `OFF|EMAIL|SERVICE_TOKEN|TOKEN`.
// نتیجهٔ آن کپی، بدترین شکل شکست بود: مدل یک مقدارِ کاملاً معقول پیشنهاد می‌کرد،
// نگهبان می‌پذیرفتش، و بعد **کلِ** پچ روی دروازهٔ آخر می‌افتاد.
//
// `enum_lists_match_the_profile` در پایین همین فایل، هر یک از این نام‌ها را با
// یک رفت‌و‌برگشتِ واقعیِ serde می‌سنجد، تا این فهرست‌ها دیگر نتوانند بی‌صدا از
// `profile.rs` واگرا شوند.

const BACKENDS: &[&str] = &["AETHER", "AETHER_PSIPHON"];
const PROTOCOLS: &[&str] = &["SMART", "MASQUE", "WIREGUARD", "GOOL", "MIM"];
const SCAN_MODES: &[&str] = &["TURBO", "BALANCED", "THOROUGH", "STEALTH", "IRONCLAD"];
const IP_VERSIONS: &[&str] = &["V4", "V6", "BOTH"];
const NOIZE_MODES: &[&str] = &["OFF", "LIGHT", "FIREWALL", "BALANCED", "GFW", "AGGRESSIVE"];
const ENDPOINT_MODES: &[&str] = &["AUTO", "MANUAL_PEER", "MANUAL_RANGE"];
const SPLIT_MODES: &[&str] = &["OFF", "INCLUDE", "EXCLUDE"];
const ACCESS_MODES: &[&str] = &["OFF", "EMAIL", "SERVICE_TOKEN", "TOKEN"];

/// هر کلیدی که مشاور اجازه دارد بنویسد — و هیچ کلید دیگری.
///
/// نام‌ها همان چیزی هستند که `ConnectionProfile` روی سیم سریالایز می‌کند
/// (camelCase). هر افزودنی به این فهرست یک تصمیم امنیتی است.
fn writable() -> BTreeMap<&'static str, Kind> {
    let mut m = BTreeMap::new();
    // --- پشتهٔ شبکه
    m.insert("backend", Kind::Enum(BACKENDS));
    m.insert("exitRegion", Kind::Text(2));
    m.insert("protocol", Kind::Enum(PROTOCOLS));
    m.insert("scanMode", Kind::Enum(SCAN_MODES));
    m.insert("ipVersion", Kind::Enum(IP_VERSIONS));
    m.insert("masqueHttp2", Kind::Bool);
    // --- ترابرد و ضد‌DPI: قلبِ کاری که مشاور برایش وجود دارد
    m.insert("noize", Kind::Enum(NOIZE_MODES));
    m.insert("endpointMode", Kind::Enum(ENDPOINT_MODES));
    m.insert("manualPeer", Kind::Text(120));
    m.insert("manualRange", Kind::Text(200));
    m.insert("keepalive", Kind::Int(0, 120));
    m.insert("fragment", Kind::Bool);
    m.insert("ech", Kind::Bool);
    // سقف ۹۰۰۰ چون پیش‌تنظیم ۸۵۰۰ (jumbo) قانونی است؛ کف ۱۲۸۰ کفِ IPv6 است.
    m.insert("mtu", Kind::Int(1_280, 9_000));
    // --- تاب‌آوری
    m.insert("quickReconnect", Kind::Bool);
    m.insert("reconnectAttempts", Kind::Int(3, 20));
    m.insert("killSwitch", Kind::Bool);
    m.insert("ipv6Protection", Kind::Bool);
    m.insert("lanShare", Kind::Bool);
    // --- DNS و مسیریابی
    m.insert("dns", Kind::TextList(8, 64));
    m.insert("routeBlock", Kind::TextList(200, 200));
    m.insert("routeDirect", Kind::TextList(200, 200));
    m.insert("routeSniff", Kind::Bool);
    m.insert("gateway", Kind::Bool);
    // --- پروکسی بالادست و Zero Trust (بدون اعتبارنامه)
    m.insert("upstream", Kind::Text(200));
    m.insert("team", Kind::Text(64));
    m.insert("accessMode", Kind::Enum(ACCESS_MODES));
    m.insert("accessEmail", Kind::Text(160));
    m.insert("accessId", Kind::Text(120));
    m.insert("splitMode", Kind::Enum(SPLIT_MODES));
    m
}

/// کلیدهایی که **چت** اجازهٔ نوشتنشان را دارد — زیرمجموعهٔ باریکِ [`writable`].
///
/// # چرا چت و مشاور یک فهرست ندارند
///
/// مشاور یک مسیرِ بسته است: کاربر «تحلیل و تنظیم» را می‌زند، برنامه لاگِ خودش را
/// می‌فرستد، و پاسخ فقط پچ است. چت باز است: هر متنی که کاربر — یا هر چیزی که
/// کاربر از جایی کپی کرده — تایپ کند به مدل می‌رسد، و مدل در همان نوبت می‌تواند
/// تغییرِ تنظیمات پیشنهاد کند. همان تفکیکی که `ai/AiPatch.kt` در موبایل دارد، و
/// دلیلش این است که چند کلید در فهرستِ مشاور، اگر از دلِ یک گفت‌وگو نوشته شوند،
/// دقیقاً همان کاری را می‌کنند که این برنامه برای جلوگیری از آن وجود دارد:
///
///  - `upstream` — «همهٔ ترافیک این کاربر از میزبانی که من می‌گویم». خطرناک‌ترین
///    رشتهٔ کلِ پروفایل.
///  - `routeDirect` / `routeBlock` — یک قاعدهٔ `direct` یک دامنه را از تونل
///    **بیرون** می‌برد. «bank.example.com را direct کن» شکلِ یک نکتهٔ سرعتی را
///    دارد و یک لو‌رفتنِ هویت است.
///  - `manualPeer` / `manualRange` — تونل را به لبه‌ای که مهاجم انتخاب کرده
///    سنجاق می‌کند.
///  - `backend` — رفتن به Aether خالی خودِ هوش مصنوعی را خاموش می‌کند
///    ([`crate::ai_gate`])، یعنی یک پیشنهادِ بد، قابلیت را از درونِ همان
///    گفت‌وگویی که خرابش کرده غیرقابلِ تعمیر می‌کند.
///  - `splitMode` — تصمیم می‌گیرد کدام برنامه‌ها اصلاً محافظت می‌شوند؛ انتخابِ
///    عامدانهٔ خودِ کاربر است، نه یک پیشنهاد.
///  - `lanShare` / `gateway` — یک پراکسی را روی شبکهٔ محلی باز می‌کند.
///  - `team` / `accessMode` / `accessEmail` / `accessId` — اعتبارنامهٔ سازمانی.
///  - `killSwitch` — تنها موردِ فهرست که *خاموش‌کردنش* خطر است، نه روشن‌کردنش؛
///    و مدل راهی برای تشخیص این دو در یک جملهٔ کاربر ندارد.
///
/// افزودن یک کلید به این فهرست یعنی پاسخ‌دادن به این پرسش: «اگر مدل اشتباه کند،
/// یا کسی موفق شود مدل را به این وادار کند، بدترین چیزی که می‌شود چیست؟»
fn chat_writable() -> BTreeMap<&'static str, Kind> {
    const CHAT_KEYS: [&str; 14] = [
        "protocol",
        "scanMode",
        "ipVersion",
        "noize",
        "mtu",
        "keepalive",
        "fragment",
        "ech",
        "masqueHttp2",
        "quickReconnect",
        "reconnectAttempts",
        "ipv6Protection",
        "routeSniff",
        "dns",
    ];
    let all = writable();
    let mut m = BTreeMap::new();
    for key in CHAT_KEYS {
        // `expect` عمدی: یک کلید در این فهرست که در `writable` نباشد یک خطای
        // برنامه‌نویسی است و نه یک حالتِ زمانِ اجرا. `chat_keys_are_a_subset`
        // پایینِ همین فایل هم پیش از هر بیلد می‌گیردش.
        let kind = *all
            .get(key)
            .expect("a chat key must also be advisor-writable");
        m.insert(key, kind);
    }
    m
}

/// کلیدهایی که وجود دارند و **هرگز** نوشتنی نیستند.
///
/// جدا از «ناشناخته» نگه داشته می‌شوند تا لاگ بتواند تفاوت را بگوید: یک مدل که
/// `leakGuard: false` پیشنهاد می‌کند اشتباه نمی‌کند، دارد سعی می‌کند یک گارد
/// امنیتی را خاموش کند، و آن رخداد ارزش دیده‌شدن دارد.
const FORBIDDEN: [&str; 5] = [
    "leakGuard",
    "accessSecret",
    "accessToken",
    "settingsRev",
    "reprovision",
];

/// نتیجهٔ اعمالِ یک پچ.
#[derive(Debug, Default)]
pub struct PatchOutcome {
    /// کلیدهایی که واقعاً عوض شدند، با مقدارِ تازه، برای نمایش به کاربر.
    pub applied: Vec<(String, String)>,
    /// کلیدهایی که رد شدند، با دلیل.
    pub rejected: Vec<(String, String)>,
}

impl PatchOutcome {
    pub fn changed(&self) -> bool {
        !self.applied.is_empty()
    }
}

/// پچِ پیشنهادیِ مدل را روی یک کپی از پروفایل اعمال می‌کند.
///
/// **هرگز** خطا نمی‌دهد و **هرگز** پچ را یک‌کاسه دور نمی‌اندازد: هر کلید مستقل
/// داوری می‌شود. یک کلیدِ بدِ درونِ یک پیشنهادِ عمدتاً درست، نباید سه تغییرِ خوب
/// را هم باطل کند — و کاربر باید *ببیند* کدام کدام بود، چون این تنها راهی است که
/// کسی می‌تواند به این قابلیت اعتماد کند.
pub fn apply(profile: &mut ConnectionProfile, patch: &Value) -> PatchOutcome {
    apply_with(profile, patch, &writable())
}

/// همان [`apply`]، ولی با فهرستِ باریکِ چت.
///
/// مسیرِ چت از این رد می‌شود و نه از [`apply`]: رجوع به [`chat_writable`] برای
/// اینکه چرا دو فهرست وجود دارد.
pub fn apply_chat(profile: &mut ConnectionProfile, patch: &Value) -> PatchOutcome {
    apply_with(profile, patch, &chat_writable())
}

fn apply_with(
    profile: &mut ConnectionProfile,
    patch: &Value,
    allowed: &BTreeMap<&'static str, Kind>,
) -> PatchOutcome {
    let mut outcome = PatchOutcome::default();
    let Some(object) = patch.as_object() else {
        outcome
            .rejected
            .push(("<patch>".into(), "not a JSON object".into()));
        return outcome;
    };

    // روی یک نمایشِ JSONِ خودِ پروفایل کار می‌شود و نه با یک `match` غولِ
    // فیلدبه‌فیلد. دلیل مهندسی: آن `match` نسخهٔ دومی از ساختار پروفایل
    // می‌شد که باید دستی هم‌گام می‌ماند، و روزی که کسی یک فیلد را تغییر نام
    // می‌داد، بی‌صدا از هم می‌پاشید. اینجا `serde` تنها مرجعِ نام‌هاست.
    let Ok(mut current) = serde_json::to_value(&*profile) else {
        outcome
            .rejected
            .push(("<profile>".into(), "profile could not be read".into()));
        return outcome;
    };

    for (key, proposed) in object {
        if FORBIDDEN.contains(&key.as_str()) {
            DiagnosticsLog::w(
                "ai",
                &format!("advisor tried to write the protected key \"{key}\" — refused"),
            );
            outcome
                .rejected
                .push((key.clone(), "protected setting".into()));
            continue;
        }
        let Some(kind) = allowed.get(key.as_str()) else {
            DiagnosticsLog::w(
                "ai",
                &format!("advisor proposed the unknown key \"{key}\" — ignored"),
            );
            outcome
                .rejected
                .push((key.clone(), "unknown setting".into()));
            continue;
        };
        match coerce(*kind, proposed) {
            Ok(value) => {
                let before = current.get(key.as_str()).cloned().unwrap_or(Value::Null);
                if before == value {
                    // نه اعمال‌شده و نه رد‌شده: خبری نیست. گزارش‌کردنش به‌عنوان
                    // «تغییر» یعنی کاربر با فهرستی از کارهای انجام‌نشده روبه‌رو
                    // شود و باور کند چیزی عوض شده.
                    continue;
                }
                outcome.applied.push((key.clone(), render(&value)));
                current[key.as_str()] = value;
            }
            Err(reason) => {
                DiagnosticsLog::w(
                    "ai",
                    &format!("advisor sent an invalid value for \"{key}\": {reason}"),
                );
                outcome.rejected.push((key.clone(), reason));
            }
        }
    }

    if !outcome.applied.is_empty() {
        // آخرین دروازه: `serde` با `default` روی پروفایل، هر فیلدِ ازدست‌رفته را
        // به پیش‌فرض برمی‌گرداند؛ و بعدش `normalize` اعتبارسنجیِ *خودِ برنامه* را
        // دوباره اعمال می‌کند — از جمله `leak_guard = true` و نرمال‌سازی کد کشور.
        // هیچ مسیری وجود ندارد که پچ بتواند از آن رد شود.
        match serde_json::from_value::<ConnectionProfile>(current.clone()) {
            Ok(mut patched) => {
                patched.normalize();
                *profile = patched;
            }
            // اگر دروازهٔ آخر شکست، **همه** را دور نمی‌ریزیم.
            //
            // نسخهٔ اول همین کار را می‌کرد و قولِ اعلام‌شدهٔ این فایل را می‌شکست:
            // یک مقدارِ نامیِ نامعتبر، سه تغییرِ بی‌عیبِ دیگر را هم باطل می‌کرد و
            // پیامی می‌داد که به هیچ‌کدام از کلیدها اشاره نمی‌کرد. حالا کلید‌به‌کلید
            // بازسازی می‌شود: هر تغییری که به‌تنهایی پروفایل را معتبر نگه دارد
            // می‌ماند، و مقصرِ واقعی با نام رد می‌شود.
            Err(_) => {
                let mut rebuilt = match serde_json::to_value(&*profile) {
                    Ok(v) => v,
                    Err(_) => return outcome,
                };
                let survivors = std::mem::take(&mut outcome.applied);
                for (key, shown) in survivors {
                    let candidate = current.get(&key).cloned().unwrap_or(Value::Null);
                    let mut trial = rebuilt.clone();
                    trial[&key] = candidate;
                    match serde_json::from_value::<ConnectionProfile>(trial.clone()) {
                        Ok(_) => {
                            rebuilt = trial;
                            outcome.applied.push((key, shown));
                        }
                        Err(e) => outcome
                            .rejected
                            .push((key, format!("the app rejected this value: {e}"))),
                    }
                }
                if let Ok(mut patched) = serde_json::from_value::<ConnectionProfile>(rebuilt) {
                    patched.normalize();
                    *profile = patched;
                }
            }
        }
    }
    outcome
}

/// یک مقدارِ پیشنهادی را به شکلِ درستش تبدیل می‌کند، یا دلیل رد را می‌گوید.
///
/// سهل‌گیریِ سنجیده: مدل‌ها `"true"`، `"1500"` و `1500.0` می‌فرستند، و رد‌کردنِ
/// آن‌ها یعنی این قابلیت تصادفاً و بسته به حالِ مدل کار کند. سهل‌گیری در **نوع**
/// است، هرگز در **بازه** و هرگز در **مجموعهٔ کلیدها**.
fn coerce(kind: Kind, proposed: &Value) -> Result<Value, String> {
    match kind {
        Kind::Bool => match proposed {
            Value::Bool(b) => Ok(Value::Bool(*b)),
            Value::String(s) => match s.trim().to_lowercase().as_str() {
                "true" | "on" | "yes" | "1" => Ok(Value::Bool(true)),
                "false" | "off" | "no" | "0" => Ok(Value::Bool(false)),
                _ => Err(format!("\"{s}\" is not a yes/no value")),
            },
            _ => Err("expected true or false".into()),
        },
        Kind::Int(min, max) => {
            let number = match proposed {
                Value::Number(n) => n.as_f64().ok_or("not a number")?,
                Value::String(s) => s
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("\"{s}\" is not a number"))?,
                _ => return Err("expected a number".into()),
            };
            if !number.is_finite() || number < 0.0 {
                return Err("not a usable number".into());
            }
            let rounded = number.round() as u64;
            if rounded < min as u64 || rounded > max as u64 {
                return Err(format!(
                    "{rounded} is outside the allowed range {min}–{max}"
                ));
            }
            Ok(Value::from(rounded as u32))
        }
        Kind::Enum(options) => {
            let raw = proposed
                .as_str()
                .ok_or("expected a name")?
                .trim()
                .to_uppercase()
                .replace([' ', '-'], "_");
            options
                .iter()
                .find(|o| **o == raw)
                .map(|o| Value::String((*o).to_string()))
                .ok_or_else(|| format!("\"{raw}\" is not one of {}", options.join(", ")))
        }
        Kind::Text(max) => {
            let raw = proposed.as_str().ok_or("expected text")?.trim();
            if raw.chars().count() > max {
                return Err(format!(
                    "longer than the {max} characters this field accepts"
                ));
            }
            if raw.contains(['\n', '\r', '\0']) {
                // یک مقدار چندخطی در یک فیلد تک‌خطی، در بهترین حالت یک رابط
                // کاربری خراب است و در بدترین حالت یک تزریق به هر چیزی که بعداً
                // این متن را پارس می‌کند.
                return Err("contains a line break".into());
            }
            Ok(Value::String(raw.to_string()))
        }
        Kind::TextList(max_items, max_len) => {
            let items: Vec<&Value> = match proposed {
                Value::Array(a) => a.iter().collect(),
                // یک فهرستِ کاماشده هم پذیرفته می‌شود: شکلی است که مدل‌ها نصفِ
                // وقت‌ها برمی‌گردانند و ردکردنش هیچ چیزی را امن‌تر نمی‌کند.
                Value::String(_) => Vec::new(),
                _ => return Err("expected a list".into()),
            };
            let mut out: Vec<Value> = Vec::new();
            let raw_items: Vec<String> = if items.is_empty() {
                proposed
                    .as_str()
                    .unwrap_or("")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };
            if raw_items.len() > max_items {
                return Err(format!(
                    "more than the {max_items} entries this list accepts"
                ));
            }
            for item in raw_items {
                if item.chars().count() > max_len || item.contains(['\n', '\r', '\0', ' ']) {
                    return Err(format!("\"{item}\" is not a valid entry"));
                }
                let value = Value::String(item);
                if !out.contains(&value) {
                    out.push(value);
                }
            }
            Ok(Value::Array(out))
        }
    }
}

/// یک مقدار را برای نمایش به کاربر به متن تبدیل می‌کند.
fn render(value: &Value) -> String {
    match value {
        Value::String(s) if s.is_empty() => "(auto)".to_string(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "on" } else { "off" }.to_string(),
        Value::Array(items) if items.is_empty() => "(empty)".to_string(),
        Value::Array(items) => items
            .iter()
            .map(|i| i.as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

/// همان MTU پیش‌فرض، برای پرامپت — تا مدل بداند خط پایه کجاست.
pub const BASELINE_MTU: u32 = DEFAULT_MTU;

/// فهرست کلیدهای نوشتنی برای درج در پرامپت.
///
/// از **همین** فهرست تولید می‌شود و در پرامپت دستی تکرار نشده: دو فهرستِ دستی،
/// روزی که یکی عوض شود، یعنی مدل کلیدهایی را پیشنهاد کند که نگهبان رد می‌کند.
pub fn writable_keys_for_prompt() -> String {
    writable()
        .into_iter()
        .map(|(key, kind)| match kind {
            Kind::Bool => format!("{key} (true/false)"),
            Kind::Int(min, max) => format!("{key} ({min}-{max})"),
            Kind::Enum(options) => format!("{key} ({})", options.join("|")),
            Kind::Text(_) => format!("{key} (text)"),
            Kind::TextList(_, _) => format!("{key} (list of text)"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// فهرست کلیدهای نوشتنیِ **چت**، با شرحِ مقادیر، برای درج در پرامپت.
///
/// شرحِ هر کلید از `Kind` خودش تولید می‌شود تا همان اتفاقی که برای
/// `writable_keys_for_prompt` توضیح داده شد اینجا هم نیفتد: یک فهرستِ دستیِ دوم،
/// روزی که بازه‌ای عوض شود، به مدل دروغ می‌گوید.
pub fn chat_keys_for_prompt() -> String {
    chat_writable()
        .into_iter()
        .map(|(key, kind)| match kind {
            Kind::Bool => format!("  {key}: true | false"),
            Kind::Int(min, max) => format!("  {key}: a number from {min} to {max}"),
            Kind::Enum(options) => format!("  {key}: {}", options.join(" | ")),
            Kind::Text(max) => format!("  {key}: text, at most {max} characters"),
            Kind::TextList(max_items, _) => {
                format!("  {key}: comma separated, at most {max_items} entries, or empty for the default")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// مقدارِ کنونیِ یک کلید، به شکلی که به کاربر نشان داده می‌شود.
///
/// همان `AiPatch.read` موبایل، ولی بدون `match` فیلدبه‌فیلد: از نمایشِ JSONِ
/// پروفایل خوانده می‌شود تا `serde` تنها مرجعِ نام‌ها بماند. کارتِ پیشنهاد به این
/// نیاز دارد چون «mtu: 1500 → 1380» چیزی است که کاربر می‌تواند تأیید کند، و
/// «mtu: 1380» چیزی نیست.
pub fn read_key(profile: &ConnectionProfile, key: &str) -> String {
    let Ok(value) = serde_json::to_value(profile) else {
        return "(unknown)".into();
    };
    match value.get(key) {
        Some(found) => render(found),
        None => "(unknown)".into(),
    }
}

/// مقدارِ کنونیِ هر کلیدِ نوشتنیِ چت — بلوکِ زمینه‌ای که به پرامپت می‌رود.
pub fn chat_snapshot(profile: &ConnectionProfile) -> String {
    chat_writable()
        .keys()
        .map(|key| format!("  {key} = {}", read_key(profile, key)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile() -> ConnectionProfile {
        ConnectionProfile::default()
    }

    #[test]
    fn a_sensible_patch_is_applied() {
        let mut p = profile();
        let out = apply(
            &mut p,
            &json!({ "mtu": 1380, "fragment": true, "noize": "BALANCED" }),
        );
        assert_eq!(p.mtu, 1380);
        assert!(p.fragment);
        assert!(out.rejected.is_empty(), "{:?}", out.rejected);
        assert_eq!(out.applied.len(), 3);
    }

    #[test]
    fn the_leak_guard_can_never_be_switched_off() {
        let mut p = profile();
        let out = apply(&mut p, &json!({ "leakGuard": false }));
        // هم به‌عنوان کلید محافظت‌شده رد می‌شود…
        assert!(out.rejected.iter().any(|(k, _)| k == "leakGuard"));
        // …و هم اگر روزی از فهرست ممنوع بیفتد، `normalize` برش می‌گرداند.
        assert!(p.leak_guard);
    }

    #[test]
    fn credentials_are_not_writable_at_all() {
        let mut p = profile();
        let out = apply(
            &mut p,
            &json!({ "accessSecret": "abc", "accessToken": "def" }),
        );
        assert_eq!(out.rejected.len(), 2);
        assert!(!out.changed());
        assert!(p.access_secret.is_empty() && p.access_token.is_empty());
    }

    #[test]
    fn one_bad_key_does_not_void_the_good_ones() {
        let mut p = profile();
        let out = apply(
            &mut p,
            &json!({ "mtu": 99999, "fragment": true, "banana": 1 }),
        );
        assert!(p.fragment);
        assert_eq!(p.mtu, DEFAULT_MTU, "an out-of-range MTU must not land");
        assert_eq!(out.applied.len(), 1);
        assert_eq!(out.rejected.len(), 2);
    }

    #[test]
    fn loose_types_from_a_model_are_understood() {
        let mut p = profile();
        apply(
            &mut p,
            &json!({ "mtu": "1420", "fragment": "true", "keepalive": 25.0, "noize": "balanced" }),
        );
        assert_eq!(p.mtu, 1420);
        assert!(p.fragment);
        assert_eq!(p.keepalive, 25);
        assert_eq!(format!("{:?}", p.noize).to_uppercase(), "BALANCED");
    }

    #[test]
    fn reconnect_attempts_are_clamped_by_the_profile_too() {
        let mut p = profile();
        let out = apply(&mut p, &json!({ "reconnectAttempts": 2 }));
        // ۲ زیر کف است، پس هرگز اعمال نمی‌شود.
        assert!(out.rejected.iter().any(|(k, _)| k == "reconnectAttempts"));
        assert!((3..=20).contains(&p.reconnect_attempts));
    }

    #[test]
    fn a_list_is_cleaned_deduplicated_and_bounded() {
        let mut p = profile();
        apply(
            &mut p,
            &json!({ "dns": ["1.1.1.1", " 9.9.9.9 ", "1.1.1.1"] }),
        );
        assert_eq!(p.dns, ["1.1.1.1", "9.9.9.9"]);
        // شکل کاماشده هم — که مدل‌ها نصف وقت‌ها برمی‌گردانند.
        apply(&mut p, &json!({ "dns": "8.8.8.8, 8.8.4.4" }));
        assert_eq!(p.dns, ["8.8.8.8", "8.8.4.4"]);
    }

    #[test]
    fn a_no_op_is_reported_as_neither_applied_nor_rejected() {
        let mut p = profile();
        let out = apply(&mut p, &json!({ "mtu": DEFAULT_MTU }));
        assert!(out.applied.is_empty() && out.rejected.is_empty());
        assert!(!out.changed());
    }

    #[test]
    fn enum_lists_match_the_profile() {
        // هر مقدار نامی باید واقعاً روی *همان* فیلد پروفایل بنشیند. این تست همان
        // اشکالی را گرفت که کپی‌کردن فهرست‌های اندروید ساخته بود.
        let cases: [(&str, &[&str]); 8] = [
            ("backend", BACKENDS),
            ("protocol", PROTOCOLS),
            ("scanMode", SCAN_MODES),
            ("ipVersion", IP_VERSIONS),
            ("noize", NOIZE_MODES),
            ("endpointMode", ENDPOINT_MODES),
            ("splitMode", SPLIT_MODES),
            ("accessMode", ACCESS_MODES),
        ];
        for (key, options) in cases {
            for option in options {
                let mut p = profile();
                let out = apply(&mut p, &json!({ key: option }));
                assert!(
                    out.rejected.is_empty(),
                    "\"{key}\" rejected the value \"{option}\": {:?}",
                    out.rejected
                );
            }
        }
    }

    #[test]
    fn a_value_the_profile_refuses_does_not_void_the_other_changes() {
        // پیش از این، یک مقدار نامیِ نامعتبر کلِ پچ را می‌انداخت — بدون اینکه
        // بگوید کدام کلید مقصر بود.
        let mut p = profile();
        let out = apply(
            &mut p,
            &json!({ "mtu": 1380, "fragment": true, "noize": "MEDIUM" }),
        );
        assert_eq!(p.mtu, 1380, "a good change must survive a bad sibling");
        assert!(p.fragment);
        assert!(
            out.rejected.iter().any(|(k, _)| k == "noize"),
            "{:?}",
            out.rejected
        );
        assert_eq!(out.applied.len(), 2, "{:?}", out.applied);
    }

    #[test]
    fn the_prompt_key_list_comes_from_the_allow_list_itself() {
        let listed = writable_keys_for_prompt();
        assert!(listed.contains("mtu (1280-9000)"));
        assert!(listed.contains("reconnectAttempts (3-20)"));
        // و هیچ رازی داخلش نیست.
        assert!(!listed.contains("accessSecret") && !listed.contains("accessToken"));
    }

    #[test]
    fn desktop_key_names_are_used_not_the_android_ones() {
        // این تست، بدترین شکل شکستِ یک پورت کلمه‌به‌کلمه را می‌گیرد: کلید
        // اندرویدی باید رد شود و کلید دسکتاپی باید بنشیند.
        let mut p = profile();
        let out = apply(
            &mut p,
            &json!({ "dnsServers": ["1.1.1.1"], "reconnectRetryLimit": 7 }),
        );
        assert_eq!(out.rejected.len(), 2, "{:?}", out.rejected);
        // مقدار باید *غیرِ* مقدارِ فعلی باشد، وگرنه `apply` آن را «خبری نیست»
        // می‌شمارد و تست به‌جای نامِ کلید، پیش‌فرضِ پروفایل را می‌سنجد. با
        // پاریتیِ پیش‌فرض‌ها با موبایل ۱.۳.۰ همین اتفاق افتاد: پیش‌فرض ۵ شد و
        // `"reconnectAttempts": 5` بی‌صدا به no-op تبدیل شد.
        let target = if p.reconnect_attempts == 7 { 8 } else { 7 };
        assert_ne!(target, p.reconnect_attempts);
        let out2 = apply(
            &mut p,
            &json!({ "dns": ["1.1.1.1"], "reconnectAttempts": target }),
        );
        assert_eq!(out2.applied.len(), 2, "{:?}", out2);
        assert_eq!(p.reconnect_attempts, target);
    }

    // ---- مرزِ باریک‌ترِ چت -------------------------------------------------

    #[test]
    fn chat_keys_are_a_subset_of_the_advisor_keys() {
        // `chat_writable` با `expect` از `writable` می‌خواند، پس یک نامِ غلط
        // پانیک می‌کند. این تست همان پانیک را به یک شکستِ تست تبدیل می‌کند تا
        // پیش از بیلد گرفته شود و نه در دستِ کاربر.
        let chat = chat_writable();
        let all = writable();
        assert!(!chat.is_empty());
        for (key, kind) in &chat {
            assert_eq!(
                all.get(key),
                Some(kind),
                "{key} must match the advisor list"
            );
        }
    }

    #[test]
    fn the_chat_cannot_redirect_traffic_or_leak_a_domain() {
        // قلبِ این تست: هر کلیدی که فهرستِ چت عمداً حذف کرده، از مسیر چت رد
        // می‌شود — حتی وقتی مسیر مشاور آن را می‌پذیرد. اگر روزی کسی
        // `chat_writable` را با `writable` یکی کند، این تست است که می‌شکند.
        let dangerous = json!({
            "upstream": "socks5://10.0.0.9:1080",
            "routeDirect": ["bank.example.com"],
            "manualPeer": "203.0.113.7:443",
            "backend": "AETHER",
            "splitMode": "EXCLUDE",
            "lanShare": true,
            "killSwitch": false,
            "accessMode": "TOKEN",
        });

        let mut chat_profile = profile();
        let refused = apply_chat(&mut chat_profile, &dangerous);
        assert!(
            refused.applied.is_empty(),
            "chat applied {:?}",
            refused.applied
        );
        assert_eq!(refused.rejected.len(), 8, "{:?}", refused.rejected);
        assert_eq!(chat_profile, profile(), "the profile must be untouched");

        // و اثبات اینکه تست خودش بی‌معنا نیست: مسیر مشاور همین‌ها را می‌پذیرد،
        // پس رد‌شدنِ بالا از فهرستِ چت آمده و نه از بی‌اعتبار بودنِ مقادیر.
        let mut advisor_profile = profile();
        let accepted = apply(&mut advisor_profile, &dangerous);
        assert!(accepted.applied.len() >= 6, "{:?}", accepted);
    }

    #[test]
    fn a_tuning_change_still_goes_through_the_chat() {
        let mut p = profile();
        let out = apply_chat(
            &mut p,
            &json!({ "mtu": "1380", "noize": "GFW", "fragment": "on" }),
        );
        assert!(out.rejected.is_empty(), "{:?}", out.rejected);
        assert_eq!(p.mtu, 1380);
        assert!(p.fragment);
        assert_eq!(out.applied.len(), 3);
    }

    #[test]
    fn the_chat_prompt_list_and_snapshot_agree_with_the_allow_list() {
        let listed = chat_keys_for_prompt();
        assert!(
            listed.contains("mtu: a number from 1280 to 9000"),
            "{listed}"
        );
        assert!(listed.contains("noize: OFF | LIGHT"), "{listed}");
        // هیچ‌کدام از کلیدهای حذف‌شده به مدل حتی *پیشنهاد* نمی‌شوند.
        for banned in [
            "upstream",
            "routeDirect",
            "manualPeer",
            "backend",
            "accessMode",
        ] {
            assert!(
                !listed.contains(banned),
                "{banned} must not be offered to the model"
            );
        }
        let snapshot = chat_snapshot(&profile());
        assert!(
            snapshot.contains(&format!("mtu = {DEFAULT_MTU}")),
            "{snapshot}"
        );
        assert!(!snapshot.contains("accessSecret"));
    }

    #[test]
    fn read_key_renders_what_the_card_shows() {
        let mut p = profile();
        p.mtu = 1380;
        p.fragment = true;
        assert_eq!(read_key(&p, "mtu"), "1380");
        assert_eq!(read_key(&p, "fragment"), "on");
        assert_eq!(read_key(&p, "dns"), "(empty)");
        assert_eq!(read_key(&p, "banana"), "(unknown)");
    }
}

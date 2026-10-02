//! هویتِ ساخت — معادلِ ویندوزیِ `core/BuildProvenance.kt`.
//!
//! # چرا این فایل وجود دارد
//!
//! بنرِ موتور فقط نسخهٔ **بالادستِ** هسته را چاپ می‌کند (`Aether v2.0.0`) و آن
//! عدد در هر سطحِ پچ یکسان است. یعنی یک موتورِ کهنه و یک موتورِ تازه لاگِ
//! ابتداییِ یکسان می‌دهند. روی اندروید همین موضوع یک دورِ کاملِ تحلیل را سوزاند:
//! بعداً معلوم شد باینریِ تحتِ آزمون اصلاً آن فیکسی که دنبالش بودند را نداشت.
//!
//! پس هویت باید وجود داشته باشد و **در دو سر** بررسی شود:
//!
//!   * سطحِ پچِ خودِ برنامه از فایلِ `PATCHLEVEL` ریشهٔ مخزن در زمانِ کامپایل
//!     خوانده می‌شود ([`APP_PATCH_LEVEL`])؛
//!   * موتور همان مقدار را در `aether.exe` مهر می‌کند (`native/aether/aether/build.rs`)
//!     و در دومین سطرِ خروجی‌اش چاپ می‌کند؛
//!   * CI همان رشته را داخلِ payload بسته‌شده grep می‌کند، پس موتورِ کهنه
//!     قابلِ انتشار نیست؛
//!   * و [`ingest`] این دو را **در زمانِ اجرا** مقایسه می‌کند و ناهم‌خوانی را
//!     فریاد می‌زند.
//!
//! نتیجهٔ عملی برای هر کسی که لاگِ بعدی را می‌خواند: پاسخِ «آیا اصلاً بیلدِ درست
//! را آزمایش می‌کنم؟» همیشه در سطرهای اول است، و ناهم‌خوانی یک سطرِ `E/build`
//! است که نمی‌شود ندید.

use crate::log::DiagnosticsLog;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const TAG: &str = "build";

/// این برنامه با چه سطحِ پچی ساخته شده — از `PATCHLEVEL` ریشهٔ مخزن.
///
/// `src-tauri/build.rs` این را تنظیم می‌کند؛ اگر فایل پیدا نشود مقدارِ
/// `unstamped` می‌آید که عمداً بلند و greppable است.
pub const APP_PATCH_LEVEL: &str = env!("AETHER_APP_PATCHLEVEL");

/// مقداری که یک ساختِ بی‌هویت می‌گیرد — نه یک نسخهٔ ساختگی.
pub const UNSTAMPED: &str = "unstamped";

static REPORTED: AtomicBool = AtomicBool::new(false);
static ENGINE_LEVEL: Mutex<Option<String>> = Mutex::new(None);

/// حکمِ مقایسهٔ سطحِ پچِ برنامه و موتور.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// هر دو یکی‌اند — بیلد از درون سازگار است.
    Consistent,
    /// موتور مهر ندارد، پس قابلِ شناسایی نیست.
    EngineUnstamped,
    /// خودِ برنامه مهر ندارد؛ مقایسه بی‌معنی است.
    AppUnstamped,
    /// دو عددِ متفاوت — موتورِ کهنه.
    Mismatch,
}

/// مهرِ ساخت را از یک سطرِ خروجیِ موتور بیرون می‌کشد.
///
/// ارزان است: در مسیرِ داغ یک `contains` و بس، پس روی هزاران سطری که بنر
/// نیستند هیچ هزینه‌ای ندارد و پیش از هر تخصیصی برمی‌گردد.
///
/// شکلِ رشته عیناً همان چیزی است که `build.rs` جاسازی می‌کند:
/// `AETHER-BUILD-STAMP:<سطح>` و سطح از `[0-9A-Za-z.-]` تشکیل می‌شود.
pub fn stamp_in(line: &str) -> Option<&str> {
    const MARK: &str = "AETHER-BUILD-STAMP:";
    let start = line.find(MARK)? + MARK.len();
    let rest = &line[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some(&rest[..end])
}

/// حکم را از دو مقدار می‌سازد — تابعِ خالص، پس آزمون‌پذیر بی هیچ موتوری.
pub fn verdict(app: &str, engine: &str) -> Verdict {
    if engine == UNSTAMPED {
        Verdict::EngineUnstamped
    } else if app == UNSTAMPED {
        Verdict::AppUnstamped
    } else if app == engine {
        Verdict::Consistent
    } else {
        Verdict::Mismatch
    }
}

/// در هر اتصال، پیش از راه‌افتادنِ موتور صدا زده می‌شود.
pub fn log_app_identity(core_version: &str) {
    REPORTED.store(false, Ordering::Relaxed);
    if let Ok(mut slot) = ENGINE_LEVEL.lock() {
        *slot = None;
    }
    DiagnosticsLog::i(
        TAG,
        &format!(
            "App patch level {} (version {}, core {}). Quote this line in any bug \
             report - it is what identifies the build.",
            APP_PATCH_LEVEL,
            env!("CARGO_PKG_VERSION"),
            core_version,
        ),
    );
    if APP_PATCH_LEVEL == UNSTAMPED {
        DiagnosticsLog::w(
            TAG,
            "This build has no PATCHLEVEL stamp, so it cannot be identified. Build it \
             from the repository root so PATCHLEVEL is picked up.",
        );
    }
}

/// خروجیِ موتور را می‌پاید و نخستین مهر را با سطحِ پچِ برنامه می‌سنجد.
pub fn ingest(line: &str) {
    if REPORTED.load(Ordering::Relaxed) || !line.contains("AETHER-BUILD-STAMP:") {
        return;
    }
    let Some(found) = stamp_in(line) else { return };
    REPORTED.store(true, Ordering::Relaxed);
    if let Ok(mut slot) = ENGINE_LEVEL.lock() {
        *slot = Some(found.to_string());
    }
    match verdict(APP_PATCH_LEVEL, found) {
        Verdict::Consistent => DiagnosticsLog::i(
            TAG,
            &format!(
                "Engine patch level {found} matches the app. This build is internally consistent."
            ),
        ),
        Verdict::EngineUnstamped => DiagnosticsLog::w(
            TAG,
            "The engine reports no patch level, so it cannot be told apart from any \
             other build of this core. Nothing this session says about the data plane \
             can be attributed to a specific build.",
        ),
        Verdict::AppUnstamped => DiagnosticsLog::w(
            TAG,
            &format!(
                "The engine is patch level {found} but this build carries no stamp of its \
                 own, so the two cannot be compared."
            ),
        ),
        Verdict::Mismatch => DiagnosticsLog::e(
            TAG,
            &format!(
                "ENGINE/APP PATCH LEVEL MISMATCH: the app is {APP_PATCH_LEVEL} but \
                 aether.exe is {found}. You are testing a STALE ENGINE and any conclusion \
                 drawn from this session about the data plane will be wrong. Rebuild the \
                 engine (scripts/build-engine.ps1) and reinstall before testing further."
            ),
        ),
    }
}

/// سطحی که موتور گزارش کرد — برای تشخیص‌ها.
pub fn engine_patch_level() -> Option<String> {
    ENGINE_LEVEL.lock().ok().and_then(|s| s.clone())
}

/// آیا موتور خودش را معرفی کرده و با برنامه می‌خواند؟
pub fn consistent() -> bool {
    engine_patch_level()
        .map(|e| e == APP_PATCH_LEVEL)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// شکلِ واقعیِ سطری که هستهٔ ۲.۰.۰ چاپ می‌کند — عیناً از لاگِ میدانی.
    const REAL_LINE: &str = "[2026-09-14T14:51:51.208Z INFO  aether] [*] AETHER-BUILD-STAMP:1.3.0 \
                             (app patch level 1.3.0; netstack congestion control=cubic)";

    #[test]
    fn the_stamp_is_read_out_of_a_real_engine_line() {
        assert_eq!(stamp_in(REAL_LINE), Some("1.3.0"));
    }

    /// سطرهایی که مهر ندارند نباید چیزی برگردانند — این مسیرِ داغِ هزاران
    /// سطرِ دیگر است.
    #[test]
    fn ordinary_lines_carry_no_stamp() {
        for line in [
            "[*] performance profile: High (cpus=8)",
            "Aether v2.0.0",
            "",
            "AETHER-BUILD-STAMP",
            "AETHER-BUILD-STAMP:",
        ] {
            assert_eq!(stamp_in(line), None, "line `{line}` produced a stamp");
        }
    }

    /// سطحِ پچ می‌تواند پسوند داشته باشد (اندروید `1.2.8-r5` می‌فرستد) و باید
    /// کامل خوانده شود، نه تا اولین خط تیره.
    #[test]
    fn a_suffixed_patch_level_survives_intact() {
        assert_eq!(
            stamp_in("x AETHER-BUILD-STAMP:1.2.8-r5 y"),
            Some("1.2.8-r5")
        );
        assert_eq!(stamp_in("AETHER-BUILD-STAMP:1.2.5"), Some("1.2.5"));
    }

    /// و مهر تا اولین نویسهٔ نامعتبر خوانده می‌شود، نه تا انتهای سطر.
    #[test]
    fn the_stamp_stops_at_the_first_invalid_character() {
        assert_eq!(
            stamp_in("AETHER-BUILD-STAMP:1.2.5 (app patch level"),
            Some("1.2.5")
        );
        assert_eq!(stamp_in("AETHER-BUILD-STAMP:1.2.5)"), Some("1.2.5"));
    }

    /// چهار حکمِ ممکن — همان چیزی که تفاوتِ «موتورِ کهنه» و «بیلدِ بی‌هویت» را
    /// در لاگ روشن نگه می‌دارد.
    #[test]
    fn every_verdict_is_reachable() {
        assert_eq!(verdict("1.2.5", "1.2.5"), Verdict::Consistent);
        assert_eq!(verdict("1.2.5", "1.2.4"), Verdict::Mismatch);
        assert_eq!(verdict("1.2.5", UNSTAMPED), Verdict::EngineUnstamped);
        assert_eq!(verdict(UNSTAMPED, "1.2.5"), Verdict::AppUnstamped);
    }

    /// موتورِ بی‌مهر مهم‌تر از برنامهٔ بی‌مهر است: اگر هر دو بی‌مهر باشند،
    /// حکم باید همان «موتور بی‌مهر» بماند، چون آن است که مسیرِ داده را دارد.
    #[test]
    fn an_unstamped_engine_outranks_an_unstamped_app() {
        assert_eq!(verdict(UNSTAMPED, UNSTAMPED), Verdict::EngineUnstamped);
    }

    /// همان اشتباهی که این ماژول برای گرفتنش نوشته شد: نسخهٔ بالادستِ هسته
    /// یکی است ولی سطحِ پچ فرق دارد — و این باید Mismatch شود، نه Consistent.
    #[test]
    fn the_same_core_with_a_different_patch_level_is_still_a_mismatch() {
        let stale = "[*] AETHER-BUILD-STAMP:1.2.4 (app patch level 1.2.4)";
        let found = stamp_in(stale).unwrap();
        assert_eq!(verdict("1.2.5", found), Verdict::Mismatch);
    }

    /// این ساختِ درخت باید مهر داشته باشد: اگر `build.rs` از کار بیفتد،
    /// `APP_PATCH_LEVEL` بی‌صدا `unstamped` می‌شود و کلِ زنجیره بی‌اثر است.
    #[test]
    fn this_build_is_stamped_from_the_repository_patchlevel() {
        assert_ne!(
            APP_PATCH_LEVEL, UNSTAMPED,
            "build.rs did not pick up the repo-root PATCHLEVEL file"
        );
    }
}

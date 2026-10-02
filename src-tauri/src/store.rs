//! پورت از `data/ProfileStore.kt`.
//!
//! در اندروید DataStore بود؛ در ویندوز یک فایل JSON در %LOCALAPPDATA%\Aether.
//! رفتار یکسان است: خواندن هرگز خطا نمی‌دهد — فایل خراب یعنی پیش‌فرض‌ها.

use crate::log::DiagnosticsLog;
use crate::profile::{ConnectionProfile, TransportBackend, SETTINGS_REV};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("profile.json"),
        }
    }

    pub fn load(&self) -> ConnectionProfile {
        let mut profile = match std::fs::read_to_string(&self.path) {
            Ok(raw) => match serde_json::from_str::<ConnectionProfile>(&raw) {
                Ok(p) => p,
                Err(e) => {
                    DiagnosticsLog::w(
                        "store",
                        &format!("Profile unreadable ({e}); using defaults."),
                    );
                    ConnectionProfile::default()
                }
            },
            Err(_) => ConnectionProfile::default(),
        };
        if migrate(&mut profile) {
            let _ = self.save(&profile);
        }
        profile
    }

    /// نوشتن اتمیک: اول فایل موقت، بعد rename. قطع برق وسط ذخیره
    /// نباید تنظیمات کاربر را نابود کند.
    pub fn save(&self, profile: &ConnectionProfile) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(profile)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Brings a profile written by an older build up to the current defaults.
///
/// Returns `true` when something changed, so the caller can persist it once.
///
/// ## rev 2 - the MASQUE carrier toggle
///
/// `MASQUE over HTTP/2` is a fallback for networks that block HTTP/3, and it is
/// the one data plane whose flow control is a single HTTP/2 window shared by the
/// whole machine. Two things could leave it stuck on: the user ticking it once
/// while chasing a connection problem, and - until 1.2.3-p2 - Smart Auto's
/// hardened pass, which used to turn it on unconditionally and then wrote the
/// profile back. Either way the setting long outlived the network condition that
/// justified it, and every later session paid for it in download speed.
///
/// Turning it back off is safe: the pre-connect probe now measures UDP for real,
/// so a network that genuinely needs the TCP carrier gets it anyway, on evidence
/// rather than on a stale checkbox.
///
/// ## rev 3 - the connection backend goes back to Aether
///
/// `ConnectionProfile::default()` and `default_backend()` have always been
/// `Aether`, so a genuinely fresh profile starts there. What made the app come up
/// on a Tor pipeline anyway is that `profile.json` lives in `%LOCALAPPDATA%` and
/// outlives an uninstall: whatever backend was selected while chasing a
/// connection problem is still there after reinstalling, and Tor is the slowest
/// and least likely of them to come up on its own.
///
/// The backend is the one setting where a leftover choice costs the user the
/// whole session, so it is reset once - the same reasoning as rev 2, and the
/// mechanism these revisions exist for. A user who wants Tor picks it again and
/// keeps it: later revisions do not touch it.
fn migrate(profile: &mut ConnectionProfile) -> bool {
    if profile.settings_rev >= SETTINGS_REV {
        return false;
    }

    if profile.backend != TransportBackend::Aether {
        let was = profile.backend;
        profile.backend = TransportBackend::Aether;
        DiagnosticsLog::i(
            "store",
            &format!(
                "Settings migration: the connection backend was still '{was:?}' from an earlier \
                 session and is back on 'Aether', the default. Pick another backend in Settings \
                 to keep it."
            ),
        );
    }

    if profile.masque_http2 {
        profile.masque_http2 = false;
        DiagnosticsLog::i(
            "store",
            "Settings migration: 'MASQUE over HTTP/2' was left on from an earlier session and has been turned off. \
             The HTTP/3 carrier is faster, and the network probe now selects HTTP/2 by itself when UDP is blocked.",
        );
    }

    profile.settings_rev = SETTINGS_REV;
    true
}

// ===========================================================================
//  v12 — انبار ترجیحاتِ کوچک (فقط لایهٔ هوش مصنوعی)
// ===========================================================================

/// یک انبار کلید/مقدارِ متنی برای ترجیحاتی که نه پروفایل اتصال‌اند و نه راز.
///
/// # چرا یک انبار سومِ کوچک
///
/// لایهٔ هوش مصنوعی دو چیز را باید به یاد بسپارد: مدلِ انتخاب‌شده و فهرست
/// مدل‌هایی که کلیدِ کاربر آخرین بار می‌دید. هیچ‌کدام جایی برای زندگی نداشتند و
/// هر سه گزینهٔ موجود غلط بودند:
///
/// * **`profile.json`** — پروفایل *پیکربندی تونل* است و با «بازنشانی همهٔ
///   تنظیمات» پاک می‌شود. انتخاب مدل به تونل بی‌ربط است و نباید با ریست‌کردن
///   MTU از بین برود.
/// * **`secrets.bin`** — با DPAPI مهر می‌شود. شناسهٔ یک مدل راز نیست، و
///   گذاشتنش آنجا هزینهٔ رمزنگاری را به داده‌ای عمومی می‌داد و مرزِ «هر چیزی که
///   در این فایل است محرمانه است» را گل‌آلود می‌کرد.
/// * **`localStorage`** — جایی است که ترجیحات رابط کاربری (زبان) می‌مانند، ولی
///   *سمت جاوااسکریپت* است، و این مقادیر را **Rust** موقع ساخت نشست می‌خواند.
///   خواندنشان از فرانت‌اند یعنی هر فراخوان هوش مصنوعی باید مدل را به‌عنوان
///   پارامتر پاس بدهد و فهرست مجاز روی مرزی اعمال شود که کاربر کنترلش می‌کند.
///
/// همان قواعد `ProfileStore`: خواندن هرگز خطا نمی‌دهد، نوشتن اتمیک است.
pub struct PrefsStore {
    path: PathBuf,
}

impl PrefsStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("prefs.json"),
        }
    }

    fn read_all(&self) -> std::collections::BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.read_all().get(key).cloned().filter(|v| !v.is_empty())
    }

    pub fn set_string(&self, key: &str, value: &str) -> Result<()> {
        let mut all = self.read_all();
        if value.is_empty() {
            all.remove(key);
        } else {
            all.insert(key.to_string(), value.to_string());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&all)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{ConnectionProfile, TransportBackend};

    /// rev 3: ein gespeichertes Tor-Backend faellt einmalig auf Aether zurueck.
    #[test]
    fn a_profile_from_an_older_build_comes_back_on_aether() {
        let mut old = ConnectionProfile {
            backend: TransportBackend::Tor,
            settings_rev: 2,
            ..ConnectionProfile::default()
        };

        assert!(migrate(&mut old));
        assert_eq!(old.backend, TransportBackend::Aether);
        assert_eq!(old.settings_rev, SETTINGS_REV);
    }

    /// Und danach nicht mehr: wer Tor bewusst waehlt, behaelt es.
    #[test]
    fn a_current_profile_keeps_the_backend_the_user_picked() {
        let mut mine = ConnectionProfile {
            backend: TransportBackend::Tor,
            settings_rev: SETTINGS_REV,
            ..ConnectionProfile::default()
        };

        assert!(!migrate(&mut mine));
        assert_eq!(mine.backend, TransportBackend::Tor);
    }
}

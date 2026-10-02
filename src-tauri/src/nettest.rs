//! تب «تست شبکه» — پینگ، سرعت دانلود و سرعت آپلود، همه از مسیر تونل.
//!
//! # چرا اندازه‌گیری از مسیر تونل و نه از شبکهٔ محلی
//!
//! هر عددی که کاربر در این تب می‌بیند باید جوابِ «اینترنت من از پشت فیلتر
//! چقدر است» را بدهد، نه «روتر من چقدر است». پس همه‌چیز روی همان SOCKS5
//! محلیِ خودِ تونل سوار می‌شود که `probe` و `ai_http` هم از آن استفاده
//! می‌کنند — همان مسیر، همان DNS روی نقطهٔ خروج، همان گواهی‌ها. یک تست
//! مستقیم روی شبکهٔ اپراتور، عددی می‌داد که کاربر دقیقاً می‌داند بی‌فایده است.
//!
//! # چرا پینگ = زمانِ SOCKS CONNECT
//!
//! `socks5_stream` یک `CONNECT` کامل (با DOMAIN، پس DNS هم روی تونل) است و
//! زمانش مجموعِ RTT تا نقطهٔ خروج + پاسخ مقصد تا Accept شدن. این صادقانه‌ترین
//! «پینگ»ی است که روی TCP از پشت تونل می‌شود گرفت؛ ICMP در این مسیر وجود
//! ندارد و یک TLS کامل برای هر نمونه، هزینه‌اش سه برابر RTT است.
//!
//! # چرا speed.cloudflare.com
//!
//! یک میزبان، دو نقطهٔ پایانیِ بی‌هزینه و بی‌احراز هویت (`__down?bytes=N`,
//! `__up`) که فقط بایت برمی‌گردانند. Cloudflare از همین IP‌های خروجیِ تونل
//! در دسترس است و برخلاف سرویس‌های تست سرعت کلاسیک، آگهی ویدیویی و اندازهٔ
//! نمونهٔ متغیر ندارد. تعداد بایت عمداً طوری انتخاب شده که حتی روی تونل‌های
//! ~۲۰Mbps هم بودجهٔ زمانی ۱۲ ثانیه‌ای تمام شود و تست هر دو جهت روی هم
//! کمتر از نیم دقیقه طول بکشد.

use crate::log::DiagnosticsLog;
use crate::probe;
use serde::Serialize;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// میزبان تست سرعت. `__down` دقیقاً همان تعداد بایت که بخواهیم برمی‌گرداند.
const SPEED_HOST: &str = "speed.cloudflare.com";
/// هدف‌های پینگ، چرخشی. هر سه روی Cloudflare‌اند چون نقطهٔ خروجِ تونل
/// خودِ Cloudflare/Psiphon است و می‌خواهیم RTTِ مسیر را بسنجیم نه رفتار
/// یک سرور متفرقه را.
const PING_TARGETS: [&str; 3] = ["one.one.one.one", "www.cloudflare.com", SPEED_HOST];
const PING_ROUNDS: usize = 2;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// تایم‌اوت هر *خواندن*. کوتاه است چون بودجهٔ کلِ فاز جدا نگه داشته می‌شود:
/// یک خواندنِ معلق نباید ۱۲ ثانیهٔ فاز را تنهایی بلعد.
const READ_TIMEOUT: Duration = Duration::from_secs(4);
const WRITE_TIMEOUT: Duration = Duration::from_secs(6);
const DOWN_BYTES: usize = 25 * 1024 * 1024;
const DOWN_BUDGET: Duration = Duration::from_secs(12);
const UP_BYTES: usize = 8 * 1024 * 1024;
const UP_BUDGET: Duration = Duration::from_secs(12);

/// آنچه رابط کاربری می‌بیند — بازتابِ همان چیزی که روی `aether://nettest`
/// منتشر می‌شود. `progress` همیشه نسبتِ ۰..۱ درونِ فازِ جاری است.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetTestView {
    /// IDLE | RUNNING | DONE | FAILED
    pub status: String,
    /// "" | Ping | Download | Upload
    pub phase: String,
    pub progress: f64,
    pub pings: Vec<u32>,
    pub ping_min_ms: Option<u32>,
    pub ping_avg_ms: Option<u32>,
    pub ping_max_ms: Option<u32>,
    /// میانگینِ اختلافِ نمونه‌های پی‌در‌و — همان چیزی که مکالمهٔ زنده و گیم را
    /// می‌کُشد حتی وقتی خودِ میانگین پینگ خوب است.
    pub ping_jitter_ms: Option<u32>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub error: Option<String>,
}

impl Default for NetTestView {
    fn default() -> Self {
        Self {
            status: "IDLE".to_string(),
            phase: String::new(),
            progress: 0.0,
            pings: Vec::new(),
            ping_min_ms: None,
            ping_avg_ms: None,
            ping_max_ms: None,
            ping_jitter_ms: None,
            download_mbps: None,
            upload_mbps: None,
            error: None,
        }
    }
}

static VIEW: parking_lot::Mutex<NetTestView> = parking_lot::Mutex::new(NetTestView {
    status: String::new(),
    phase: String::new(),
    progress: 0.0,
    pings: Vec::new(),
    ping_min_ms: None,
    ping_avg_ms: None,
    ping_max_ms: None,
    ping_jitter_ms: None,
    download_mbps: None,
    upload_mbps: None,
    error: None,
});

static BUSY: AtomicBool = AtomicBool::new(false);

pub fn snapshot() -> NetTestView {
    let mut view = VIEW.lock().clone();
    // مقدارِ اولیهٔ static باید const باشد، پس "IDLE" در همان init نمی‌نشیند؛
    // اینجا پر می‌شود تا رابط هرگز رشتهٔ خالی نبیند.
    if view.status.is_empty() {
        view.status = "IDLE".to_string();
    }
    view
}

fn update(
    publish: &impl Fn(&NetTestView),
    f: impl FnOnce(&mut NetTestView),
) {
    {
        let mut view = VIEW.lock();
        f(&mut view);
    }
    publish(&snapshot());
}

/// یک تست کامل را روی رشتهٔ خودش شروع می‌کند.
///
/// `Err` فقط وقتی برگ می‌گردد که *نمی‌شود شروع کرد** (تست دیگری در حال اجرا
/// است). شکستهای حینِ اجرا در خودِ snapshot (`status = FAILED` + `error`)
/// می‌نشینند، چون رابط کاربری آن لحظه دارد به همان snapshot گوش می‌دهد و
/// نباید دو مسیرِ گزارش‌دهی داشته باشد.
pub fn start(publish: impl Fn(&NetTestView) + Send + 'static) -> Result<(), String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("A network test is already running.".to_string());
    }
    update(
        &publish,
        |v| {
            *v = NetTestView {
                status: "RUNNING".to_string(),
                phase: "Ping".to_string(),
                ..Default::default()
            };
        },
    );
    let spawned = std::thread::Builder::new()
        .name("aether-nettest".into())
        .spawn(move || {
            run(&publish);
            BUSY.store(false, Ordering::SeqCst);
        });
    match spawned {
        // JoinHandle عمداً دور ریخته می‌شود: رشتهٔ تست خودش تمام می‌شود و
        // هیچ‌کس قرار نیست به آن بپیوندد.
        Ok(_) => Ok(()),
        Err(_) => {
            BUSY.store(false, Ordering::SeqCst);
            Err("Could not start the test thread.".to_string())
        }
    }
}

fn run(publish: &impl Fn(&NetTestView)) {
    let samples = ping_all();
    if samples.is_empty() {
        fail(
            publish,
            "No target answered through the tunnel. Is the connection up?",
        );
        return;
    }
    let stats = stats(&samples);
    update(
        publish,
        |v| {
            v.pings = samples;
            if let Some((min, avg, max, jitter)) = stats {
                v.ping_min_ms = Some(min);
                v.ping_avg_ms = Some(avg);
                v.ping_max_ms = Some(max);
                v.ping_jitter_ms = Some(jitter);
            }
            v.progress = 1.0;
            v.phase = "Download".to_string();
        },
    );

    match download(publish) {
        Ok(mbps) => DiagnosticsLog::i("nettest", &format!("download {mbps:.1} Mbps")),
        Err(e) => {
            fail(publish, &format!("Download test failed: {e}"));
            return;
        }
    }
    update(
        publish,
        |v| {
            v.phase = "Upload".to_string();
            v.progress = 0.0;
        },
    );
    match upload(publish) {
        Ok(mbps) => DiagnosticsLog::i("nettest", &format!("upload {mbps:.1} Mbps")),
        Err(e) => {
            fail(publish, &format!("Upload test failed: {e}"));
            return;
        }
    }
    update(
        publish,
        |v| {
            v.status = "DONE".to_string();
            v.phase = String::new();
            v.progress = 1.0;
        },
    );
}

fn fail(publish: &impl Fn(&NetTestView), message: &str) {
    DiagnosticsLog::w("nettest", message);
    update(
        publish,
        |v| {
            v.status = "FAILED".to_string();
            v.error = Some(message.to_string());
        },
    );
}

// ---- پینگ ------------------------------------------------------------------

fn ping_all() -> Vec<u32> {
    let mut out = Vec::with_capacity(PING_TARGETS.len() * PING_ROUNDS);
    for round in 0..PING_ROUNDS {
        for target in PING_TARGETS {
            let began = Instant::now();
            let opened = probe::socks5_stream(target, 443, CONNECT_TIMEOUT).is_some();
            if opened {
                out.push(began.elapsed().as_millis() as u32);
            } else {
                DiagnosticsLog::w("nettest", &format!("ping {target} (round {}) got no path", round + 1));
            }
        }
    }
    out
}

// ---- دانلود/آپلود -----------------------------------------------------------

/// یک سوکت TLS تازه از مسیر تونل. هر فاز سوکتِ خودش را می‌گیرد: اشتراک‌گذاری
/// یک اتصالِ keep-alive بین فازها یعنی نتیجهٔ آپلود تحت‌تأثیرِ کُندیِ خواندنِ
/// پاسخِ دانلود بماند.
fn tls_via_tunnel(host: &str) -> Result<native_tls::TlsStream<std::net::TcpStream>, String> {
    let stream = probe::socks5_stream(host, 443, CONNECT_TIMEOUT)
        .ok_or_else(|| format!("could not open a path to {host} through the tunnel"))?;
    let _ = stream.set_nodelay(true);
    let connector = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    // دست‌دهه زیر همان تایم‌اوتِ ۸ ثانیه‌ایِ SOCKS می‌ماند؛ تایم‌اوت‌های فاز
    // *بعد از* دست‌دهه و روی سوکتِ TCP گذاشته می‌شوند چون TlsStream خودش
    // متدِ تایم‌اوت ندارد و خطا را از همان سوکت به read/write منتقل می‌کند.
    let mut tls = connector.connect(host, stream).map_err(|e| e.to_string())?;
    let _ = tls.get_mut().set_read_timeout(Some(READ_TIMEOUT));
    let _ = tls.get_mut().set_write_timeout(Some(WRITE_TIMEOUT));
    Ok(tls)
}

fn request_head(method: &str, host: &str, path: &str, content_length: Option<usize>) -> String {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Aether-Windows/{}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(len) = content_length {
        head.push_str(&format!("Content-Length: {len}\r\n"));
    }
    head.push_str("\r\n");
    head
}

fn download(publish: &impl Fn(&NetTestView)) -> Result<f64, String> {
    let mut tls = tls_via_tunnel(SPEED_HOST)?;
    tls.write_all(request_head("GET", SPEED_HOST, &format!("/__down?bytes={DOWN_BYTES}"), None).as_bytes())
        .map_err(|e| e.to_string())?;
    let began = Instant::now();
    let mut buf = [0u8; 64 * 1024];
    let mut head: Vec<u8> = Vec::new();
    let mut body_started = false;
    let mut got: usize = 0;
    let mut last_pct = -1.0;
    loop {
        if began.elapsed() > DOWN_BUDGET {
            break;
        }
        match tls.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if !body_started {
                    head.extend_from_slice(&buf[..n]);
                    if let Some(cut) = index_of_header_end(&head) {
                        got += head.len() - cut;
                        head.clear();
                        body_started = true;
                    }
                    continue;
                }
                got += n;
            }
            // تایم‌اوتِ خواندن یعنی *بودجه* هنوز تمام نشده و جریان کند است —
            // شمارشِ ادامه‌دار با چکِ بالای حلقه درست‌ترین حدسِ ممکن است.
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e.to_string()),
        }
        // انتشارِ پیشروی فقط هر ~۵٪: هر ۶۴ کیلوبایت یک رخداد IPC، تست را به
        // بمبارانِ رابط کاربری تبدیل می‌کرد.
        let pct = got.min(DOWN_BYTES) as f64 / DOWN_BYTES as f64;
        if pct - last_pct >= 0.05 || pct >= 1.0 {
            last_pct = pct;
            update(publish, |v| v.progress = pct.min(1.0));
        }
    }
    if !body_started || got == 0 {
        return Err("the server never started sending the body".to_string());
    }
    let rate = mbps(got as u64, began.elapsed());
    update(publish, |v| v.download_mbps = Some(rate));
    Ok(rate)
}

fn upload(publish: &impl Fn(&NetTestView)) -> Result<f64, String> {
    let mut tls = tls_via_tunnel(SPEED_HOST)?;
    tls.write_all(request_head("POST", SPEED_HOST, "/__up", Some(UP_BYTES)).as_bytes())
        .map_err(|e| e.to_string())?;
    let chunk = [0u8; 64 * 1024];
    let began = Instant::now();
    let mut sent: usize = 0;
    let mut last_pct = -1.0;
    while sent < UP_BYTES {
        if began.elapsed() > UP_BUDGET {
            break;
        }
        // `write` دستی و نه `write_all`: با تایم‌اوتِ نوشتن، نوشتنِ نیمه ممکن
        // است و write_all آن را به خطا می‌شکست درحالی‌که ادامه‌دادنش سالم است.
        // ولی تایم‌اوتِ *وسطِ یک رکوردِ TLS* را نمی‌شود ادامه داد (بایت‌های
        // رفته‌برگشت‌نشدنی‌اند)، پس TimedOut اینجا شکستِ فاز است نه continue.
        match tls.write(&chunk) {
            Ok(0) => break,
            Ok(n) => sent += n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
            Err(e) if e.kind() == ErrorKind::TimedOut => break,
            Err(e) => return Err(e.to_string()),
        }
        let pct = sent as f64 / UP_BYTES as f64;
        if pct - last_pct >= 0.05 || pct >= 1.0 {
            last_pct = pct;
            update(publish, |v| v.progress = pct.min(1.0));
        }
    }
    tls.flush().map_err(|e| e.to_string())?;
    // پاسخ فقط خوانده می‌شود تا سوکت مؤدبانه بسته شود؛ محتوایش (اکوی بدنه)
    // عمداً دور ریخته می‌شود.
    let _ = tls.read(&mut [0u8; 256]);
    // نصفِ نشدنیِ بار یعنی مسیر وسطِ راه ریخته — عددی که گزارش می‌شود باید
    // همان باشد که واقعاً رفته، پس نرخ روی *نوشته‌شده* حساب می‌شود ولی شکست
    // گزارش می‌شود تا کاربر عددِ نصفه را با نتیجهٔ کامل اشتباه نگیرد.
    if sent < UP_BYTES / 2 {
        return Err(format!(
            "only {} KB of the sample could be sent in time",
            sent / 1024
        ));
    }
    let rate = mbps(sent as u64, began.elapsed());
    update(publish, |v| v.upload_mbps = Some(rate));
    Ok(rate)
}

// ---- ریاضیاتِ خالص (تست‌پذیر) ------------------------------------------------

fn mbps(bytes: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64().max(0.001);
    (bytes as f64 * 8.0 / secs / 1e6 * 10.0).round() / 10.0
}

/// (min, avg, max, jitter) — jitter میانگینِ اختلافِ نمونه‌های پی‌در‌و است.
fn stats(samples: &[u32]) -> Option<(u32, u32, u32, u32)> {
    if samples.is_empty() {
        return None;
    }
    let min = *samples.iter().min()?;
    let max = *samples.iter().max()?;
    let sum: u64 = samples.iter().map(|&s| s as u64).sum();
    let avg = (sum / samples.len() as u64) as u32;
    let jitter = if samples.len() < 2 {
        0
    } else {
        let deltas: u64 = samples.windows(2).map(|w| w[0].abs_diff(w[1]) as u64).sum();
        (deltas / (samples.len() as u64 - 1)) as u32
    };
    Some((min, avg, max, jitter))
}

fn index_of_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_are_bytes_per_second_times_eight() {
        // ۱۲٫۵ مگابایت در ۱۰ ثانیه = ۱۰ مگابیت.
        assert_eq!(mbps(12_500_000, Duration::from_secs(10)), 10.0);
        // صفرِ مطلق هرگز تقسیم بر صفر نمی‌شود.
        assert!(mbps(0, Duration::ZERO).is_finite());
    }

    #[test]
    fn jitter_is_the_mean_consecutive_delta() {
        let (min, avg, max, jitter) = stats(&[100, 120, 100, 120]).unwrap();
        assert_eq!((min, avg, max), (100, 110, 120));
        // سه اختلافِ پی‌در‌و، هر کدام ۲۰ میلی‌ثانیه.
        assert_eq!(jitter, 20);
        assert!(stats(&[]).is_none());
        // یک نمونه: تعریفاً بدون نوسان.
        assert_eq!(stats(&[77]).unwrap().3, 0);
    }

    #[test]
    fn the_header_cut_is_after_the_blank_line() {
        let buf = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc";
        let cut = index_of_header_end(buf).unwrap();
        assert_eq!(&buf[cut..], b"abc");
        assert_eq!(index_of_header_end(b"HTTP/1.1 200"), None);
    }
}

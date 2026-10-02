//! تور به‌عنوان یک فرزندِ نظارت‌شده: `tor.exe` رسمی، torrc، و پورتِ کنترل.
//!
//! # چرا این ماژول وجود دارد
//!
//! تا ۱.۲.۵ مسیرِ تورِ برنامه از `aether.exe --tor-only` می‌گذشت، و آن مسیر
//! روی arti (کتابخانهٔ تورِ Rust) سوار است. لاگِ میدانیِ ۱۶ سپتامبر ۲۰۲۶
//! (`loge1.txt`) نشان می‌دهد این مسیر در ایران به مقصد نمی‌رسد و روی ۱۵٪
//! می‌ماند، و سه محدودیتِ arti در همان لاگ دیده می‌شود:
//!
//! * arti برای هر پل «هویت» می‌خواهد. سطرِ `meek_lite` که خودِ Tor منتشر
//!   می‌کند فینگرپرینت ندارد، پس در arti دو میلی‌ثانیه‌ای می‌سوزد:
//!   «none of the bridges could be read by tor».
//! * فهرستِ پل‌های داخلی را ما دستی نگه می‌داشتیم. `pt_config.json`ی که در
//!   بستهٔ رسمیِ Tor است `fronts=app.datapacket.com,www.datapacket.com` دارد
//!   — جمع، همان چیزی که lyrebird 0.6.1 می‌فهمد — و ما مجبور بودیم آن را
//!   برای arti تغییر دهیم.
//! * هر سپرهٔ ناکام، یک کلاینتِ کاملِ arti بود که نمی‌مُرد.
//!
//! پس روشِ تور را عوض می‌کنیم، نه تنظیماتش را: همان چیزی که مرورگر تور
//! اجرا می‌کند — `tor.exe` رسمی — به‌عنوان یک پروسهٔ فرزند، با یک torrc، و
//! پیشرفتش از پورتِ کنترلِ خودش خوانده می‌شود.
//!
//! روش، خط‌به‌خط از پروژهٔ WhiteAesther 1.9.4 (`src-tauri/src/tor.rs`) گرفته
//! شده؛ پروژه‌ای که هستهٔ Aether 2.0.0 را دارد و کانکشنِ تورش روی ویندوز کار
//! می‌کند. نکته‌هایی که آن‌ها با اندازه‌گیری به‌دست آورده‌اند و اینجا عیناً
//! حفظ شده‌اند، چون هرکدام یک باگِ ساعت‌خور است:
//!
//! * **مسیرِ lyrebird در `ClientTransportPlugin` نقل‌قول نمی‌شود.** تور برای
//!   گزینه‌های خودش نقل‌قول را برمی‌دارد ولی آرگومانِ `exec` را همان‌طور به
//!   CreateProcess می‌دهد؛ نقل‌قول‌شده، نتیجه‌اش
//!   `CreateProcessA() failed: The system cannot find the file specified`
//!   است و بعد هر پل با «there is no configured transport called obfs4»
//!   می‌افتد. اندازه‌گیری‌شده که بی‌نقل‌قول، حتی با فاصله در مسیر
//!   (`C:\Program Files`)، کار می‌کند.
//! * **بقیهٔ مسیرها نقل‌قول می‌شوند و بک‌اسلش‌هایشان دوبرابر.** تور در یک
//!   مقدارِ نقل‌قول‌شده بک‌اسلش را escape می‌خواند، پس `C:\Users\…` به مسیری
//!   با کاراکترِ کنترلی تبدیل می‌شود و تور فایل را رد می‌کند.
//! * **`SocksPort auto` و `ControlPort auto`.** پورتِ ثابت روی ماشینی که ما
//!   کنترلش نمی‌کنیم می‌تواند گرفته باشد، و آن خطا شبیهِ «تور بالا نمی‌آید»
//!   دیده می‌شود. پورتِ واقعی از خودِ تور پرسیده می‌شود.
//! * **`__OwningControllerProcess`.** بی‌آن، تور پس از مرگِ برنامه زنده
//!   می‌ماند و یک تونلِ بی‌مصرف باقی می‌گذارد.
//! * **گیتِ «وصل شد» فقط `PROGRESS=100` است.** تور پورتِ SOCKS را همان لحظهٔ
//!   شروع باز می‌کند، مدت‌ها پیش از داشتنِ مدار؛ پورتِ بازِ بی‌مدار اتصال را
//!   می‌پذیرد و بعد رویش می‌نشیند. این دقیقاً همان اشتباهی است که
//!   `tor_bootstrap.rs` برای مسیرِ arti مستند کرده است.
//! * **هر دو جریانِ خروجی خوانده می‌شوند**، وگرنه تور با پرشدنِ لوله روی
//!   لاگِ خودش قفل می‌کند — و همان لاگ تنها روایتِ خرابی‌هایی است که پیش از
//!   بالاآمدنِ پورتِ کنترل رخ می‌دهند.
//!
//! # آنچه عوض شده، و دلیلش
//!
//! * جای `AppHandle` تائوری، مسیرها مستقیم گرفته می‌شوند. این ماژول هیچ
//!   وابستگیِ تائوری ندارد تا همین کدی که روی ویندوز می‌رود، در آزمونِ
//!   میزبان هم ساخته و اجرا شود.
//! * سطرهای لاگ به یک کال‌بک می‌روند تا `log.rs` خودمان همان مسیرِ همیشگی را
//!   داشته باشد.
//! * فهرستِ پل‌های داخلی از `pt_config.json` خوانده می‌شود، نه از فهرستی که
//!   ما نگه می‌داریم. دلیلش را WhiteAesther نوشته و لاگِ خودمان تأییدش کرد:
//!   فهرستِ دستی می‌پوسد.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, ErrorKind, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU16, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// مهلتِ ساختِ مدار.
///
/// دست‌ودل‌بازانه و به‌عمد: پلی که باید از راهِ یک ترابرِ افزودنی گرفته شود
/// معمولاً چند ده ثانیه می‌برد. کوتاه‌کردنش یعنی اعلامِ شکست روی شبکه‌ای که
/// تور در آن موفق می‌شد.
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(180);

/// فاصلهٔ پرسیدنِ «تا کجا رسیدی؟».
const BOOTSTRAP_POLL: Duration = Duration::from_millis(500);

/// مهلتِ نوشتنِ فایلِ پورتِ کنترل و کوکی توسطِ تور.
const CONTROL_FILE_TIMEOUT: Duration = Duration::from_secs(20);

/// مهلتِ برقراریِ خودِ اتصالِ TCP به پورتِ کنترل.
const CONTROL_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// یک برشِ خواندن. با پرشدنش شکستی اعلام نمی‌شود؛ فقط دوباره تلاش می‌شود.
///
/// # چرا این عدد کوچک است و مهلتِ واقعی جای دیگری است
///
/// لاگِ ۱۷ سپتامبر (`loge2.txt`) سه اجرای پشت‌سرهم را نشان می‌دهد که همه‌شان
/// دقیقاً ۱۰.۳ ثانیه پس از شروع مردند، با یک خطای واحد:
///
/// ```text
///   08:15:30.803 [notice] Opened Control listener connection (ready) on 127.0.0.1:11533
///   08:15:33.000 [notice] Bootstrapped 0% (starting): Starting
///   ← هفت ثانیه سکوتِ کاملِ تور →
///   08:15:40.000 [notice] Starting with guard context "bridges"
///   E/tor: tor did not come up: خواندنِ پورتِ کنترلِ تور نشد: … (os error 10060)
/// ```
///
/// پورتِ کنترل باز شده بود و اتصال هم برقرار شد (وگرنه خطا «در دسترس نبود»
/// می‌شد، نه os error 10060 که مهلتِ *خواندن* است). چیزی که رخ داد این است:
/// تور در ثانیه‌های اولِ راه‌اندازی حلقهٔ رویدادش را چند ثانیه رها نمی‌کند —
/// همان هفت ثانیه سکوت در لاگِ خودش — و در آن فاصله به فرمانِ ما جواب نمی‌دهد.
/// مهلتِ ۱۰ ثانیه‌ایِ قبلی درست وسطِ آن سکوت می‌افتاد و کلِ تلاش را می‌کشت،
/// روی توری که هنوز داشت بالا می‌آمد. پس مهلتِ خواندن دیگر ابزارِ تصمیم نیست:
/// هر برش کوتاه است، و تصمیم را [`CONTROL_HANDSHAKE_TIMEOUT`] و بودجهٔ خودِ
/// نشست می‌گیرند.
const CONTROL_READ_SLICE: Duration = Duration::from_secs(2);

/// فاصلهٔ دو تلاشِ گرفتنِ نشستِ کنترل.
const CONTROL_RETRY: Duration = Duration::from_millis(500);

/// چقدر برای *برقرارشدنِ* نشستِ کنترل صبر می‌شود.
///
/// از سکوتِ اندازه‌گیری‌شدهٔ بالا (~۷ ثانیه روی این ماشین) دست‌ودل‌بازانه
/// بزرگ‌تر گرفته شده. نرسیدن به آن دیگر شکست نیست: مسیرِ دومِ
/// [`Inner::log_progress`] از لاگِ خودِ تور خوانده می‌شود.
const CONTROL_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(90);

/// مهلتِ یک پرسشِ ساده از پورتِ کنترل، وقتی نشست قبلاً برقرار شده.
const CONTROL_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// یک موجِ ترابر چقدر وقت می‌گیرد، وقتی موجِ بعدی هم در صف است.
const WAVE_BUDGET: Duration = Duration::from_secs(150);

/// بی‌پیشرفتیِ چنددقیقه‌ای یعنی این ترابر روی این شبکه راه نمی‌دهد.
///
/// معیار «بی‌پیشرفت بودن» است نه سپری‌شدنِ زمان: درصدی که بالا می‌رود ساعتِ
/// این مهلت را از صفر می‌کند. و فقط وقتی به‌کار می‌رود که موجِ بعدی وجود
/// داشته باشد — روی آخرین موج، بودجهٔ نشست تنها حرفِ آخر است.
const WAVE_STALL: Duration = Duration::from_secs(45);

#[cfg(windows)]
pub const TOR_FILENAME: &str = "tor.exe";
#[cfg(not(windows))]
pub const TOR_FILENAME: &str = "tor";

#[cfg(windows)]
pub const LYREBIRD_FILENAME: &str = "lyrebird.exe";
#[cfg(not(windows))]
pub const LYREBIRD_FILENAME: &str = "lyrebird";

#[cfg(windows)]
pub const SNOWFLAKE_FILENAME: &str = "snowflake-client.exe";
#[cfg(not(windows))]
pub const SNOWFLAKE_FILENAME: &str = "snowflake-client";

#[cfg(windows)]
pub const CONJURE_FILENAME: &str = "conjure-client.exe";
#[cfg(not(windows))]
pub const CONJURE_FILENAME: &str = "conjure-client";

/// چند سطر از خروجیِ خودِ تور برای گزارشِ عیب‌یابی نگه داشته شود.
const MAX_LINES: usize = 400;

/// کدام پل‌ها، اگر اصلاً.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BridgeMode {
    /// مستقیم به شبکهٔ تور. جایی که تور بسته نیست، سریع‌ترین راه است.
    #[default]
    None,
    /// همان فهرستی که Tor در `pt_config.json` کنارِ باینری می‌فرستد.
    BuiltIn,
    /// سطرهایی که کاربر خودش چسبانده است.
    Custom,
}

/// آنچه برای یک اجرا لازم است.
#[derive(Debug, Clone)]
pub struct TorLaunch {
    /// مسیرِ `tor.exe`.
    pub binary: PathBuf,
    /// پوشه‌ای که `lyrebird.exe`، `geoip`، `geoip6` و `pt_config.json` در آن است.
    pub support: PathBuf,
    /// `DataDirectory` تور.
    pub home: PathBuf,
    pub bridges: BridgeMode,
    /// از فهرستِ داخلی کدام ترابر: `obfs4`، `snowflake` یا `meek`.
    pub transport: String,
    /// سطرهای دستیِ کاربر، هر سطر یکی.
    pub custom_bridges: String,
    /// هُپِ قبلی، اگر تور دومین حلقهٔ زنجیر است.
    pub upstream: Option<SocketAddr>,
    /// پورتِ SOCKS. `None` یعنی خودِ تور یکی بردارد.
    ///
    /// WhiteAesther اینجا به‌عمد `auto` می‌گذارد، چون پورتِ ثابت می‌تواند روی
    /// ماشینِ کاربر گرفته باشد. برنامهٔ ما اما قراردادِ دیگری دارد: کاربر
    /// مرورگرش را به `127.0.0.1:1819` وصل می‌کند و همهٔ مسیرهای دیگر —
    /// پراکسیِ سیستم، TUN، اشتراکِ شبکه — همان عدد را می‌شناسند. پس برای
    /// حالتِ «تور تنها» پورت را ثابت می‌دهیم و اگر گرفته بود، خطا را
    /// سربسته نمی‌گذاریم.
    pub socks: Option<u16>,
    /// کلِ وقتی که این نشست برای رسیدن به مدار دارد.
    ///
    /// `None` یعنی [`BOOTSTRAP_TIMEOUT`]. برنامه اما بودجهٔ خودش را دارد و در
    /// لاگ هم اعلامش می‌کند (`timeout=600s`)؛ پیش از این آن عدد اعلام می‌شد و
    /// اینجا ۱۸۰ ثانیه اعمال می‌شد — یعنی لاگ چیزی می‌گفت که کد نمی‌کرد.
    pub budget: Option<Duration>,
}

impl TorLaunch {
    pub fn validate(&self) -> Result<(), String> {
        if !self.binary.is_file() {
            return Err("این نصب فایلِ tor را ندارد".into());
        }
        match self.bridges {
            BridgeMode::None | BridgeMode::BuiltIn => Ok(()),
            BridgeMode::Custom => {
                if self.bridge_lines().is_empty() {
                    Err("حداقل یک سطرِ پل بچسبان، یا پل‌های داخلی را انتخاب کن".into())
                } else {
                    Ok(())
                }
            }
        }
    }

    /// سطرهای چسبانده‌شده، بی خطِ خالی و بی توضیح.
    ///
    /// پیشوندِ `Bridge ` برداشته می‌شود: صفحه‌ای که مردم از آن کپی می‌کنند
    /// همین کلمه را هم می‌نویسد و torrc دقیقاً یک‌بار می‌خواهدش؛ وگرنه
    /// `Bridge Bridge obfs4 …` می‌شود و تور کلِ فایل را با خطای تجزیه رد
    /// می‌کند — روی سطری که کاربر ننوشته است.
    fn bridge_lines(&self) -> Vec<String> {
        self.custom_bridges
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                line.strip_prefix("Bridge ")
                    .or_else(|| line.strip_prefix("bridge "))
                    .unwrap_or(line)
                    .trim()
                    .to_string()
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorSnapshot {
    /// `idle`، `connecting`، `connected`، `error`.
    pub state: String,
    pub pid: Option<u32>,
    pub socks_port: Option<u16>,
    /// پیشرفتِ bootstrap، ۰ تا ۱۰۰.
    pub bootstrap: u8,
    /// تور با زبانِ خودش می‌گوید الان چه می‌کند.
    pub bootstrap_summary: Option<String>,
    /// `ip:port`ِ اولین هاپ (پل)، وقتی پورتِ کنترل آن را داده باشد.
    pub first_hop: Option<String>,
    pub last_error: Option<String>,
}

impl Default for TorSnapshot {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            pid: None,
            socks_port: None,
            bootstrap: 0,
            bootstrap_summary: None,
            first_hop: None,
            last_error: None,
        }
    }
}

type LineSink = Arc<dyn Fn(&str) + Send + Sync>;

/// فازِ حامل. عددی است چون در یک `AtomicU8` می‌نشیند و از چند ترد خوانده
/// می‌شود؛ سه حالت بیشتر ندارد و enum برایش هزینهٔ اضافه بود.
const PHASE_IDLE: u8 = 0;
const PHASE_STARTING: u8 = 1;
const PHASE_RUNNING: u8 = 2;

struct Inner {
    /// فازِ راه‌اندازی؛ ببینید `is_alive`.
    phase: AtomicU8,
    child: Mutex<Option<Child>>,
    snapshot: Mutex<TorSnapshot>,
    lines: Mutex<VecDeque<String>>,
    generation: AtomicU64,
    /// درصدی که از *لاگِ خودِ تور* درآمده، مستقل از پورتِ کنترل.
    ///
    /// مسیرِ دوم، و دلیلِ وجودش لاگِ ۱۷ سپتامبر است: وقتی پورتِ کنترل چند
    /// ثانیه جواب نمی‌دهد، همان تور در stdout خودش `Bootstrapped N%` را
    /// می‌نویسد. یک نشست نباید به یک کاناله بند باشد وقتی کانالِ دوم مجانی
    /// جلوی چشم است.
    log_progress: AtomicU8,
    /// پورتی که تور در لاگش گفت لیسنرِ SOCKS را روی آن باز کرده. صفر یعنی
    /// هنوز نگفته.
    log_socks: AtomicU16,
    // >>> AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network
    /// مسیرِ فایلِ پورتِ کنترل و کوکیِ این اجرا — تا پس از bootstrap هم
    /// بتوان فرمانی به تور داد. پیش‌تر هر دو فقط متغیرِ محلیِ `start`
    /// بودند و با پایانِ راه‌اندازی از دست می‌رفتند.
    control: Mutex<Option<(PathBuf, PathBuf)>>,
    // <<< AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network
    sink: LineSink,
}

#[derive(Clone)]
pub struct TorNative {
    inner: Arc<Inner>,
}

impl TorNative {
    pub fn new(sink: LineSink) -> Self {
        Self {
            inner: Arc::new(Inner {
                phase: AtomicU8::new(PHASE_IDLE),
                child: Mutex::new(None),
                snapshot: Mutex::new(TorSnapshot::default()),
                lines: Mutex::new(VecDeque::with_capacity(MAX_LINES)),
                generation: AtomicU64::new(0),
                log_progress: AtomicU8::new(0),
                log_socks: AtomicU16::new(0),
                // >>> AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network
                control: Mutex::new(None),
                // <<< AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network
                sink,
            }),
        }
    }

    pub fn snapshot(&self) -> TorSnapshot {
        self.inner.snapshot.lock().clone()
    }

    pub fn lines(&self) -> Vec<String> {
        self.inner.lines.lock().iter().cloned().collect()
    }

    /// آیا فرزند هنوز زنده است. یک پروسهٔ مرده، یک snapshotِ سرحال جا
    /// می‌گذارد؛ پس این پرسش جدا از snapshot پاسخ داده می‌شود.
    /// آیا این حامل زنده است؟
    ///
    /// «هنوز شروع نشده» با «مرده» یکی نیست. راه‌اندازی روی یک تردِ دیگر است و
    /// tor.exe چند ده میلی‌ثانیه بعد از ساخته‌شدنِ آن ترد اجرا می‌شود؛ در آن
    /// فاصله ماشینِ حالت `false` می‌گرفت و تلاش را در همان تیکِ اول جمع می‌کرد
    /// (لاگِ ۱۶ سپتامبر: ۲۰۰ms پس از شروع، «Engine exited before it opened the
    /// SOCKS5 port»). پس فاز صریح است، و Starting زنده است.
    pub fn is_alive(&self) -> bool {
        match self.inner.phase.load(Ordering::SeqCst) {
            PHASE_STARTING => true,
            PHASE_RUNNING => {
                let mut guard = self.inner.child.lock();
                match guard.as_mut() {
                    Some(child) => !matches!(child.try_wait(), Ok(Some(_))),
                    // در حالِ اجرا و بی‌فرزند نمی‌شود؛ اگر شد، مرده است.
                    None => false,
                }
            }
            _ => false,
        }
    }

    /// «راه‌اندازی شکست خورد» — همهٔ مسیرهای خطای `start` از یک جا اعلام
    /// می‌شوند: تردِ فراخوان، وقتی `start` خطا برگرداند. اینکه هر `?` خودش
    /// فاز را بگذارد، همان چیزی است که یکی‌اش فراموش می‌شود.
    pub fn mark_dead(&self) {
        self.inner.phase.store(PHASE_IDLE, Ordering::SeqCst);
    }

    /// «از این لحظه در حالِ راه‌اندازی‌ام» — پیش از سپردنِ `start` به یک ترد
    /// صدا زده می‌شود، وگرنه همان مسابقهٔ بالا برمی‌گردد.
    pub fn mark_starting(&self) {
        self.inner.phase.store(PHASE_STARTING, Ordering::SeqCst);
    }

    /// تور را راه می‌اندازد و منتظرِ مدار می‌ماند.
    ///
    /// # موج‌های ترابر
    ///
    /// با پل‌های داخلی، یک نوبتِ obfs4 تمامِ شانسِ کاربر نیست. مسیرِ arti همین نردبان را
    /// داشت (`obfs4 → snowflake → meek`)؛ با کوچ به `tor.exe` گم شد و لاگِ ۱۷ سپتامبر
    /// دقیقاً همین را نشان می‌دهد: `Plan ready (1 attempt)`. در شبکه‌ای که obfs4ِ عمومیِ
    /// توکار را شمارده و بسته، دو ترابرِ domain-fronted تنها شانسِ باقی‌مانده‌اند و باید
    /// نوبت بگیرند.
    pub fn start(&self, launch: &TorLaunch) -> Result<SocketAddr, String> {
        // `stop` فاز را به Idle می‌برد، پس Starting بعد از آن دوباره گذاشته
        // می‌شود — وگرنه پنجرهٔ مسابقه فقط جابه‌جا می‌شود.
        launch.validate()?;
        self.stop();
        self.inner.phase.store(PHASE_STARTING, Ordering::SeqCst);

        let overall = Instant::now() + launch.budget.unwrap_or(BOOTSTRAP_TIMEOUT);
        let waves = plan_waves(launch);
        let last = waves.len().saturating_sub(1);
        let mut failures: Vec<String> = Vec::new();

        for (index, transport) in waves.iter().enumerate() {
            let left = overall.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let slice = if index == last {
                left
            } else {
                left.min(WAVE_BUDGET)
            };
            // موجِ آخر با کلِ بودجهٔ نشست کار می‌کند و معیارِ گیرکردن ندارد: جایی
            // برای رفتن نیست، پس کوتاه‌کردنش فقط شانس را می‌سوزاند.
            let stall = if index == last {
                None
            } else {
                Some(WAVE_STALL)
            };
            let label = wave_label(transport);
            if waves.len() > 1 {
                (self.inner.sink)(&format!(
                    "[*] tor bridge wave {}/{}: {label} ({}s of the {}s left)",
                    index + 1,
                    waves.len(),
                    slice.as_secs(),
                    left.as_secs()
                ));
            }
            match self.run_once(launch, transport, slice, stall) {
                Ok(address) => {
                    (self.inner.sink)(&format!(
                        "[+] tor has a circuit via {label}; SOCKS listener on {address}"
                    ));
                    return Ok(address);
                }
                Err(error) => {
                    failures.push(format!("{label}: {error}"));
                    if index != last {
                        (self.inner.sink)(&format!(
                            "[-] {label} did not get there ({error}); trying the next transport"
                        ));
                    }
                    // بینِ دو موج، فرزند می‌میرد ولی حامل زنده می‌ماند: `stop` فاز را
                    // Idle می‌کند و ماشینِ حالت در همان تیک کلِ تلاش را جمع می‌کند.
                    self.kill_child_keep_phase();
                }
            }
        }

        self.stop();
        let error = if failures.is_empty() {
            "بودجهٔ این نشست پیش از اولین تلاش تمام شد".to_string()
        } else {
            failures.join(" | ")
        };
        let mut snapshot = self.inner.snapshot.lock();
        snapshot.state = "error".into();
        snapshot.last_error = Some(error.clone());
        Err(error)
    }

    /// یک نوبت: یک `tor.exe` با یک ترابر، تا مدار یا تا پایانِ برشِ زمانی‌اش.
    fn run_once(
        &self,
        launch: &TorLaunch,
        transport: &str,
        budget: Duration,
        stall: Option<Duration>,
    ) -> Result<SocketAddr, String> {
        let inner = &self.inner;
        let generation = inner.generation.fetch_add(1, Ordering::SeqCst) + 1;

        // تور اگر پوشهٔ دادهٔ خود را برای گروه یا همه خواندنی ببیند بالا
        // نمی‌آید؛ این بررسیِ خودِ تور است و دورزدنش کارِ ما نیست.
        std::fs::create_dir_all(&launch.home)
            .map_err(|error| format!("پوشهٔ تور آماده نشد: {error}"))?;

        let control_port_file = launch.home.join("control-port");
        let cookie_file = launch.home.join("control-auth-cookie");
        // فایلِ ماندهٔ اجرای قبلی، پورتی را نشان می‌دهد که دیگر توری پشتش
        // نیست، و به‌عنوانِ فایلِ همین اجرا خوانده می‌شود.
        let _ = std::fs::remove_file(&control_port_file);
        let _ = std::fs::remove_file(&cookie_file);

        let torrc = launch.home.join("torrc");
        // >>> AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network
        // همین دو مسیر پس از اتصال هم لازم‌اند — رجوع به
        // [`TorNative::new_circuit`].
        *inner.control.lock() = Some((control_port_file.clone(), cookie_file.clone()));
        // <<< AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network
        let rendered = render_torrc(launch, transport, &control_port_file, &cookie_file)?;
        std::fs::write(&torrc, &rendered)
            .map_err(|error| format!("نوشتنِ تنظیماتِ تور نشد: {error}"))?;

        {
            let mut snapshot = inner.snapshot.lock();
            *snapshot = TorSnapshot {
                state: "connecting".into(),
                bootstrap_summary: Some("Starting Tor".into()),
                ..TorSnapshot::default()
            };
        }
        // درصدِ موجِ قبلی نباید موجِ تازه را «در حالِ پیشرفت» نشان دهد.
        inner.log_progress.store(0, Ordering::SeqCst);
        inner.log_socks.store(0, Ordering::SeqCst);

        let mut command = Command::new(&launch.binary);
        command
            .arg("-f")
            .arg(&torrc)
            .current_dir(&launch.home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut command);

        let mut child = command
            .spawn()
            .map_err(|error| format!("اجرای تور نشد: {error}"))?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        {
            let mut guard = inner.child.lock();
            if inner.generation.load(Ordering::SeqCst) != generation {
                let _ = child.kill();
                let _ = child.wait();
                return Err("تور در میانهٔ راه‌اندازی متوقف شد".into());
            }
            *guard = Some(child);
        }
        // از این‌جا به بعد زنده‌بودن را خودِ فرآیند جواب می‌دهد.
        inner.phase.store(PHASE_RUNNING, Ordering::SeqCst);
        inner.snapshot.lock().pid = Some(pid);

        // هر دو جریان خوانده می‌شوند، وگرنه تور با پرشدنِ لوله روی لاگِ خودش
        // قفل می‌کند — و همان لاگ تنها روایتِ خرابی‌های پیش از بالاآمدنِ
        // پورتِ کنترل است.
        for reader in [
            stdout.map(|out| Box::new(out) as Box<dyn Read + Send>),
            stderr.map(|err| Box::new(err) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            spawn_log_reader(inner.clone(), reader, generation);
        }

        // سطرهای پلِ همین موج، برای ترجمهٔ اثرِ انگشتِ اولین هاپ به `ip:port`.
        let offered = bridge_lines(launch, transport).unwrap_or_default();

        self.wait_for_circuit(
            &control_port_file,
            &cookie_file,
            generation,
            Instant::now() + budget,
            stall,
            launch.socks,
            &offered,
            transport,
        )
    }

    /// منتزرِ `PROGRESS=100` می‌ماند و در راه می‌گوید تا کجا رسیده.
    ///
    /// # چرا دو منبع دارد
    ///
    /// پورتِ کنترل دقیق است (درصد و خلاصهٔ متنی)، ولی تنها منبع نیست: تور همان درصد را
    /// در stdout خودش هم می‌نویسد. لاگِ ۱۷ سپتامبر نشان داد وابستگیِ تک‌کاناله چه
    /// هزینه‌ای دارد — یک مهلتِ خواندنِ ۱۰ ثانیه‌ای روی پورتِ کنترل، در همان هفت ثانیه‌ای
    /// که تور به هیچ فرمانی جواب نمی‌دهد، کلِ نشست را می‌کشت. حالا از دست رفتنِ پورتِ
    /// کنترل فقط یک سطرِ هشدار است و کار با لاگ ادامه می‌یابد.
    fn wait_for_circuit(
        &self,
        control_port_file: &Path,
        cookie_file: &Path,
        generation: u64,
        deadline: Instant,
        stall: Option<Duration>,
        fallback_socks: Option<u16>,
        offered: &[String],
        transport: &str,
    ) -> Result<SocketAddr, String> {
        let inner = &self.inner;
        let mut session = self.open_control(control_port_file, cookie_file, generation, deadline);

        let mut told = 0u8;
        let mut best = 0u8;
        let mut headway = Instant::now();

        while Instant::now() < deadline {
            if inner.generation.load(Ordering::SeqCst) != generation {
                return Err("تور در میانهٔ راه‌اندازی متوقف شد".into());
            }
            // توری که پیش از bootstrap مرده، سوکتِ کنترل را بسته می‌گذارد؛
            // دیدنش اینجا یک هنگ را به یک پیام تبدیل می‌کند.
            if let Some(child) = inner.child.lock().as_mut() {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return Err("تور پیش از ساختِ مدار متوقف شد".into());
                }
            } else {
                return Err("تور در میانهٔ راه‌اندازی متوقف شد".into());
            }

            // درصد: هر چه لاگ گفته، و اگر پورتِ کنترل جواب می‌دهد هر چه آن می‌گوید.
            let mut progress = inner.log_progress.load(Ordering::SeqCst);
            let mut summary = None;
            // پرچم، نه انتساب در همان بازو: نشستی که هم‌زمان قرض داده شده و کنار
            // گذاشته می‌شود، همان چیزی است که قرض‌گیر را ناخوش می‌کند.
            let mut lost_control = false;
            if let Some(open) = session.as_mut() {
                match open.get_info("status/bootstrap-phase", CONTROL_REPLY_TIMEOUT) {
                    Ok(phase) => {
                        progress = progress.max(parse_progress(&phase).unwrap_or(0));
                        summary = parse_summary(&phase);
                    }
                    Err(error) => {
                        (inner.sink)(&format!(
                            "[-] tor's control port stopped answering ({error}); \
                             following its own log instead"
                        ));
                        lost_control = true;
                    }
                }
            }
            if lost_control {
                session = None;
            }
            {
                let mut snapshot = inner.snapshot.lock();
                snapshot.bootstrap = progress;
                if summary.is_some() {
                    snapshot.bootstrap_summary = summary.clone();
                }
            }
            if progress != told {
                told = progress;
                (inner.sink)(&format!(
                    "[*] tor reaching the network: {progress}%{}",
                    summary
                        .as_deref()
                        .map(|text| format!(": {text}"))
                        .unwrap_or_default()
                ));
            }
            if progress > best {
                best = progress;
                headway = Instant::now();
            }

            if progress >= 100 {
                // پورت: اول از خودِ تور پرسیده می‌شود، و اگر پورتِ کنترل نیست از
                // لاگِ خودش و در نهایت از قراردادِ خودِ برنامه.
                let port = session
                    .as_mut()
                    .and_then(|open| open.socks_port(CONTROL_REPLY_TIMEOUT).ok())
                    .or_else(|| match inner.log_socks.load(Ordering::SeqCst) {
                        0 => None,
                        port => Some(port),
                    })
                    .or(fallback_socks)
                    .ok_or("تور مدار ساخت ولی هیچ لیسنرِ SOCKSی اعلام نکرد")?;
                // اندپوینتِ واقعی: اولین هاپِ مدار، از پورتِ کنترل پرسیده و با
                // سطرهای پلِ همین موج به `ip:port` ترجمه‌شده. کارتِ اتصال تا
                // امروز اینجا IPِ *خروجی* و نامِ کشور را نشان می‌داد — یعنی
                // همان چیزی که ردیفِ بالایش هم می‌گفت.
                let hop = session
                    .as_mut()
                    .and_then(|open| open.first_hop(CONTROL_REPLY_TIMEOUT))
                    .and_then(|fingerprint| bridge_endpoint_of(offered, &fingerprint));
                if let Some(addr) = &hop {
                    let via = if transport.is_empty() {
                        "direct"
                    } else {
                        transport
                    };
                    (inner.sink)(&format!("[+] tor first hop: {addr} via {via}"));
                }

                let mut snapshot = inner.snapshot.lock();
                snapshot.socks_port = Some(port);
                snapshot.state = "connected".into();
                snapshot.bootstrap = 100;
                snapshot.first_hop = hop;
                return Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
            }

            if let Some(limit) = stall {
                if headway.elapsed() >= limit {
                    return Err(format!(
                        "تا {best}% رسید و {}s بی‌پیشرفت ماند",
                        limit.as_secs()
                    ));
                }
            }
            thread::sleep(BOOTSTRAP_POLL);
        }

        Err(format!("در مهلتِ این نوبت مدار نساخت؛ تا {best}% رسید"))
    }

    /// نشستِ کنترل، اگر به دست بیاید. نیامدنش شکست نیست.
    // >>> AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network
    /// از تور یک مدارِ تازه می‌خواهد — `SIGNAL NEWNYM`.
    ///
    /// # چرا این کار لازم است
    ///
    /// در لاگِ میدانی، نشستِ توری که کامل bootstrap شده بود و ترافیک را
    /// می‌برد، ۳۹۷ms و در نشستی دیگر ۲۱۱۸ms تأخیر نشان می‌داد. هیچ‌کدام
    /// خرابیِ اندازه‌گیری نبود ([`crate::ping`] از ۱.۲.۵ روی یک نشستِ گرم و با
    /// یک round trip می‌سنجد): مدارِ تور از سه رله‌ای ساخته می‌شود که تصادفی
    /// انتخاب شده‌اند، و یکی از آن سه می‌تواند آن سرِ دنیا و اشباع باشد. عددِ
    /// بد، *خبرِ همان مدار* است، نه خبرِ شبکه — و تور خودش راهِ عوض‌کردنش را
    /// دارد.
    ///
    /// `NEWNYM` مدارهای موجود را نمی‌بندد؛ فقط استریم‌های تازه را روی مدارِ
    /// تازه می‌فرستد. پس صداکننده باید نشستِ گرمِ پینگ را هم بیندازد
    /// (`ping::reset`)، وگرنه همان مدارِ قدیم را اندازه می‌گیرد و «تغییری
    /// نکرد» نتیجه می‌دهد.
    ///
    /// «بهترینِ سه مدار» شدنی نیست و ادعا هم نمی‌شود: تور راهی برای برگشتن به
    /// مدارِ قبلی نمی‌دهد. کاری که می‌شود کرد این است که از مدارِ *بد* عبور
    /// کنیم و روی اولین مدارِ قابل‌قبول بایستیم.
    pub fn new_circuit(&self, budget: Duration) -> Result<(), String> {
        let (control_file, cookie_file) = self
            .inner
            .control
            .lock()
            .clone()
            .ok_or_else(|| "این نشستِ تور پورتِ کنترل ندارد".to_string())?;
        if !self.is_alive() {
            return Err("تور زنده نیست".into());
        }
        let generation = self.inner.generation.load(Ordering::SeqCst);
        let deadline = Instant::now() + budget;
        let mut session = self
            .open_control(&control_file, &cookie_file, generation, deadline)
            .ok_or_else(|| "پورتِ کنترلِ تور جواب نداد".to_string())?;
        let reply = session.command("SIGNAL NEWNYM", deadline)?;
        if !reply.starts_with("250") {
            return Err(format!("تور NEWNYM را رد کرد: {reply}"));
        }
        Ok(())
    }
    // <<< AETHER-APP-PATCH a-bad-circuit-is-not-a-bad-network

    fn open_control(
        &self,
        control_port_file: &Path,
        cookie_file: &Path,
        generation: u64,
        deadline: Instant,
    ) -> Option<ControlSession> {
        let inner = &self.inner;
        let handshake = deadline.min(Instant::now() + CONTROL_HANDSHAKE_TIMEOUT);
        let control = match read_control_port(control_port_file, CONTROL_FILE_TIMEOUT) {
            Ok(address) => address,
            Err(error) => {
                (inner.sink)(&format!(
                    "[-] {error}; following tor's own log for progress instead"
                ));
                return None;
            }
        };
        let cookie = match read_cookie(cookie_file, CONTROL_FILE_TIMEOUT) {
            Ok(cookie) => cookie,
            Err(error) => {
                (inner.sink)(&format!(
                    "[-] {error}; following tor's own log for progress instead"
                ));
                return None;
            }
        };
        let alive = || {
            inner.generation.load(Ordering::SeqCst) == generation
                && match inner.child.lock().as_mut() {
                    Some(child) => !matches!(child.try_wait(), Ok(Some(_))),
                    None => false,
                }
        };
        match ControlSession::open(control, &cookie, handshake, &alive) {
            Ok(session) => Some(session),
            Err(error) => {
                (inner.sink)(&format!(
                    "[-] tor's control port did not answer in time ({error}); \
                     following its own log for progress instead"
                ));
                None
            }
        }
    }

    /// فرزند را می‌کشد ولی حامل را زنده نگه می‌دارد — برای فاصلهٔ دو موج.
    ///
    /// ترتیب مهم است: اول فاز به Starting می‌رود، بعد فرزند کشته می‌شود. عکسش یک
    /// پنجرهٔ چندمیلی‌ثانیه‌ای می‌سازد که در آن حامل «مرده» دیده می‌شود و ماشینِ حالت
    /// کلِ تلاش را جمع می‌کند.
    fn kill_child_keep_phase(&self) {
        let inner = &self.inner;
        inner.phase.store(PHASE_STARTING, Ordering::SeqCst);
        inner.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut child) = inner.child.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn stop(&self) {
        let inner = &self.inner;
        inner.phase.store(PHASE_IDLE, Ordering::SeqCst);
        inner.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut child) = inner.child.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *inner.snapshot.lock() = TorSnapshot::default();
    }
}

impl Drop for TorNative {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.stop();
        }
    }
}

fn spawn_log_reader(inner: Arc<Inner>, reader: Box<dyn Read + Send>, generation: u64) {
    thread::spawn(move || {
        let mut lines = BufReader::new(reader).lines();
        while let Some(Ok(line)) = lines.next() {
            if inner.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            // مسیرِ دوم: هر چه تور دربارهٔ پیشرفت و لیسنرش می‌گوید، پیش از آنکه
            // پورتِ کنترل حرفی زده باشد.
            if let Some(progress) = parse_log_progress(&line) {
                inner.log_progress.fetch_max(progress, Ordering::SeqCst);
            }
            if let Some(port) = parse_log_socks(&line) {
                inner.log_socks.store(port, Ordering::SeqCst);
            }
            {
                let mut kept = inner.lines.lock();
                if kept.len() == MAX_LINES {
                    kept.pop_front();
                }
                kept.push_back(line.clone());
            }
            (inner.sink)(&format!("[tor] {line}"));
        }
    });
}

/// `… [notice] Bootstrapped 45% (requesting_descriptors): …` → `Some(45)`
fn parse_log_progress(line: &str) -> Option<u8> {
    let after = line.split("Bootstrapped ").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `… [notice] Opened Socks listener connection (ready) on 127.0.0.1:1819` → `Some(1819)`
///
/// فقط loopback پذیرفته می‌شود: هر نشانیِ دیگری در این سطر یعنی چیزی را می‌خوانیم
/// که قراردادِ ما نیست، و برداشتنِ عددِ آن یک وعدهٔ دروغ می‌سازد.
fn parse_log_socks(line: &str) -> Option<u16> {
    if !line.contains("Socks listener") {
        return None;
    }
    let address = line.rsplit(" on ").next()?.trim();
    let (host, port) = address.rsplit_once(':')?;
    if host != "127.0.0.1" {
        return None;
    }
    port.trim().parse().ok()
}

/// برچسبِ یک موج، برای لاگ.
fn wave_label(transport: &str) -> String {
    match transport.trim() {
        "" => "direct or via bridges".to_string(),
        other => other.to_string(),
    }
}

/// ترتیبِ ترابرهایی که این نشست می‌آزماید.
///
/// فقط برای پلِ داخلی و وقتی تور خودش روبروی شبکه است. پشتِ یک حاملِ دیگر
/// (`upstream`) پل از اول معنا ندارد، و وقتی کاربر خودش ترابر یا سطرِ پل داده،
/// انتخابِ او جای حدسِ ما را می‌گیرد.
fn plan_waves(launch: &TorLaunch) -> Vec<String> {
    if launch.bridges != BridgeMode::BuiltIn
        || launch.upstream.is_some()
        || !launch.transport.trim().is_empty()
    {
        return vec![launch.transport.trim().to_string()];
    }
    match built_in_waves(&launch.support) {
        Ok(waves) if !waves.is_empty() => waves,
        // فهرست خوانده نشد: همان مسیرِ قدیم، یک نوبت و پیش‌فرضِ obfs4.
        _ => vec![String::new()],
    }
}

/// ترابرهایی که همِ پل دارند و هم بایناریِ اجراکننده‌اش در این نصب هست.
///
/// داده‌محور است، نه فهرستِ دستی: نامِ کلیدها در `pt_config.json` از نسخه‌ای به
/// نسخه‌ای عوض می‌شود (`meek`، `meek-azure`، …) و روشِ واقعی همان اولین واژهٔ
/// خودِ سطرِ پل است. ترتیبِ ترجیح از لاگِ میدانی می‌آید: obfs4 ارزان‌ترین است و
/// اگر باز باشد زود جواب می‌دهد؛ دو تای بعدی domain-fronted اند و جایی به کار
/// می‌آیند که obfs4ِ توکار شمارده و بسته باشد.
fn built_in_waves(support: &Path) -> Result<Vec<String>, String> {
    const PREFERENCE: [&str; 4] = ["obfs4", "snowflake", "meek", "webtunnel"];
    let body = std::fs::read_to_string(support.join("pt_config.json"))
        .map_err(|error| format!("فهرستِ پل‌های داخلی خوانده نشد: {error}"))?;
    let config: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("فهرستِ پل‌های داخلی ناخوانا است: {error}"))?;
    let groups = config
        .get("bridges")
        .and_then(|bridges| bridges.as_object())
        .ok_or("فهرستِ پل‌های داخلی بخشِ bridges ندارد")?;
    let plugins = available_plugins(support);

    let mut usable: Vec<String> = Vec::new();
    for (key, value) in groups {
        let Some(first) = value
            .as_array()
            .and_then(|list| list.iter().find_map(|line| line.as_str()))
        else {
            continue;
        };
        let Some(method) = first.split_whitespace().next() else {
            continue;
        };
        if plugin_for(&plugins, method).is_some() {
            usable.push(key.clone());
        }
    }

    usable.sort_by_key(|key| {
        PREFERENCE
            .iter()
            .position(|wanted| key.contains(wanted))
            .unwrap_or(PREFERENCE.len())
    });
    Ok(usable)
}

/// هر بایناریِ ترابر، و روش‌هایی که واقعاً اجرا می‌کند.
///
/// # چرا این فهرست وجود دارد
///
/// پیش از این، یک سطرِ ثابت می‌گفت lyrebird همِ `meek_lite,obfs4,webtunnel,snowflake` را
/// اجرا می‌کند. lyrebird سه تای اول را می‌دهد و snowflake را نه — و ما هم در
/// `stage-tor.ps1` تنها همین یک بایناری را می‌فرستیم. نتیجهٔ آن وعده این بود که
/// انتخابِ snowflake حتماً به `No running bridges` می‌رسید، بی آنکه کسی بداند چرا.
/// اینجا فقط روشی اعلام می‌شود که بایناری‌اش روی دیسک هست.
fn available_plugins(support: &Path) -> Vec<(&'static str, PathBuf)> {
    const PROVIDERS: [(&str, &str); 3] = [
        (LYREBIRD_FILENAME, "meek_lite,obfs4,webtunnel"),
        (SNOWFLAKE_FILENAME, "snowflake"),
        (CONJURE_FILENAME, "conjure"),
    ];
    PROVIDERS
        .iter()
        .filter_map(|(file, methods)| {
            let path = support.join(file);
            path.is_file().then_some((*methods, path))
        })
        .collect()
}

/// کدام ترابر این روش را اجرا می‌کند؟
fn plugin_for<'a>(
    plugins: &'a [(&'static str, PathBuf)],
    method: &str,
) -> Option<&'a (&'static str, PathBuf)> {
    plugins
        .iter()
        .find(|(methods, _)| methods.split(',').any(|known| known == method))
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// CREATE_NO_WINDOW: وگرنه هر اجرا یک پنجرهٔ کنسول باز می‌کند.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// تنظیماتی که تور با آن شروع می‌شود.
fn render_torrc(
    launch: &TorLaunch,
    transport: &str,
    control_port_file: &Path,
    cookie_file: &Path,
) -> Result<String, String> {
    let support = launch.support.as_path();
    let mut config = String::new();
    // هر دو auto: پورتِ ثابت یک چیزِ دیگر است که می‌تواند روی ماشینِ کاربر
    // گرفته باشد، و آن خطا شبیهِ «تور بالا نمی‌آید» دیده می‌شود.
    match launch.socks {
        Some(port) => config.push_str(&format!("SocksPort 127.0.0.1:{port}\n")),
        None => config.push_str("SocksPort auto\n"),
    }
    config.push_str("ControlPort auto\n");
    config.push_str(&format!(
        "ControlPortWriteToFile {}\n",
        quote(control_port_file)
    ));
    config.push_str("CookieAuthentication 1\n");
    config.push_str(&format!("CookieAuthFile {}\n", quote(cookie_file)));
    config.push_str(&format!("DataDirectory {}\n", quote(&launch.home)));
    config.push_str(&format!("GeoIPFile {}\n", quote(&support.join("geoip"))));
    config.push_str(&format!("GeoIPv6File {}\n", quote(&support.join("geoip6"))));
    // همه‌چیز روی loopback.
    config.push_str("SocksPolicy accept 127.0.0.0/8\n");
    config.push_str("SocksPolicy reject *\n");
    // بی‌این، تور پس از مرگِ پروسه‌ای که اجرایش کرده زنده می‌ماند و روی یک
    // کرش، تونلی جا می‌گذارد که هیچ ترافیکی به آن نمی‌رود.
    config.push_str("__OwningControllerProcess ");
    config.push_str(&std::process::id().to_string());
    config.push('\n');

    // هُپِ جلویی، وقتی هست. نه نقل‌قول‌شده و نه URL: تور این یکی را
    // `host:port` خالی می‌خواهد.
    if let Some(address) = launch.upstream {
        config.push_str(&format!("Socks5Proxy {address}\n"));
    }

    // پل برای رسیدن به توری است که خودش بسته است. پشتِ یک حاملِ دیگر، آن
    // پرسش قبلاً پاسخ گرفته — هُپِ جلویی همان چیزی است که ما را بیرون برده —
    // پس تورِ زنجیره‌ای رله‌های مستقیم را می‌گیرد و ترابرها را کنار
    // می‌گذارد. WhiteAesther این ترکیب را هم به‌عمد نمی‌فرستد: تور
    // `Socks5Proxy` و `ClientTransportPlugin` را با هم می‌پذیرد، ولی هرگز
    // دیده نشده که lyrebird پلش را *از راهِ آن پراکسی* بگیرد، و ترابری که
    // آن را نادیده بگیرد سراغِ پل می‌رود؛ همان یک نشانی که روی شبکهٔ
    // سانسورشده نباید لخت گرفته شود.
    let bridges_wanted = launch.bridges != BridgeMode::None && launch.upstream.is_none();
    if bridges_wanted {
        let plugins = available_plugins(support);
        if plugins.is_empty() {
            return Err("ترابرهای افزودنی در این نصب نیستند".into());
        }
        let lines = bridge_lines(launch, transport)?;
        // روشِ هر سطر باید مجریِ زنده‌ای داشته باشد. سطری که مجری ندارد
        // در تور به `No running bridges` ترجمه می‌شود — خطایی که شبکه را متهم
        // می‌کند و تقصیرِ بسته‌بندیِ خودِ ما است. پس صریح گفته می‌شود.
        for line in &lines {
            let method = line.split_whitespace().next().unwrap_or_default();
            if plugin_for(&plugins, method).is_none() {
                return Err(format!(
                    "ترابرِ {method} در این نصب نیست؛ پلی از نوعِ دیگر انتخاب کن"
                ));
            }
        }
        // این مسیرها نقل‌قول نمی‌شوند. دلیلش در سرِ همین فایل است.
        for (methods, path) in &plugins {
            config.push_str(&format!(
                "ClientTransportPlugin {methods} exec {}\n",
                path.to_string_lossy()
            ));
        }
        config.push_str("UseBridges 1\n");
        for line in lines {
            config.push_str(&format!("Bridge {line}\n"));
        }
    }
    Ok(config)
}

/// پل‌هایی که در torrc نوشته می‌شوند.
fn bridge_lines(launch: &TorLaunch, transport: &str) -> Result<Vec<String>, String> {
    match launch.bridges {
        BridgeMode::None => Ok(Vec::new()),
        BridgeMode::Custom => Ok(launch.bridge_lines()),
        BridgeMode::BuiltIn => {
            let transport = match transport.trim() {
                "" => "obfs4",
                other => other,
            };
            let lines = built_in_bridges(&launch.support.join("pt_config.json"), transport)?;
            if lines.is_empty() {
                return Err(format!(
                    "این نسخه پلِ داخلیِ {transport} ندارد؛ یکی را دستی بچسبان"
                ));
            }
            Ok(lines)
        }
    }
}

/// پل‌های داخلیِ خودِ Tor، از فایلی که کنارِ باینری می‌آید.
///
/// به‌عمد فهرستِ ما نیست. فهرستِ دستی می‌پوسد: لاگِ ۱۶ سپتامبر نشان داد یکی
/// از پل‌های فهرستِ ما دیگر جواب نمی‌داد و سطرِ `meek_lite` بی‌فینگرپرینت را
/// arti نمی‌خواند. این یکی را Tor نگه می‌دارد، با همان باینری سفر می‌کند، و
/// زیرِ همان دایجستِ امضاشده‌ای است که اسکریپتِ تدارک بررسی می‌کند.
fn built_in_bridges(path: &Path, transport: &str) -> Result<Vec<String>, String> {
    let body = std::fs::read_to_string(path)
        .map_err(|error| format!("فهرستِ پل‌های داخلی خوانده نشد: {error}"))?;
    parse_built_in(&body, transport)
}

fn parse_built_in(body: &str, transport: &str) -> Result<Vec<String>, String> {
    let config: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("فهرستِ پل‌های داخلی ناخوانا است: {error}"))?;
    Ok(config
        .get("bridges")
        .and_then(|bridges| bridges.get(transport))
        .and_then(|list| list.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|value| value.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default())
}

/// مسیر، همان‌طور که torrc می‌خواهد.
///
/// نقل‌قول‌شده و با بک‌اسلشِ دوبرابر: مسیرِ ویندوز پر از بک‌اسلش است و تور در
/// مقدارِ نقل‌قول‌شده آن را escape می‌خواند، پس `C:\Users\…` به مسیری با
/// کاراکترِ کنترلی تبدیل می‌شود و تور فایل را رد می‌کند.
fn quote(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('\\', "\\\\"))
}

fn read_control_port(path: &Path, timeout: Duration) -> Result<SocketAddr, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(body) = std::fs::read_to_string(path) {
            // PORT=127.0.0.1:51234
            if let Some(address) = body.trim().strip_prefix("PORT=") {
                if let Ok(parsed) = address.trim().parse() {
                    return Ok(parsed);
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("تور هیچ پورتِ کنترلی اعلام نکرد".into())
}

fn read_cookie(path: &Path, timeout: Duration) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("تور کوکیِ کنترلش را ننوشت".into())
}

/// `NOTICE BOOTSTRAP PROGRESS=25 TAG=… SUMMARY="Loading…"`
fn parse_progress(phase: &str) -> Option<u8> {
    phase
        .split("PROGRESS=")
        .nth(1)?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn parse_summary(phase: &str) -> Option<String> {
    let after = phase.split("SUMMARY=\"").nth(1)?;
    let text = after.split('"').next()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// اثرِ انگشتِ اولین هاپ از پاسخِ `GETINFO circuit-status`.
///
/// قالبِ پاسخِ تور:
///
/// ```text
/// 250+circuit-status=
/// 5 BUILT $AAAA…~guard,$BBBB…~middle,$CCCC…~exit BUILD_FLAGS=… PURPOSE=GENERAL
/// .
/// 250 OK
/// ```
///
/// فقط مدارِ ساخته‌شده به کار می‌آید؛ مدارِ نیمه‌کاره مسیرِ نهایی‌اش را ندارد.
fn first_hop_of(reply: &str) -> Option<String> {
    for line in reply.lines() {
        let mut parts = line.split_whitespace();
        let _id = parts.next()?;
        if parts.next() != Some("BUILT") {
            continue;
        }
        let path = parts.next()?;
        let hop = path.split(',').next()?;
        let fingerprint = hop.trim_start_matches('$').split(['~', '=']).next()?.trim();
        if fingerprint.len() == 40 && fingerprint.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(fingerprint.to_ascii_uppercase());
        }
    }
    None
}

/// نشانیِ `ip:port`ِ پلی که این اثرِ انگشت مالِ آن است.
///
/// # چرا هر نشانی‌ای پذیرفته نمی‌شود
///
/// سطرهای اسنوفلیک نشانیِ ساختگی دارند — `192.0.2.3:80` از بلوکِ مستنداتِ
/// RFC 5737. آن عدد هیچ‌جا نیست و نشان‌دادنش به کاربر دروغ است: اسنوفلیک از
/// راهِ کارگزارِ WebRTC می‌رود، نه از آن نشانی. پس بلوکِ مستندات رد می‌شود و
/// در آن حالت اندپوینتی گزارش نمی‌شود.
fn bridge_endpoint_of(bridges: &[String], fingerprint: &str) -> Option<String> {
    for line in bridges {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let has = tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case(fingerprint));
        if !has {
            continue;
        }
        for token in &tokens {
            if let Ok(addr) = token.parse::<SocketAddr>() {
                if is_documentation_addr(&addr) {
                    return None;
                }
                return Some(addr.to_string());
            }
        }
    }
    None
}

/// بلوکِ `192.0.2.0/24` (RFC 5737) — نشانیِ نمونه، نه نشانیِ واقعی.
fn is_documentation_addr(addr: &SocketAddr) -> bool {
    match addr.ip() {
        std::net::IpAddr::V4(v4) => v4.octets()[..3] == [192, 0, 2],
        std::net::IpAddr::V6(_) => false,
    }
}

/// یک گفت‌وگوی احرازشده با پورتِ کنترلِ تور.
struct ControlSession {
    stream: TcpStream,
    /// بایت‌هایی که خوانده شده‌اند ولی هنوز سطرِ کاملی نساخته‌اند.
    ///
    /// # چرا دستی و نه `BufReader`
    ///
    /// خواندن با مهلتِ کوتاه، هر [`CONTROL_READ_SLICE`] یک `TimedOut` می‌دهد و ما
    /// می‌خواهیم صبر کنیم نه شکست بخوریم. اگر آن خطا از میانِ یک سطرِ نیمه‌خوانده بگذرد،
    /// بایت‌های خوانده‌شده نباید گم شوند — پس بافر مالِ خودِ نشست است و بین دو تلاش
    /// باقی می‌ماند.
    pending: Vec<u8>,
}

impl ControlSession {
    /// نشست را می‌گیرد و تا `deadline` دست برنمی‌دارد.
    ///
    /// `alive` هر دور پرسیده می‌شود: توری که مرده، صبرکردن برایش فقط تلف‌کردنِ بودجهٔ
    /// موجِ بعدی است.
    fn open(
        control: SocketAddr,
        cookie: &[u8],
        deadline: Instant,
        alive: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        let hex: String = cookie.iter().map(|byte| format!("{byte:02x}")).collect();
        loop {
            if !alive() {
                return Err("تور پیش از پاسخ‌دادن به پورتِ کنترل متوقف شد".into());
            }
            match Self::try_open(control, &hex, deadline) {
                Ok(session) => return Ok(session),
                // آخرین علتِ واقعی برگردانده می‌شود، نه یک متنِ آماده: پیشتر یک
                // مقدارِ اولیه اینجا بود که هرگز خوانده نمی‌شد.
                Err(error) => {
                    if Instant::now() >= deadline {
                        return Err(error);
                    }
                }
            }
            thread::sleep(CONTROL_RETRY);
        }
    }

    fn try_open(control: SocketAddr, hex: &str, deadline: Instant) -> Result<Self, String> {
        let stream = TcpStream::connect_timeout(&control, CONTROL_CONNECT_TIMEOUT)
            .map_err(|error| format!("پورتِ کنترلِ تور در دسترس نبود: {error}"))?;
        stream
            .set_read_timeout(Some(CONTROL_READ_SLICE))
            .map_err(|error| format!("تنظیمِ پورتِ کنترلِ تور نشد: {error}"))?;
        let mut session = Self {
            stream,
            pending: Vec::new(),
        };

        // احراز با کوکی، نه رمز: تور فایلی می‌نویسد که فقط همین کاربر
        // می‌خواندش، و ما ثابت می‌کنیم خوانده‌ایمش.
        let reply = session.command(&format!("AUTHENTICATE {hex}"), deadline)?;
        if !reply.starts_with("250") {
            return Err(format!("پورتِ کنترلِ تور احراز را رد کرد: {reply}"));
        }
        Ok(session)
    }

    fn command(&mut self, line: &str, deadline: Instant) -> Result<String, String> {
        self.stream
            .write_all(format!("{line}\r\n").as_bytes())
            .map_err(|error| format!("نوشتن روی پورتِ کنترلِ تور نشد: {error}"))?;
        self.read_reply(deadline)
    }

    /// یک پاسخ را می‌خواند، که می‌تواند چند سطر باشد.
    ///
    /// پروتکل ادامه را با `-` یا `+` پس از کد نشان می‌دهد و سطرِ آخر را با
    /// فاصله؛ پس خواندنِ یک سطر، سطرِ اولِ پاسخِ چندسطری را برمی‌دارد و بقیه
    /// را برای فرمانِ بعدی جا می‌گذارد تا غلط خوانده شود.
    fn read_reply(&mut self, deadline: Instant) -> Result<String, String> {
        let mut reply = String::new();
        loop {
            let line = self.read_line(deadline)?;
            if !reply.is_empty() {
                reply.push('\n');
            }
            reply.push_str(&line);
            // «250 OK» تمامش می‌کند؛ «250-…» و «250+…» نه.
            if line.len() < 4 || line.as_bytes()[3] == b' ' {
                return Ok(reply);
            }
        }
    }

    /// یک سطر، با صبر تا `deadline`.
    ///
    /// مهلتِ سوکت کوچک است و پرشدنش تصمیم نیست: تا زمانی که مهلتِ فرمان نگذشته،
    /// دوباره می‌خوانیم. این همان جایی است که لاگِ ۱۷ سپتامبر در آن سه بار مرد —
    /// os error 10060 روی سوکتی که تور چند ثانیه بعد به آن جواب می‌داد.
    fn read_line(&mut self, deadline: Instant) -> Result<String, String> {
        loop {
            if let Some(at) = self.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=at).collect();
                return Ok(String::from_utf8_lossy(&line).trim_end().to_string());
            }
            let mut chunk = [0u8; 512];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err("پورتِ کنترلِ تور بسته شد".into()),
                Ok(read) => self.pending.extend_from_slice(&chunk[..read]),
                Err(error) if is_timeout(&error) => {
                    if Instant::now() >= deadline {
                        return Err(format!("تور در مهلت به پورتِ کنترل جواب نداد ({error})"));
                    }
                }
                Err(error) => return Err(format!("خواندنِ پورتِ کنترلِ تور نشد: {error}")),
            }
        }
    }

    fn get_info(&mut self, key: &str, budget: Duration) -> Result<String, String> {
        let reply = self.command(&format!("GETINFO {key}"), Instant::now() + budget)?;
        if !reply.starts_with("250") {
            return Err(format!("تور GETINFO {key} را رد کرد: {reply}"));
        }
        Ok(reply)
    }

    /// اثرِ انگشتِ اولین هاپِ مدارِ ساخته‌شده — «اندپوینت»ی که واقعاً به آن وصل شده‌ایم.
    fn first_hop(&mut self, budget: Duration) -> Option<String> {
        let reply = self.get_info("circuit-status", budget).ok()?;
        first_hop_of(&reply)
    }

    /// پورتی که تور واقعاً گرفته، پرسیده‌شده نه فرض‌شده.
    fn socks_port(&mut self, budget: Duration) -> Result<u16, String> {
        let reply = self.get_info("net/listeners/socks", budget)?;
        // 250-net/listeners/socks="127.0.0.1:9150"
        let quoted = reply
            .split('"')
            .nth(1)
            .ok_or("تور هیچ لیسنرِ SOCKS اعلام نکرد")?;
        quoted
            .rsplit(':')
            .next()
            .and_then(|port| port.parse().ok())
            .ok_or_else(|| format!("لیسنرِ SOCKSی که تور گفت خوانده نشد: {quoted}"))
    }
}

/// مهلتِ خواندن پر شد — که در ویندوز `TimedOut` است و در برخی مسیرها `WouldBlock`.
fn is_timeout(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> TorLaunch {
        TorLaunch {
            binary: PathBuf::from("C:\\Program Files\\Aether\\tor\\tor.exe"),
            support: PathBuf::from("C:\\Program Files\\Aether\\tor"),
            home: PathBuf::from("C:\\Users\\Sadeghi\\AppData\\Local\\aether\\tor"),
            bridges: BridgeMode::None,
            transport: String::new(),
            custom_bridges: String::new(),
            upstream: None,
            socks: None,
            budget: None,
        }
    }

    #[test]
    fn paths_are_quoted_with_doubled_backslashes() {
        let config = render_torrc(
            &launch(),
            "",
            Path::new("C:\\Users\\Sadeghi\\control-port"),
            Path::new("C:\\Users\\Sadeghi\\cookie"),
        )
        .expect("renders");
        assert!(config.contains(
            "DataDirectory \"C:\\\\Users\\\\Sadeghi\\\\AppData\\\\Local\\\\aether\\\\tor\""
        ));
        assert!(config.contains("SocksPort auto"));
        assert!(config.contains("ControlPort auto"));
        assert!(config.contains("CookieAuthentication 1"));
        assert!(config.contains("__OwningControllerProcess"));
    }

    #[test]
    fn a_chained_tor_takes_direct_relays_and_an_upstream() {
        let mut launch = launch();
        launch.upstream = Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 1819)));
        launch.bridges = BridgeMode::BuiltIn;
        let config = render_torrc(&launch, "", Path::new("c"), Path::new("k")).expect("renders");
        assert!(config.contains("Socks5Proxy 127.0.0.1:1819"));
        // پشتِ حاملِ دیگر، ترابر کاری ندارد — و lyrebird هم لازم نمی‌شود،
        // پس رندر بی‌آن‌که فایل موجود باشد باید موفق شود.
        assert!(!config.contains("UseBridges"));
        assert!(!config.contains("ClientTransportPlugin"));
    }

    #[test]
    fn pasted_lines_lose_the_bridge_keyword() {
        let mut launch = launch();
        launch.bridges = BridgeMode::Custom;
        launch.custom_bridges =
            "Bridge obfs4 1.2.3.4:80 ABCD cert=x iat-mode=0\n\n# a comment\nobfs4 5.6.7.8:443 EF01 cert=y iat-mode=0\n"
                .into();
        let lines = bridge_lines(&launch, "").expect("lines");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("obfs4 1.2.3.4:80"));
        assert!(lines[1].starts_with("obfs4 5.6.7.8:443"));
    }

    #[test]
    fn custom_mode_needs_at_least_one_line() {
        let mut launch = launch();
        launch.bridges = BridgeMode::Custom;
        launch.custom_bridges = "   \n# only a comment\n".into();
        assert!(launch.bridge_lines().is_empty());
    }

    #[test]
    fn built_in_bridges_come_from_tors_own_file() {
        // شکلِ واقعیِ pt_config.json در بستهٔ رسمیِ 15.0.23.
        let body = r#"{
            "recommendedDefault": "obfs4",
            "bridges": {
                "meek": ["meek_lite 192.0.2.20:80 url=https://example.invalid front=www.phpmyadmin.net utls=HelloRandomizedALPN"],
                "obfs4": ["obfs4 37.218.245.14:38224 D9A8 cert=bjRa iat-mode=0"],
                "snowflake": ["snowflake 192.0.2.3:80 2B28 fingerprint=2B28 url=https://example.invalid/ fronts=app.datapacket.com,www.datapacket.com ice=stun:stun.epygi.com:3478"]
            }
        }"#;
        let obfs4 = parse_built_in(body, "obfs4").expect("parses");
        assert_eq!(obfs4.len(), 1);
        // سطرِ meek_lite فینگرپرینت ندارد و C-tor آن را می‌پذیرد؛ arti نه.
        // این تفاوت، دلیلِ وجودِ همین ماژول است.
        let meek = parse_built_in(body, "meek").expect("parses");
        assert!(meek[0].starts_with("meek_lite "));
        // `fronts=` جمع است، همان‌طور که Tor می‌فرستد. دست نمی‌زنیم.
        let snowflake = parse_built_in(body, "snowflake").expect("parses");
        assert!(snowflake[0].contains("fronts=app.datapacket.com,www.datapacket.com"));
        assert!(parse_built_in(body, "webtunnel")
            .expect("parses")
            .is_empty());
    }

    #[test]
    fn progress_and_summary_come_out_of_one_line() {
        let phase = "250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=15 TAG=conn_done \
                     SUMMARY=\"Connecting to a relay\"\n250 OK";
        assert_eq!(parse_progress(phase), Some(15));
        assert_eq!(
            parse_summary(phase).as_deref(),
            Some("Connecting to a relay")
        );
        assert_eq!(parse_progress("250 OK"), None);
    }

    #[test]
    fn a_hundred_percent_is_what_counts() {
        let done = "250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done \
                    SUMMARY=\"Done\"\n250 OK";
        assert_eq!(parse_progress(done), Some(100));
    }

    #[test]
    fn a_fixed_socks_port_is_written_as_a_loopback_address() {
        let mut fixed = launch();
        fixed.socks = Some(1819);
        let config = render_torrc(&fixed, "", Path::new("c"), Path::new("k")).expect("renders");
        assert!(config.contains("SocksPort 127.0.0.1:1819"));
        assert!(!config.contains("SocksPort auto"));
        // پورتِ کنترل همچنان auto می‌ماند: دو پورتِ ثابت، دو برخوردِ ممکن.
        assert!(config.contains("ControlPort auto"));
    }

    #[test]
    fn a_missing_binary_is_refused_before_anything_is_written() {
        let mut broken = launch();
        broken.binary = PathBuf::from("/definitely/not/here/tor");
        assert!(broken.validate().is_err());
    }

    /// لاگِ ۱۷ سپتامبر: `Bootstrapped 0% (starting): Starting` تنها روایتِ پیشرفت است
    /// وقتی پورتِ کنترل ساکت مانده.
    #[test]
    fn progress_also_comes_out_of_tors_own_log() {
        assert_eq!(
            parse_log_progress("Sep 17 08:15:33.000 [notice] Bootstrapped 0% (starting): Starting"),
            Some(0)
        );
        assert_eq!(
            parse_log_progress("[notice] Bootstrapped 100% (done): Done"),
            Some(100)
        );
        assert_eq!(
            parse_log_progress("[notice] Starting with guard context"),
            None
        );
    }

    #[test]
    fn the_socks_port_can_be_read_off_the_log_too() {
        assert_eq!(
            parse_log_socks(
                "Sep 17 08:15:30.803 [notice] Opened Socks listener connection (ready) on 127.0.0.1:1819"
            ),
            Some(1819)
        );
        // یک نشانیِ غیرِ loopback قراردادِ ما نیست و نباید به‌عنوانِ پورتِ ما جا بزند.
        assert_eq!(
            parse_log_socks("[notice] Opened Socks listener connection (ready) on 10.0.0.5:9050"),
            None
        );
        assert_eq!(
            parse_log_socks("[notice] Opening Control listener on 127.0.0.1:0"),
            None
        );
    }

    /// یک ترابر، یک باینری. سطرِ ثابتِ قبلی مدعیِ snowflakeی بود که نمی‌فرستادیم.
    #[test]
    fn a_transport_is_only_offered_when_its_binary_is_here() {
        let dir = std::env::temp_dir().join(format!("aether-pt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let lyrebird = dir.join(LYREBIRD_FILENAME);
        std::fs::write(&lyrebird, b"binary").expect("write");

        let plugins = available_plugins(&dir);
        assert!(plugin_for(&plugins, "obfs4").is_some());
        assert!(plugin_for(&plugins, "meek_lite").is_some());
        // snowflake را lyrebird اجرا نمی‌کند.
        assert!(plugin_for(&plugins, "snowflake").is_none());

        std::fs::write(dir.join(SNOWFLAKE_FILENAME), b"binary").expect("write");
        let plugins = available_plugins(&dir);
        assert!(plugin_for(&plugins, "snowflake").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// بودجه از برنامه می‌آید، وگرنه لاگ `timeout=600s` می‌گوید و کد ۱۸۰ ثانیه اعمال
    /// می‌کند — همان ناسازگاریِ لاگِ ۱۷ سپتامبر.
    #[test]
    fn the_session_budget_is_the_apps_not_a_constant() {
        let mut wide = launch();
        wide.budget = Some(Duration::from_secs(600));
        assert_eq!(wide.budget.unwrap_or(BOOTSTRAP_TIMEOUT).as_secs(), 600);
        assert_eq!(
            launch().budget.unwrap_or(BOOTSTRAP_TIMEOUT),
            BOOTSTRAP_TIMEOUT
        );
    }

    /// بی پلِ داخلی، موجی هم نیست: یک نوبت، همان‌طور که کاربر خواسته.
    #[test]
    fn waves_are_only_for_the_built_in_list() {
        let direct = launch();
        assert_eq!(plan_waves(&direct), vec![String::new()]);

        let mut chosen = launch();
        chosen.bridges = BridgeMode::BuiltIn;
        chosen.transport = "snowflake".into();
        assert_eq!(plan_waves(&chosen), vec!["snowflake".to_string()]);

        // پشتِ حاملِ دیگر، پل معنا ندارد و نردبان هم نه.
        let mut chained = launch();
        chained.bridges = BridgeMode::BuiltIn;
        chained.upstream = Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 1819)));
        assert_eq!(plan_waves(&chained), vec![String::new()]);
    }
}

#!/usr/bin/env bash
# کنترل‌های منفیِ نگهبانِ کارتِ اتصال و زنجیره‌های تور.
#
# هر بند یکی از راه‌هایی است که یکی از چهار اصلاحِ امروز می‌تواند در بازنویسیِ
# بعدی ساکت از بین برود. نگهبان باید هر کدام را جداگانه بگیرد — وگرنه سبزیِ
# آن هیچ چیزی را ثابت نمی‌کند.
set -u
cd "$(dirname "$0")/.."

GUARD=scripts/check-connection-card.py
K=native/aether/aether/src/tor.rs
S=src-tauri/src/state.rs
P=src-tauri/src/probe.rs
E=src-tauri/src/engine.rs
M=src-tauri/src/main.rs
C=src/views/connectioncard.js
N=src-tauri/src/tor_native.rs
F=src-tauri/src/firsthop.rs

cp "$K" /tmp/c.kern; cp "$S" /tmp/c.state; cp "$P" /tmp/c.probe
cp "$E" /tmp/c.engine; cp "$M" /tmp/c.main; cp "$C" /tmp/c.card
cp "$N" /tmp/c.native; cp "$F" /tmp/c.firsthop
restore() {
  cp /tmp/c.kern "$K"; cp /tmp/c.state "$S"; cp /tmp/c.probe "$P"
  cp /tmp/c.engine "$E"; cp /tmp/c.main "$M"; cp /tmp/c.card "$C"
  cp /tmp/c.native "$N"; cp /tmp/c.firsthop "$F"
}

# بازگردانی باید اثبات شود، نه فرض.
#
# در اجرای اول، `firsthop.rs` در فهرستِ بازگردانی نبود و دست‌کاریِ بندِ ۸
# روی درخت ماند؛ هشت بندِ بعدی همه قرمز شدند — ولی همه به دلیلِ عیناً
# یکسان، که معنایش این است که هیچ‌کدام چیزی را اثبات نکرده بود. اینجا پس از
# هر بازگردانی می‌پرسیم نگهبان دوباره سبز شده یا نه.
restore_and_prove() {
  restore
  if ! python3 "$GUARD" >/dev/null 2>&1; then
    echo "  ✗ بازگردانی ناقص ماند — کنترل‌های بعدی بی‌معنا می‌شوند"
    fails=$((fails + 1))
  fi
}
trap restore EXIT

fails=0
expect_red() { # نام
  local name="$1"
  if python3 "$GUARD" >/tmp/c.out 2>&1; then
    echo "  ✗ $name — نگهبان سبز ماند (باید می‌گرفت)"
    fails=$((fails + 1))
  else
    echo "  ✓ $name → $(grep -m1 '^   - ' /tmp/c.out | sed 's/^   - //' | cut -c1-92)"
  fi
  restore_and_prove
}

echo "کنترل‌های منفی — کارتِ اتصال و زنجیره‌های تور"

# ۱) رانتایم به مسیرِ async برمی‌گردد — همان پانیکِ loge3
python3 - "$K" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1]); t = p.read_text(encoding="utf-8")
t = t.replace('.name("tor-runtime-birth".to_string())', '.name("tor-runtime".to_string())')
p.write_text(t, encoding="utf-8")
PY
expect_red "نامِ تردِ تولد گم شود (رانتایم دوباره در async ساخته می‌شود)"

# ۲) ساختِ کلاینت به شکلِ همگام برمی‌گردد (قفلِ فایل، executor بسته)
sed -i 's/create_unbootstrapped_async()/create_unbootstrapped()/' "$K"
expect_red "ساختِ همگامِ کلاینت"

# ۳) دروازه دوباره منتظرِ پورتِ خروجیِ زنجیره می‌شود — قفلِ دوطرفهٔ loge4
python3 - "$S" <<'PY'
import sys, pathlib, re
p = pathlib.Path(sys.argv[1]); t = p.read_text(encoding="utf-8")
i = t.index("fn tor_gate(&mut self)")
head, tail = t[:i], t[i:]
tail = tail.replace("let exit = self.tor_listener_port();",
                    "let exit = self.data_path_exit_port();", 1)
p.write_text(head + tail, encoding="utf-8")
PY
expect_red "دروازه به پورتِ استیج ۲ برمی‌گردد"

# ۴) ماژولِ هاپِ اول ثبت‌نشده می‌ماند — کدِ مرده
sed -i 's/^mod firsthop;/\/\/ mod firsthop;/' "$M"
expect_red "ثبت‌نشدنِ mod firsthop"

# ۵) سینکِ لاگِ موتور دیگر به هاپِ اول نمی‌رسد
sed -i 's/^\( *\)crate::firsthop::ingest(&line);/\1\/\/ crate::firsthop::ingest(\&line);/' "$E"
expect_red "قطعِ ingest در موتور"

# ۶) نشستِ تازه اندپوینتِ نشستِ قبلی را به ارث می‌برد
sed -i 's/^\( *\)crate::firsthop::reset();/\1\/\/ crate::firsthop::reset();/' "$S"
expect_red "نبودِ reset بین دو نشست"

# ۷) اندپوینت دوباره تکرارِ ردیفِ بالایی می‌شود
python3 - "$S" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1]); t = p.read_text(encoding="utf-8")
t = t.replace("self.endpoint = crate::firsthop::get().or_else(|| {",
              "self.endpoint = ({", 1)
p.write_text(t, encoding="utf-8")
PY
expect_red "برگشتِ اندپوینت به IPِ خروجی"

# ۸) هاپِ اول بی‌رجوع به SocketAddr پذیرفته شود — هر واژه‌ای پس از مارکر
sed -i 's/token.parse::<SocketAddr>()/Ok::<String, ()>(token.to_string())/' "$F"
expect_red "پذیرشِ هر واژه به‌جای نشانی"

# ۹) کاشیِ پروتکل به شکلِ قبلی برمی‌گردد — «SMART» در حالتِ تورِ تنها
python3 - "$S" <<'PY'
import sys, pathlib, re
p = pathlib.Path(sys.argv[1]); t = p.read_text(encoding="utf-8")
# بی‌توجه به قالب‌بندی: هر چه `cargo fmt` با فاصله‌ها و شکستِ سطر کرده
# باشد، شرطِ دولایه را به شکلِ قدیمی برمی‌گردانیم.
alt = """        if self.chain_exit_port.is_some() {
            if let Some(label) = self.profile.backend.protocol_label() {
                return Some(label.to_string());
            }
        }"""
muster = re.compile(
    r"        if let Some\(label\) = self\.profile\.backend\.protocol_label\(\) \{\s*"
    r"\n\s*if !self\.profile\.is_chained\(\) \|\| self\.chain_exit_port\.is_some\(\) \{"
    r"\s*\n\s*return Some\(label\.to_string\(\)\);\s*\n\s*\}\s*\n\s*\}"
)
neu, zahl = muster.subn(alt, t, count=1)
assert zahl == 1, "شکلِ تازهٔ display_protocol پیدا نشد"
p.write_text(neu, encoding="utf-8")
PY
expect_red "برگشتِ display_protocol به شرطِ زنجیره"

# ۱۰) `T1` دوباره کشور حساب می‌شود — پرچمِ کره‌ای
sed -i 's/^const PSEUDO: \[&str; 5\]/const PSEUDO: [\&str; 4]/' "$P"
sed -i 's/\["T1", "A1", "A2", "O1", "XX"\]/["A1", "A2", "O1", "XX"]/' "$P"
expect_red "افتادنِ T1 از فهرستِ شبه‌کشورها"

# ۱۱) پالایشِ از-داخلِ-تونل شبه‌کشور را به‌عنوان نتیجه می‌پذیرد
python3 - "$P" <<'PY'
import sys, pathlib, re
p = pathlib.Path(sys.argv[1]); t = p.read_text(encoding="utf-8")
# شرطِ is_real_country را از درونِ refine_country برمی‌داریم — با هر قالبی
# که `cargo fmt` بهش داده باشد.
i = t.index("fn refine_country(")
kopf, rest = t[:i], t[i:]
muster = re.compile(r"if is_real_country\(&cc\) \{\s*\n(.*?)\n\s*\}\s*\n", re.S)
neu, zahl = muster.subn(lambda m: m.group(1) + "\n", rest, count=1)
assert zahl == 1, "شرطِ is_real_country در refine_country پیدا نشد"
p.write_text(kopf + neu, encoding="utf-8")
PY
expect_red "پذیرشِ شبه‌کشور در refine_country"

# ۱۲) مقیاسِ پینگِ تور تا مقیاسِ تونل تنگ می‌شود
sed -i 's/^  good: 450,/  good: 190,/' "$C"
expect_red "تنگ‌شدنِ مقیاسِ توری تا مقیاسِ تونل"

# ۱۳) مقیاس در زمانِ رندر از خط‌لوله انتخاب نمی‌شود
sed -i 's/bands = pingBandsFor(connected ? snapshot.protocol : null)/bands = PING_BANDS_TUNNEL/' "$C"
expect_red "ثابت‌شدنِ مقیاس روی تونل"

# ۱۴) ارتفاعِ میله با کلمه واگرا می‌شود
sed -i 's/levelTarget = pingStrength(connected, lastMs, bands)/levelTarget = pingStrength(connected, lastMs)/' "$C"
expect_red "واگراییِ ارتفاعِ میله از برچسب"

# ۱۵) STUN به شکلِ نوبتی برمی‌گردد — همان ۱۰.۷ ثانیه
python3 - "$P" <<'PY'
import sys, pathlib, re
p = pathlib.Path(sys.argv[1]); t = p.read_text(encoding="utf-8")
i = t.index("pub fn stun_reflexive_ip(timeout: Duration)")
j = t.index("\nfn stun_query(", i)
alt = """pub fn stun_reflexive_ip(timeout: Duration) -> Option<StunResult> {
    for server in STUN_SERVERS {
        if let Some(ip) = stun_query(server, timeout) {
            return Some(StunResult { server: server.to_string(), reflexive_ip: ip });
        }
    }
    None
}
"""
p.write_text(t[:i] + alt + t[j:], encoding="utf-8")
PY
expect_red "برگشتِ STUN به پرسشِ نوبتی"

# ۱۶) فرستندهٔ اصلی نمی‌افتد — سکوتِ همه = صبرِ کاملِ مهلت
sed -i 's/^    drop(tx);/    \/\/ drop(tx);/' "$P"
expect_red "نیفتادنِ فرستندهٔ اصلی"

echo
if [ "$fails" -eq 0 ]; then
  echo "✓ همهٔ ۱۶ کنترلِ منفی قرمز شدند"
  exit 0
fi
echo "✗ $fails کنترلِ منفی نگرفت"
exit 1

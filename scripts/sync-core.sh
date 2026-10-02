#!/usr/bin/env bash
# =============================================================================
#  همگام‌سازی هستهٔ Aether — معادل دقیق scripts/sync-core.sh مخزن اندروید.
# -----------------------------------------------------------------------------
#  رفع ریشه‌ای خطای اجرای قبلی:
#      cp: cannot stat 'native/aether': No such file or directory
#  علت: روی مخزن تازه، پوشهٔ native/aether هنوز وجود ندارد (submodule
#  اضافه نشده بود) و اسکریپت مستقیم از آن snapshot می‌گرفت.
#  حالا اگر پوشه نباشد، خود اسکریپت هسته را clone می‌کند. هیچ کار دستی
#  لازم نیست و این مرحله هرگز بیلد را نمی‌شکند.
# =============================================================================
set -euo pipefail

BASELINE="2.0.0"
CORE_DIR="native/aether"
BASELINE_DIR="$CORE_DIR/.upstream-baseline"
PREV_DIR="native/.core-prev"
STATE_FILE="native/.core-sync-state"
# فایل‌هایی که پچ محلی داریم و باید در ارتقای هسته سه‌طرفه merge شوند.
# ۱.۲.۳-p1: کارِ توان‌عبوری (سرعت دانلود) هفت فایل دیگر را هم لمس کرد. اگر
# این‌ها اینجا نباشند، اولین sync موفقِ هسته آن‌ها را بی‌صدا پاک می‌کند و
# دقیقاً همان بیلد کندِ قبلی برمی‌گردد — بدون هیچ خطایی در لاگ.
# ۱.۲.۳-p3: پچ زمانِ کانکت (بودجهٔ اسکن، توقف زودهنگام روی کریرِ مسدود و
# جست‌وجوی موازی/زمان‌بندی‌شدهٔ ECH) فایل dns.rs را هم لمس کرد؛ بدون این خط،
# اولین sync موفق ۹ ثانیه تأخیر کانکت را بی‌صدا برمی‌گرداند.
# ۱.۲.۴ (هستهٔ ۱.۹.۰): آپ‌استریم پچ‌های توان‌عبوریِ ۱.۲.۳ را خودش جذب کرد —
# تفکیک بافر rx/tx نتستک، پنجره‌های HTTP/2 و دسته‌بندی کپسول‌ها. پس دو فایل از
# این فهرست بیرون رفتند:
#   * masque_h2.rs — آپ‌استریم مسیر ارسال را با تسک pump_outbound از نو نوشت.
#   * (سایر پچ‌ها ماندند و حالا با نشانهٔ AETHER-APP-PATCH علامت‌دار هستند.)
# چیزی که آپ‌استریم جذب نکرد و پچش سرِ جایش است: انتخاب CUBIC در smoltcp،
# سقفِ صفِ تحویل بسته در lib.rs و تفکیک SO_RCVBUF/SO_SNDBUF در sysprofile.rs.
#
# ۱.۲.۵ (هستهٔ ۲.۰.۰): ربیسِ این ده فایل روی ۲.۰.۰ دستی انجام شد (۱۷ تعارض،
# هر کدام با یک تصمیم صریح) و `.upstream-baseline` با نسخهٔ پاکِ ۲.۰.۰ تازه شد؛ پس
# merge سه‌طرفهٔ بعدی از همین مبنا شروع می‌کند. آپ‌استریم در ۲.۰.۰ هیچ‌کدام از آن
# ده پچ را جذب نکرد.
#
# ۱.۲.۵-p3 — تصحیحِ یک جملهٔ همین کامنت که دیگر درست نبود:
#   تا پیش از این نوشته بود «tor.rs، bridges.rs، egress.rs عمداً اینجا نیستند:
#   چیزی از خودمان در آن‌ها نیست». آن جمله در لحظهٔ ربیسِ ۲.۰.۰ راست بود و بعد از
#   آن کهنه شد: کارِ تور در ۱۶ و ۱۷ سپتامبر ۲۰۲۶ سه تای این فایل‌ها را پچ کرد و
#   کسی این فهرست را تازه نکرد. اندازهٔ چیزی که در معرضِ حذف بود:
#     tor.rs        ۲۲۹ سطر اختلاف با ۲.۰.۰ پاک (headway، wave-own-runtime،
#                   stall-check، attempt-isolation، bridge-order-per-country)
#     bridges.rs    ۲۴۲ سطر (embedded-bridge-lines-usable، bridgedb-budget،
#                   bridge-order-per-country + تست‌هایشان)
#     lastconn.rs    ۱۹ سطر (scan-once-after-a-slow-cache)
#   یعنی همان تلهٔ «بی‌صدا» که این کامنت برای فایل‌های دیگر هشدارش را می‌دهد،
#   این بار برای خودِ کارِ تور باز مانده بود. egress.rs همچنان بی‌پچ است و
#   عمداً بیرونِ فهرست می‌ماند.
#   از این پس این فهرست دستی نگه داشته نمی‌شود: scripts/check-core-patches.py هر
#   فایلِ هسته‌ای که نشانِ AETHER-APP-PATCH دارد و در این فهرست نیست را در
#   preflight می‌شکند.
PATCHED_FILES=(
  aether/src/prober.rs
  aether/src/wg_prober.rs
  aether/src/netstack.rs
  aether/src/wireguard.rs
  aether/src/lib.rs
  aether/src/sysprofile.rs
  aether/src/upstream.rs
  aether/src/quic.rs
  aether/src/dns.rs
  aether/src/tor.rs
  aether/src/bridges.rs
  aether/src/lastconn.rs
  aether/Cargo.toml
)

# فایل‌هایی که آپ‌استریم اصلاً ندارد و مالِ خودِ این اپ‌اند.
#
# merge سه‌طرفه برای این‌ها بی‌معنی است (مبنایی وجود ندارد)، و مسیرِ ارتقا با
# `rm -rf $CORE_DIR` + `cp -a $TMP/new` کار می‌کند — پس بی این حلقه، اولین ارتقای
# موفقِ هسته این فایل‌ها را نه merge می‌کند و نه نگه می‌دارد، بلکه پاک می‌کند.
# aether/build.rs مهرِ `AETHER-BUILD-STAMP:` را در باینری می‌گذارد؛ همان مهری که
# scripts/verify-package.sh (موردِ ۱۴) در payload می‌سنجد و اپ در زمانِ اجرا با
# آنچه موتور گزارش می‌کند مقایسه می‌کند. با حذفِ بی‌صدای آن، موتورِ کهنه دوباره
# بی‌آنکه کسی بفهمد قابلِ تحویل می‌شد.
APP_OWNED_FILES=(
  aether/build.rs
)

AETHER_REPO="${AETHER_REPO:-CluvexStudio/Aether}"
CORE_API_BASE="${CORE_API_BASE:-https://api.github.com}"
CORE_GIT_BASE="${CORE_GIT_BASE:-https://github.com}"

note() { echo "::notice::$*"; }
warn() { echo "::warning::$*"; }

emit() { echo "$1=$2" >> "${GITHUB_OUTPUT:-/dev/null}"; }

mkdir -p native

# -----------------------------------------------------------------------------
# ۰) تضمین وجود هسته — رفع ریشه‌ای خطای "cannot stat".
# -----------------------------------------------------------------------------
if [[ ! -d "$CORE_DIR/.git" && ! -f "$CORE_DIR/CORE_VERSION" ]]; then
  note "Core not present yet - cloning ${AETHER_REPO} (baseline ${BASELINE})."
  rm -rf "$CORE_DIR"
  mkdir -p "$(dirname "$CORE_DIR")"
  if ! git clone --depth 1 "${CORE_GIT_BASE}/${AETHER_REPO}.git" "$CORE_DIR" 2>/dev/null; then
    warn "Could not clone the core repository. Falling back to the vendored baseline."
    mkdir -p "$CORE_DIR"
    echo "$BASELINE" > "$CORE_DIR/CORE_VERSION"
  fi
  [[ -f "$CORE_DIR/CORE_VERSION" ]] || echo "$BASELINE" > "$CORE_DIR/CORE_VERSION"
fi

VENDORED="$(cat "$CORE_DIR/CORE_VERSION" 2>/dev/null || echo "$BASELINE")"
echo "Vendored core version: ${VENDORED}"

# -----------------------------------------------------------------------------
# ۱) خاموش‌کردن دستی همگام‌سازی
# -----------------------------------------------------------------------------
if [[ "${CORE_SYNC:-on}" == "off" ]]; then
  note "CORE_SYNC=off - keeping the vendored core at ${VENDORED}."
  emit core_version "$VENDORED"
  exit 0
fi

# -----------------------------------------------------------------------------
# ۲) جدیدترین نسخهٔ بالادست
# -----------------------------------------------------------------------------
LATEST="${CORE_TARGET:-}"
if [[ -z "$LATEST" ]]; then
  LATEST="$(curl -fsSL \
      -H 'Accept: application/vnd.github+json' \
      ${GITHUB_TOKEN:+-H "Authorization: Bearer ${GITHUB_TOKEN}"} \
      "${CORE_API_BASE}/repos/${AETHER_REPO}/releases/latest" 2>/dev/null \
    | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n1 || true)"
fi

if [[ -z "$LATEST" ]]; then
  warn "Could not reach upstream - keeping ${VENDORED}."
  emit core_version "$VENDORED"
  exit 0
fi
echo "Upstream latest: ${LATEST}"

if [[ "$LATEST" == "$VENDORED" ]]; then
  note "Core already at ${VENDORED}."
  emit core_version "$VENDORED"
  exit 0
fi

# -----------------------------------------------------------------------------
# ۳) snapshot برای بازگشت خودکار اگر بیلد با هستهٔ جدید شکست خورد
# -----------------------------------------------------------------------------
note "Upgrading core ${VENDORED} -> ${LATEST}"
rm -rf "$PREV_DIR"
mkdir -p "$PREV_DIR"
cp -a "$CORE_DIR/." "$PREV_DIR/"   # دیگر قطعاً وجود دارد

# -----------------------------------------------------------------------------
# ۴) دریافت نسخهٔ جدید و ربیس سه‌طرفهٔ پچ‌های ما
# -----------------------------------------------------------------------------
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if ! git clone --depth 1 --branch "v${LATEST}" \
      "${CORE_GIT_BASE}/${AETHER_REPO}.git" "$TMP/new" 2>/dev/null; then
  warn "Could not fetch v${LATEST} - keeping ${VENDORED}."
  emit core_version "$VENDORED"
  exit 0
fi

for rel in "${PATCHED_FILES[@]}"; do
  ours="$CORE_DIR/$rel"
  base="$BASELINE_DIR/$rel"
  theirs="$TMP/new/$rel"
  # >>> AETHER-APP-PATCH core-sync-never-drops-a-patch-silently
  # پیش از این اینجا یک `|| continue` بود. یعنی نبودِ مبنا — که برای سه فایلِ
  # تازه‌افزوده‌شده واقعاً هم نبود — به‌جای خطا، سکوت می‌داد و فایلِ ما را با
  # نسخهٔ آپ‌استریم عوض می‌کرد. حالا هر سه حالت ارتقا را متوقف می‌کنند و
  # هستهٔ وندورشده سرِ جایش می‌ماند؛ همان رفتارِ محافظه‌کارانهٔ تعارضِ merge.
  if [[ ! -f "$ours" ]]; then
    warn "Patched file ${rel} is not in the vendored core - keeping ${VENDORED}."
    emit core_version "$VENDORED"
    exit 0
  fi
  if [[ ! -f "$base" ]]; then
    warn "No upstream baseline for ${rel} - a three-way merge is impossible, keeping ${VENDORED}."
    emit core_version "$VENDORED"
    exit 0
  fi
  if [[ ! -f "$theirs" ]]; then
    warn "Upstream v${LATEST} no longer ships ${rel} - keeping ${VENDORED}."
    emit core_version "$VENDORED"
    exit 0
  fi
  # <<< AETHER-APP-PATCH core-sync-never-drops-a-patch-silently
  if ! git merge-file -p "$ours" "$base" "$theirs" > "$TMP/merged" 2>/dev/null; then
    warn "Patch conflict in ${rel} - keeping ${VENDORED} and skipping the upgrade."
    emit core_version "$VENDORED"
    exit 0
  fi
  cp "$TMP/merged" "$TMP/new/$rel"
done

rm -rf "$CORE_DIR"
mkdir -p "$(dirname "$CORE_DIR")"
cp -a "$TMP/new" "$CORE_DIR"
rm -rf "$CORE_DIR/.git"
# >>> AETHER-APP-PATCH core-sync-keeps-app-owned-files
# درختِ آپ‌استریم جای درختِ قبلی را گرفت؛ فایل‌های مالِ خودمان در آن نیستند و
# باید از snapshotِ همین اجرا برگردند.
for rel in "${APP_OWNED_FILES[@]}"; do
  keep="$PREV_DIR/$rel"
  if [[ ! -f "$keep" ]]; then
    warn "App-owned ${rel} was not in the snapshot - keeping ${VENDORED}."
    rm -rf "$CORE_DIR"
    mkdir -p "$(dirname "$CORE_DIR")"
    cp -a "$PREV_DIR" "$CORE_DIR"
    emit core_version "$VENDORED"
    exit 0
  fi
  mkdir -p "$(dirname "$CORE_DIR/$rel")"
  cp "$keep" "$CORE_DIR/$rel"
done
# <<< AETHER-APP-PATCH core-sync-keeps-app-owned-files
mkdir -p "$BASELINE_DIR"
for rel in "${PATCHED_FILES[@]}"; do
  [[ -f "$TMP/new/$rel" ]] || continue
  mkdir -p "$BASELINE_DIR/$(dirname "$rel")"
  cp "$TMP/new/$rel" "$BASELINE_DIR/$rel"
done
echo "$LATEST" > "$CORE_DIR/CORE_VERSION"

{
  echo "CORE_PREV_VERSION=$VENDORED"
  echo "CORE_NEW_VERSION=$LATEST"
  echo "CORE_UPGRADED=1"
} > "$STATE_FILE"

note "Core staged at ${LATEST} (rollback snapshot kept in ${PREV_DIR})."
emit core_version "$LATEST"

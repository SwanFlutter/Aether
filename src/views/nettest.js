// src/views/nettest.js — تب «تست شبکه»
//
// همهٔ اندازه‌گیری در Rust (`nettest.rs`) و از مسیر تونل انجام می‌شود؛ این
// فایل فقط snapshot را نقاشی می‌کند. دو مسیر رسیدن به snapshot عمدتاً یکی
// هستند: `listen('aether://nettest')` برای زندهٔ تست، و یک `get_net_test`
// هنگام __onShow تا نتیجهٔ اجرای قبلی — که وقتی تب باز نبود منتشر شد —
// ناپدید نشود.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { app, onChange } from '../main.js'
import { t } from '../i18n.js'

const fmt = (n) => (n == null ? '—' : String(n))

export function renderNetTest() {
  const root = document.createElement('div')
  root.className = 'view view--nettest'
  root.innerHTML = `
    <h2 class="view__title">${t('Network test')}</h2>
    <p class="summary">${t('Ping, download and upload are measured through the tunnel — the same path your real traffic takes.')}</p>

    <div class="row">
      <button class="btn btn--primary" id="nt-run">${t('Run the test')}</button>
    </div>
    <p class="summary nt__phase" id="nt-phase" hidden></p>
    <div class="nt__bar-wrap" id="nt-progress" hidden><div class="nt__bar" id="nt-bar"></div></div>
    <p class="summary nt__error" id="nt-error" hidden></p>

    <div class="nt__grid">
      <div class="card nt__stat">
        <h3 class="card__title">${t('Ping')}</h3>
        <div class="nt__value ltr" id="nt-ping">—</div>
        <div class="nt__sub ltr" id="nt-ping-sub"></div>
        <div class="nt__samples" id="nt-samples"></div>
        <p class="nt__sub">${t('Jitter is the wobble between samples — high jitter hurts calls and games even when the average ping is fine.')}</p>
      </div>
      <div class="card nt__stat">
        <h3 class="card__title">${t('Download')}</h3>
        <div class="nt__value ltr" id="nt-down">—</div>
        <div class="nt__sub">${t('Mbps')}</div>
      </div>
      <div class="card nt__stat">
        <h3 class="card__title">${t('Upload')}</h3>
        <div class="nt__value ltr" id="nt-up">—</div>
        <div class="nt__sub">${t('Mbps')}</div>
      </div>
    </div>
  `

  const run = root.querySelector('#nt-run')
  const phase = root.querySelector('#nt-phase')
  const progress = root.querySelector('#nt-progress')
  const bar = root.querySelector('#nt-bar')
  const error = root.querySelector('#nt-error')
  const ping = root.querySelector('#nt-ping')
  const pingSub = root.querySelector('#nt-ping-sub')
  const samples = root.querySelector('#nt-samples')
  const down = root.querySelector('#nt-down')
  const up = root.querySelector('#nt-up')

  let running = false
  const paintGate = () => {
    run.disabled = running || app.snapshot?.state !== 'CONNECTED'
    run.textContent = running ? t('Testing…') : t('Run the test')
    if (!app.snapshot || app.snapshot.state === 'CONNECTED') error.hidden = true
  }

  const paint = (v) => {
    if (!v) return
    running = v.status === 'RUNNING'
    paintGate()
    phase.hidden = !running
    if (running) phase.textContent = `${t('Phase')}: ${t(v.phase)}`
    progress.hidden = !running
    bar.style.width = `${Math.round((v.progress || 0) * 100)}%`
    error.hidden = !(v.status === 'FAILED' && v.error)
    if (!error.hidden) error.textContent = t(v.error)

    ping.textContent = v.pingAvgMs == null ? '—' : `${v.pingAvgMs} ms`
    pingSub.textContent = v.pingAvgMs == null
      ? ''
      : t('min {0}, avg {1}, max {2}, jitter {3} ms')
          .replace('{0}', fmt(v.pingMinMs))
          .replace('{1}', fmt(v.pingAvgMs))
          .replace('{2}', fmt(v.pingMaxMs))
          .replace('{3}', fmt(v.pingJitterMs))
    const ps = v.pings || []
    const top = Math.max(...ps, 1)
    samples.replaceChildren(
      ...ps.map((ms) => {
        const b = document.createElement('span')
        b.className = 'nt__sample'
        b.style.height = `${Math.max(8, Math.round((ms / top) * 40))}px`
        b.title = `${ms} ms`
        return b
      }),
    )
    down.textContent = v.downloadMbps == null ? '—' : v.downloadMbps.toFixed(1)
    up.textContent = v.uploadMbps == null ? '—' : v.uploadMbps.toFixed(1)
  }

  run.addEventListener('click', async () => {
    error.hidden = true
    try {
      await invoke('net_test_start')
    } catch (e) {
      error.hidden = false
      error.textContent = t(String(e))
    }
  })

  let unlisten = null
  root.__onShow = async () => {
    try {
      unlisten = await listen('aether://nettest', (ev) => paint(ev.payload))
    } catch {
      unlisten = null
    }
    try {
      paint(await invoke('get_net_test'))
    } catch {
      paintGate()
    }
  }
  root.__onHide = () => {
    unlisten?.()
    unlisten = null
  }

  onChange(paintGate, root)
  paintGate()
  return root
}

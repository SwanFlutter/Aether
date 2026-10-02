// =============================================================================
//  src/views/assistant.js — صفحهٔ دستیار
//  پورت از ui/ai/AiScreen.kt (کلید + انتخاب مدل + مشاور + چت)
// =============================================================================
//
//  چهار بخش، به همان ترتیبِ اندروید، و ترتیب اهمیت دارد چون یک نردبانِ راه‌اندازی
//  است: بی‌کلید مدلی نیست، بی‌مدل مشاور و چتی نیست. صفحه‌ای که هر چهار را همیشه
//  نشان بدهد، سه بخشِ مرده به کاربرِ تازه نشان می‌دهد.

import { ai, onAiChange, gateMessage, probeSummary, setApiKey, selectModel, refreshModels, testKey, runAdvisor, dismissAdvisor, setProvider, upsertProvider, removeProvider } from '../ai.js'
import { goToTab } from '../ui/nav.js'
import { t } from '../i18n.js'
import { toast } from '../ui/toast.js'

const KEY_URL = 'https://aistudio.google.com/apikey'

function section(title) {
  const box = document.createElement('section')
  box.className = 'card ai__section'
  const h = document.createElement('h3')
  h.className = 'card__title'
  h.textContent = t(title)
  box.appendChild(h)
  return box
}

function activeProvider() {
  return ai.providers.find((p) => p.id === ai.activeProvider) || null
}

// ------------------------------------------------------ ارائه‌دهنده
//
// جمینای، OpenAI و Claude پیش‌فرض‌اند و کاربر می‌تواند هر endpoint سازگار را
// با Base URL دلخواه اضافه کند. **یک** روش برای دادن کلید وجود دارد: همان
// فیلد API key داخل همین کارت، زیرِ انتخابگرِ ارائه‌دهنده. کاربری که ارائه‌دهنده
// تازه می‌سازد هم شناسه/نام/قالب/نشانی و هم کلید را یک‌جا در همین فرم
// می‌دهد و با یک دکمه Save ذخیره‌شان می‌کند — دو جای جدا برای یک کلید، همان
// چیزی بود که کاربر «دوتا» خواند و حذفش خواست.

/// قالب‌های کلید که هر سرویس از آن شروع می‌شود — راهنمایِ دیدنیِ فیلد، نه اعتبارسنجی.
const KEY_PLACEHOLDER = { GEMINI: 'AIza…', OPEN_AI: 'sk-…', ANTHROPIC: 'sk-ant-…' }

// مقدارهای پذیرفتنیِ فرمان `ai_upsert_provider` روی سیم. 'OPEN_AI' شکلِ
// استانداردِ SCREAMING_SNAKE-case خودِ variant است؛ همان «OPENAI» بود که خطای
// ناشناخته‌بودنِ variant را می‌ساخت (alias سمت Rust دیگر جلویض را می‌گیرد،
// ولی رابط باید شکلِ درست را بفرستد).
const KIND_OPTIONS = [
  ['OPEN_AI', 'OpenAI-compatible'],
  ['ANTHROPIC', 'Anthropic (Claude)'],
]

// مقدارِ ویژهٔ انتخابگر — «ارائه‌دهندهٔ تازه بساز». یک شناسهٔ دوتیره می‌خورد،
// چون شناسهٔ ارائه‌دهنده‌ها به `[a-z0-9_-]` محدودند و Rust آن را رد می‌کند.
const ADD_ID = '__add__'

function selectField(capText, options) {
  const wrap = document.createElement('label')
  wrap.className = 'field'
  const cap = document.createElement('span')
  cap.className = 'field__label'
  cap.textContent = t(capText)
  const sel = document.createElement('select')
  sel.className = 'select'
  for (const [value, label] of options) {
    const opt = document.createElement('option')
    opt.value = value
    opt.textContent = t(label)
    sel.appendChild(opt)
  }
  wrap.append(cap, sel)
  return wrap
}

function providerSection() {
  const box = section('AI provider')

  // ---- انتخابگر ارائه‌دهنده + گزینهٔ «تازه بساز»
  const pickerWrap = selectField('Provider', [])
  const picker = pickerWrap.querySelector('select')
  box.appendChild(pickerWrap)

  // ---- فیلدهای ساخت ارائه‌دهنده؛ فقط در حالت «تازه بساز» دیده می‌شوند.
  const details = document.createElement('div')
  details.className = 'ai__providerform'
  details.hidden = true
  const box_inputs = []
  const field = (label, placeholder) => {
    const wrap = document.createElement('label')
    wrap.className = 'field'
    const cap = document.createElement('span')
    cap.className = 'field__label'
    cap.textContent = t(label)
    const el = document.createElement('input')
    el.className = 'input ltr'
    el.placeholder = placeholder
    el.type = 'text'
    el.autocomplete = 'off'
    wrap.append(cap, el)
    box_inputs.push(el)
    return wrap
  }
  const fId = field('Provider ID', 'myprovider')
  const fName = field('Display name', 'My AI Provider')
  const fKind = selectField('API format', KIND_OPTIONS)
  const fUrl = field('Base URL', 'https://api.example.com/v1')
  const formHint = document.createElement('p')
  formHint.className = 'ai__note'
  formHint.textContent = t('Lowercase letters, numbers, hyphens or underscores. Address must be https://. The key is optional if you manage auth via headers.')
  details.append(fId, fName, fKind, fUrl, formHint)
  box.appendChild(details)

  // ---- کلید API — تنها جای واردکردن کلید در کل برنامه.
  const keyNote = document.createElement('p')
  keyNote.className = 'ai__note'
  keyNote.textContent = t('The key is stored sealed on this PC with Windows DPAPI and is never written to the log.')
  box.appendChild(keyNote)

  // ۱.۲.۴-p1 — کلاس فیلد `.input` است و نه `field__input` (رجوع به CSS:
  // `field__input` هیچ‌وقت تعریف نداشت و فیلد یک input خامِ ویندوز می‌شد).
  const row = document.createElement('div')
  row.className = 'ai__keyrow'
  const input = document.createElement('input')
  input.type = 'password'
  input.className = 'input ltr'
  input.placeholder = 'AIza…'
  input.autocomplete = 'off'
  input.spellcheck = false
  input.setAttribute('aria-label', t('API key'))
  const reveal = document.createElement('button')
  reveal.type = 'button'
  reveal.className = 'btn btn--ghost btn--small'
  row.append(input, reveal)
  box.appendChild(row)

  const paintReveal = () => {
    reveal.textContent = input.type === 'password' ? t('Show') : t('Hide')
  }
  reveal.addEventListener('click', () => {
    input.type = input.type === 'password' ? 'text' : 'password'
    paintReveal()
  })
  paintReveal()

  // یک دکمه Save برای کل کارت: ارائه‌دهندهٔ تازه + کلیدش با هم ذخیره می‌شوند،
  // و روی ارائه‌دهندهٔ آماده فقط همان کلید نوشته می‌شود.
  const actions = document.createElement('div')
  actions.className = 'ai__keyactions'
  const save = document.createElement('button')
  save.type = 'button'
  save.className = 'btn btn--primary'
  save.textContent = t('Save')
  const forget = document.createElement('button')
  forget.type = 'button'
  forget.className = 'btn btn--ghost btn--small btn--danger'
  forget.textContent = t('Forget')
  const removeBtn = document.createElement('button')
  removeBtn.type = 'button'
  removeBtn.className = 'btn btn--ghost btn--small btn--danger'
  removeBtn.textContent = t('Remove provider')
  actions.append(save, forget, removeBtn)
  box.appendChild(actions)

  const state = document.createElement('p')
  state.className = 'ai__keystate'
  box.appendChild(state)

  const link = document.createElement('a')
  link.className = 'ai__link'
  link.href = KEY_URL
  link.target = '_blank'
  link.rel = 'noreferrer'
  link.textContent = t('Get a free key from Google AI Studio')
  box.appendChild(link)

  let adding = false

  picker.addEventListener('change', () => {
    if (picker.value === ADD_ID) {
      adding = true
      details.hidden = false
      return
    }
    adding = false
    details.hidden = true
    setProvider(picker.value).catch(() => {})
  })

  save.addEventListener('click', async () => {
    const key = input.value.trim()
    save.disabled = true
    try {
      if (adding) {
        const [id, name, url] = box_inputs.map((i) => i.value.trim())
        const kind = fKind.querySelector('select').value
        if (!id || !url) {
          formHint.textContent = t('Provider ID and Base URL are required.')
          return
        }
        await upsertProvider(id, name, kind, url)
        box_inputs.forEach((i) => { i.value = '' })
        adding = false
        details.hidden = true
        toast(t('Provider saved.'))
      }
      if (key) {
        await setApiKey(key)
        // فیلد فوراً پاک می‌شود: کلید ذخیره شده و نگه‌داشتنش روی صفحه فقط یک
        // اطلاعات محرمانه است که روی نمایشگر مانده.
        input.value = ''
        input.type = 'password'
        paintReveal()
        // ۱.۲.۴-p1: تأییدِ دیدنی. پیش از این، ذخیرهٔ موفق هیچ بازخوردی نداشت.
        toast(t('API key saved.'))
      } else if (!adding) {
        state.textContent = t('No key typed — nothing was changed.')
        return
      }
      // کشفِ مدل‌ها بلافاصله دنبالش می‌آید، ولی فقط اگر تونل بالا باشد؛ وگرنه
      // کاربر یک خطای شبکه می‌گیرد برای کاری که خودش نخواسته بود.
      if (ai.gateCode === 'NO_MODEL' || ai.gateCode === 'READY') await refreshModels()
    } catch (e) {
      (adding ? formHint : state).textContent = String(e)
    } finally {
      save.disabled = false
    }
  })
  forget.addEventListener('click', async () => {
    await setApiKey('')
    input.value = ''
    toast(t('API key removed'))
  })
  removeBtn.addEventListener('click', () => {
    const p = activeProvider()
    if (!p || p.builtin) return
    removeProvider(p.id).then(() => toast(t('Provider removed.'))).catch(() => {})
  })

  // بازسازی گزینه‌ها فقط وقتی فهرست یا انتخاب عوض شده: sync روی هر رویداد
  // snapshot اجرا می‌شود و یک <select> که وسط بازبودنِ کاربر نو نوسازی شود،
  // هم منوی باز را می‌بندد هم تمرکز را می‌دزدد.
  let pickerSignature = ''
  const sync = () => {
    const p = activeProvider()
    const sig = `${ai.activeProvider}|${adding ? ADD_ID : ''}|${ai.providers.map((x) => `${x.id}${x.displayName}${x.hasKey ? 1 : 0}`).join(',')}`
    if (sig !== pickerSignature) {
      pickerSignature = sig
      picker.replaceChildren()
      for (const prov of ai.providers) {
        const opt = document.createElement('option')
        opt.value = prov.id
        opt.textContent = prov.hasKey
          ? prov.displayName
          : `${prov.displayName} — ${t('no key')}`
        picker.appendChild(opt)
      }
      const add = document.createElement('option')
      add.value = ADD_ID
      add.textContent = t('Add a new provider…')
      picker.appendChild(add)
    }
    picker.value = adding ? ADD_ID : ai.activeProvider
    details.hidden = !adding

    input.placeholder = KEY_PLACEHOLDER[p?.kind] ?? 'sk-…'
    // لینک AI Studio فقط برای جمینای معنا دارد؛ برای بقیهٔ ارائه‌دهنده‌ها پنل
    // خودِ سرویس مرجع است.
    link.hidden = p?.kind !== 'GEMINI'
    state.textContent = ai.hasKey
      ? t('A key ending in …{0} is stored.').replace('{0}', ai.keyHint)
      : t('No key stored.')
    forget.hidden = !ai.hasKey || adding
    removeBtn.hidden = !p || p.builtin || adding
  }
  onAiChange(sync, box)
  sync()
  return box
}

// ------------------------------------------------- تست اتصال به API
//
// یک بخشِ جدا و بالای مدل‌ها، به همان ترتیبِ صفحهٔ موبایل: تا وقتی کاربر نداند
// کلیدش کار می‌کند، فهرست مدل و چت و مشاور همه حدس‌اند.

function testSection() {
  const box = section('Test the API connection')

  const summary = document.createElement('p')
  // کلاس دومِ اختصاصی: هر بخش یک `.ai__note` توضیحی دارد، و این خط باید بی‌ابهام
  // پیدا شود — هم برای CSS و هم برای تست.
  summary.className = 'ai__note ai__probe'
  box.appendChild(summary)

  const run = document.createElement('button')
  run.type = 'button'
  run.className = 'btn btn--primary'
  run.textContent = t('Test the API connection')
  run.addEventListener('click', () => {
    // خطا در snapshot می‌نشیند (`probe.state = FAILED`) و همین‌جا خوانده می‌شود؛
    // گرفتنش اینجا فقط جلوی یک rejection بی‌صاحب را می‌گیرد.
    testKey().catch(() => {})
  })
  box.appendChild(run)

  const sync = () => {
    summary.textContent = probeSummary()
    summary.classList.toggle('is-ok', ai.probe?.state === 'OK')
    summary.classList.toggle('is-bad', ai.probe?.state === 'FAILED')
    run.disabled = !ai.hasKey || ai.busy
  }
  onAiChange(sync, box)
  sync()
  return box
}

// -------------------------------------------------------------- مدل‌ها

function modelSection() {
  const box = section('Model')
  const note = document.createElement('p')
  note.className = 'ai__note'
  box.appendChild(note)

  const list = document.createElement('div')
  list.className = 'ai__models'
  box.appendChild(list)

  const controls = document.createElement('div')
  controls.className = 'ai__keyactions'
  const refresh = document.createElement('button')
  refresh.type = 'button'
  refresh.className = 'btn btn--ghost'
  refresh.textContent = t('Discover models for this key')
  refresh.addEventListener('click', () => refreshModels().catch(() => {}))
  controls.appendChild(refresh)
  // فیلتر «فقط رایگان»: روی OpenAI/OpenRouter معنای عملی دارد — مدل‌های
  // ':free' یا قیمت‌صفر. روی جمینای همهٔ مدل‌های فهرست رایگان‌اند و دکمه
  // پنهان می‌شود تا یک کلید بی‌اثر نباشد.
  const freeOnly = document.createElement('button')
  freeOnly.type = 'button'
  freeOnly.className = 'btn btn--ghost'
  let free = false
  controls.appendChild(freeOnly)
  box.appendChild(controls)

  const sync = () => {
    const gemini = activeProvider()?.kind === 'GEMINI'
    note.textContent = gemini
      ? t('Only fast Flash-class models are offered: they answer on a free key.')
      : t('Models are discovered from the provider itself.')
    freeOnly.hidden = gemini
    freeOnly.textContent = free ? t('Show all models') : t('Free only')
    freeOnly.classList.toggle('is-active', free)
    list.replaceChildren()
    const shown = free ? ai.models.filter((m) => m.free) : ai.models
    if (!shown.length) {
      const empty = document.createElement('p')
      empty.className = 'ai__note'
      empty.textContent = free
        ? t('No free models found for this provider.')
        : ai.hasKey
          ? t('No models discovered yet.')
          : t('Add a key first.')
      list.appendChild(empty)
    }
    for (const model of shown) {
      const row = document.createElement('button')
      row.type = 'button'
      row.className = 'ai__model'
      row.classList.toggle('is-active', model.id === ai.selectedModel)
      const name = document.createElement('span')
      name.className = 'ai__modelname ltr'
      name.textContent = model.id
      row.appendChild(name)
      if (model.free && !gemini) {
        const tag = document.createElement('span')
        tag.className = 'ai__modelmeta'
        tag.textContent = t('free')
        row.appendChild(tag)
      }
      if (model.outputTokenLimit) {
        const limit = document.createElement('span')
        limit.className = 'ai__modelmeta ltr'
        limit.textContent = `${model.outputTokenLimit} out`
        row.appendChild(limit)
      }
      row.addEventListener('click', () => selectModel(model.id).catch(() => {}))
      list.appendChild(row)
    }
    refresh.disabled = !ai.hasKey || ai.busy
    freeOnly.disabled = !ai.models.length
  }
  freeOnly.addEventListener('click', () => {
    free = !free
    sync()
  })
  onAiChange(sync, box)
  sync()
  return box
}

// -------------------------------------------------------------- مشاور

function advisorSection() {
  const box = section('Tune for my network')
  const note = document.createElement('p')
  note.className = 'ai__note'
  note.textContent = t('Reads the recent connection log — with addresses masked and identifiers removed — and changes at most one or two transport settings.')
  box.appendChild(note)

  const run = document.createElement('button')
  run.type = 'button'
  run.className = 'btn btn--primary'
  run.textContent = t('Analyse and tune')
  box.appendChild(run)

  const result = document.createElement('div')
  result.className = 'ai__advisor'
  box.appendChild(result)

  run.addEventListener('click', async () => {
    run.disabled = true
    try {
      await runAdvisor()
    } catch (e) {
      result.replaceChildren(line('is-error', String(e)))
    } finally {
      run.disabled = false
    }
  })

  const sync = () => {
    run.disabled = ai.gateCode !== 'READY' || ai.busy
    const advice = ai.advisor
    result.replaceChildren()
    if (!advice) return

    if (advice.reason) result.appendChild(line('ai__reason', advice.reason))

    if (advice.applied.length) {
      result.appendChild(head(t('Changed')))
      for (const [key, value] of advice.applied) {
        result.appendChild(line('ai__change', `${key} → ${value}`))
      }
    } else {
      result.appendChild(line('ai__reason', t('Nothing needed changing.')))
    }
    // رد‌شده‌ها **نمایش داده می‌شوند** و پنهان نمی‌شوند: مشاوری که بی‌صدا نیمی از
    // پیشنهادش را می‌خورد، ابزاری است که نمی‌توان به آن اعتماد کرد.
    if (advice.rejected.length) {
      result.appendChild(head(t('Refused by the app')))
      for (const [key, reason] of advice.rejected) {
        result.appendChild(line('ai__refused', `${key} — ${reason}`))
      }
    }
    const close = document.createElement('button')
    close.type = 'button'
    close.className = 'btn btn--ghost'
    close.textContent = t('Dismiss')
    close.addEventListener('click', () => dismissAdvisor())
    result.appendChild(close)
  }
  onAiChange(sync, box)
  sync()
  return box
}

function head(text) {
  const el = document.createElement('h4')
  el.className = 'ai__subhead'
  el.textContent = text
  return el
}

function line(cls, text) {
  const el = document.createElement('p')
  el.className = cls
  // همان دلیلِ حباب‌های چت: این خط‌ها متنِ مدل یا پیام خطای موتورند.
  el.dir = 'auto'
  el.textContent = text
  return el
}

// ------------------------------------------------- درِ ورود به صفحهٔ چت
//
// # چرا اینجا فقط یک دکمه است
//
// چتِ درون‌صفحه‌ای (a2) برداشته شد: یک لاگِ اسکرول‌شونده داخل صفحه‌ای که خودش
// اسکرول می‌شود، دو اسکرولِ تودرتو می‌ساخت و کاربر همیشه آن بیرونی را
// می‌چرخاند. گفت‌وگو حالا یک مقصدِ ناوبریِ تمام‌صفحه است، همان‌طور که در موبایل
// هست — رجوع به مستند بالای `views/chat.js`.

function chatLink() {
  const box = section('Chat with the assistant')

  const hint = document.createElement('p')
  hint.className = 'ai__note'
  hint.textContent = t('Ask anything, or say what you want changed')
  box.appendChild(hint)

  const open = document.createElement('button')
  open.type = 'button'
  open.className = 'btn btn--primary'
  open.textContent = t('Open the chat')
  open.addEventListener('click', () => goToTab('chat'))
  box.appendChild(open)

  // شمارندهٔ نوبت‌ها: تنها نشانی که می‌گوید گفت‌وگویی برای برگشتن به آن وجود دارد.
  const count = document.createElement('div')
  count.className = 'ai__note'
  box.appendChild(count)

  const sync = () => {
    const n = ai.messages.length
    count.hidden = n === 0
    count.textContent = t('{0} message(s) in the conversation').replace('{0}', String(n))
  }
  onAiChange(sync, box)
  sync()
  return box
}

// ------------------------------------------------------------- صفحه

export function renderAssistant() {
  const root = document.createElement('div')
  root.className = 'view view--ai'

  const title = document.createElement('h2')
  title.className = 'view__title'
  title.textContent = t('Assistant')
  root.appendChild(title)

  // نوار دروازه بالای همه‌چیز است، به همان دلیلی که نوار اطلاعِ آبی در a2 بالای
  // صفحهٔ تنظیمات است: شرطی که کل صفحه را بی‌اثر می‌کند باید پیش از خودِ صفحه
  // خوانده شود.
  const gate = document.createElement('div')
  gate.className = 'ai__gate'
  root.appendChild(gate)

  const error = document.createElement('div')
  error.className = 'ai__error'
  root.appendChild(error)

  root.append(providerSection(), testSection(), modelSection(), advisorSection(), chatLink())

  const sync = () => {
    const message = gateMessage()
    gate.textContent = message ?? ''
    gate.hidden = !message
    error.textContent = ai.error ?? ''
    error.hidden = !ai.error
  }
  onAiChange(sync, root)
  sync()
  return root
}

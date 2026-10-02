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
// جمینای و OpenAI پیش‌فرض‌اند و کاربر می‌تواند هر endpoint سازگارِ OpenAI را
// با Base URL دلخواه اضافه کند (OpenRouter، پنل خودمیزبان، …). انتخابِ
// ارائه‌دهنده همهٔ پایینِ صفحه را عوض می‌کند: کلید، مدل‌ها، تست.

function providerSection() {
  const box = section('AI provider')
  const note = document.createElement('p')
  note.className = 'ai__note'
  note.textContent = t('Pick the assistant backend, or add any OpenAI-compatible endpoint of your own.')
  box.appendChild(note)

  const list = document.createElement('div')
  list.className = 'ai__models'
  box.appendChild(list)

  // فرم «Add provider» — همان چهار فیلدی که کاربر در اپ مرجع دید: شناسه،
  // نامِ نمایش، Base URL، و کلید.
  const form = document.createElement('div')
  form.className = 'ai__providerform'
  form.hidden = true
  const field = (label, placeholder, type = 'text') => {
    const wrap = document.createElement('label')
    wrap.className = 'field'
    const cap = document.createElement('span')
    cap.className = 'field__label'
    cap.textContent = t(label)
    const input = document.createElement('input')
    input.className = 'input ltr'
    input.placeholder = placeholder
    input.type = type
    input.autocomplete = 'off'
    if (type === 'password') input.spellcheck = false
    wrap.append(cap, input)
    box_inputs.push(input)
    return wrap
 }
  const box_inputs = []
  const fId = field('Provider ID', 'myprovider')
  const fName = field('Display name', 'My AI Provider')
  const fUrl = field('Base URL', 'https://api.example.com/v1')
  const fKey = field('API key', 'API key', 'password')
  const formHint = document.createElement('p')
  formHint.className = 'ai__note'
  formHint.textContent = t('Lowercase letters, numbers, hyphens or underscores. Address must be https://. The key is optional if you manage auth via headers.')
  const saveBtn = document.createElement('button')
  saveBtn.type = 'button'
  saveBtn.className = 'btn btn--primary'
  saveBtn.textContent = t('Save')
  const cancelBtn = document.createElement('button')
  cancelBtn.type = 'button'
  cancelBtn.className = 'btn btn--ghost'
  cancelBtn.textContent = t('Cancel')
  form.append(fId, fName, fUrl, fKey, formHint)
  const actions = document.createElement('div')
  actions.className = 'ai__keyactions'
  actions.append(saveBtn, cancelBtn)
  form.appendChild(actions)
  box.appendChild(form)

  const addBtn = document.createElement('button')
  addBtn.type = 'button'
  addBtn.className = 'btn btn--ghost'
  addBtn.textContent = t('Add OpenAI-compatible provider')
  addBtn.addEventListener('click', () => {
    form.hidden = !form.hidden
    addBtn.textContent = form.hidden ? t('Add OpenAI-compatible provider') : t('Close')
  })
  box.appendChild(addBtn)

  saveBtn.addEventListener('click', async () => {
    const [id, name, url, key] = box_inputs.map((i) => i.value.trim())
    if (!id || !url) {
      formHint.textContent = t('Provider ID and Base URL are required.')
      return
    }
    saveBtn.disabled = true
    try {
      await upsertProvider(id, name, 'OPENAI', url)
      if (key) await setApiKey(key)
      box_inputs.forEach((i) => { i.value = '' })
      form.hidden = true
      toast(t('Provider saved.'))
      if (ai.gateCode === 'NO_MODEL' || ai.gateCode === 'READY') await refreshModels().catch(() => {})
    } catch (e) {
      formHint.textContent = String(e)
    } finally {
      saveBtn.disabled = false
    }
  })
  cancelBtn.addEventListener('click', () => { form.hidden = true })

  const sync = () => {
    list.replaceChildren()
    for (const p of ai.providers) {
      const row = document.createElement('button')
      row.type = 'button'
      row.className = 'ai__model'
      row.classList.toggle('is-active', p.id === ai.activeProvider)
      const name = document.createElement('span')
      name.className = 'ai__modelname ltr'
      name.textContent = p.displayName
      row.appendChild(name)
      const hint = document.createElement('span')
      hint.className = 'ai__modelmeta'
      hint.textContent = p.hasKey ? `…${p.keyHint}` : t('no key')
      row.appendChild(hint)
      if (!p.builtin) {
        const del = document.createElement('span')
        del.className = 'ai__modelmeta'
        del.textContent = '✕'
        del.title = t('Remove')
        del.addEventListener('click', (ev) => {
          ev.stopPropagation()
          removeProvider(p.id).then(() => toast(t('Provider removed.'))).catch(() => {})
        })
        row.appendChild(del)
      }
      row.addEventListener('click', () => setProvider(p.id).catch(() => {}))
      list.appendChild(row)
    }
  }
  onAiChange(sync, box)
  sync()
  return box
}

// ------------------------------------------------------------ کلید API

function keySection() {
  const box = section('API key')
  const note = document.createElement('p')
  note.className = 'ai__note'
  note.textContent = t('The key is stored sealed on this PC with Windows DPAPI and is never written to the log.')
  box.appendChild(note)

  // ۱.۲.۴-p1 — کلاس فیلد `.input` است و نه `field__input`.
  //
  // `field__input` هیچ‌جا در CSS تعریف نشده بود (فقط دو قاعدهٔ flex برایش وجود
  // داشت)، پس این کادر یک input خامِ ویندوز بود: سفید، ریز و بیگانه کنار دکمهٔ
  // ذخیره. `.input` همان فیلدی است که کل صفحهٔ تنظیمات استفاده می‌کند.
  const row = document.createElement('div')
  row.className = 'ai__keyrow'
  const input = document.createElement('input')
  input.type = 'password'
  input.className = 'input ltr'
  input.placeholder = 'AIza…'
  input.autocomplete = 'off'
  input.spellcheck = false
  input.setAttribute('aria-label', t('API key'))
  const save = document.createElement('button')
  save.type = 'button'
  save.className = 'btn btn--primary'
  save.textContent = t('Save')
  row.append(input, save)
  box.appendChild(row)

  // ردیف دوم: نمایش/پنهان و فراموش‌کردن. عمداً زیرِ فیلد و نه کنارش، تا خودِ
  // فیلد تمام عرض کارت را بگیرد.
  const actions = document.createElement('div')
  actions.className = 'ai__keyactions'
  const reveal = document.createElement('button')
  reveal.type = 'button'
  reveal.className = 'btn btn--ghost btn--small'
  const forget = document.createElement('button')
  forget.type = 'button'
  forget.className = 'btn btn--ghost btn--small btn--danger'
  forget.textContent = t('Forget')
  actions.append(reveal, forget)
  box.appendChild(actions)

  const paintReveal = () => {
    reveal.textContent = input.type === 'password' ? t('Show') : t('Hide')
  }
  reveal.addEventListener('click', () => {
    input.type = input.type === 'password' ? 'text' : 'password'
    paintReveal()
  })
  paintReveal()

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
  save.addEventListener('click', async () => {
    const value = input.value.trim()
    if (!value) return
    save.disabled = true
    try {
      await setApiKey(value)
      // فیلد فوراً پاک می‌شود: کلید ذخیره شده و نگه‌داشتنش روی صفحه فقط یک
      // اطلاعات محرمانه است که روی نمایشگر مانده.
      input.value = ''
      input.type = 'password'
      paintReveal()
      // ۱.۲.۴-p1: تأییدِ دیدنی. پیش از این، ذخیرهٔ موفق هیچ بازخوردی نداشت و از
      // یک کلیکِ بی‌اثر قابل تشخیص نبود.
      toast(t('API key saved.'))
      // کشفِ مدل‌ها بلافاصله دنبالش می‌آید، ولی فقط اگر تونل بالا باشد؛ وگرنه
      // کاربر یک خطای شبکه می‌گیرد برای کاری که خودش نخواسته بود.
      if (ai.gateCode === 'NO_MODEL' || ai.gateCode === 'READY') await refreshModels()
    } catch (e) {
      state.textContent = String(e)
    } finally {
      save.disabled = false
    }
  })
  forget.addEventListener('click', async () => {
    await setApiKey('')
    input.value = ''
    toast(t('API key removed'))
  })

  const sync = () => {
    const gemini = activeProvider()?.id === 'gemini'
    input.placeholder = gemini ? 'AIza…' : 'sk-…'
    // لینک AI Studio فقط برای جمینای معنا دارد؛ برای بقیهٔ ارائه‌دهنده‌ها دکمهٔ
    // «افزودن ارائه‌دهنده» یا پنل خودِ سرویس مرجع است.
    link.hidden = !gemini
    state.textContent = ai.hasKey
      ? t('A key ending in …{0} is stored.').replace('{0}', ai.keyHint)
      : t('No key stored.')
    forget.hidden = !ai.hasKey
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
    const gemini = activeProvider()?.id === 'gemini'
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

  root.append(providerSection(), keySection(), testSection(), modelSection(), advisorSection(), chatLink())

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

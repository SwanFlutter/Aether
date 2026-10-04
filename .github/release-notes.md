# Aether 1.2.7

نصب‌کنندهٔ رسمی ویندوز (x64) + فایل چک‌سام.

## تغییرات

### وصل شدن روی شبکهٔ همراه‌اول (MCI)
بررسیِ نشتیِ WebRTC دیگر اتصال را نمی‌ریزد؛ فقط هشدار می‌دهد. روی شبکهٔ موبایلِ همراه‌اول، مسیرِ UDP همیشه باز است و همین باعث می‌شد خودِآزماییِ برنامه هر بار «نشت» تشخیص بدهد و وصل شدن را رد کند. حالا وضعیتِ نشت در جدولِ سلامت و لاگ می‌ماند (نشانِ قرمز)، ولی تونل ساخته می‌شود.

### تبِ دستیار — یک باکس برای کلید
- فرمِ «افزودن ارائه‌دهندهٔ تازه» و دکمهٔ حذف ارائه‌دهنده حذف شد.
- باکسِ آبیِ «AI provider» فقط این‌ها را دارد: انتخابگرِ Gemini / OpenAI / Claude، فیلدِ کلید، Save/Forget، و لینکِ گرفتنِ کلید از هر سرویس.
- بخشِ تازهٔ «Connection log»: لاگِ زندهٔ اتصال + دکمهٔ «Ask AI to analyse log» — کلیدِ خودِ کاربر و نشانی‌ها پیش از فرستادن به AI پاک می‌شوند.

## Windows — what changed

### MCI (Hamrahe Aval) tethering can connect now
The WebRTC leak check no longer vetoes the connection; it stays a warning in the health grid and the log. On mobile tethering the local UDP path is always open, so the self-test always reported a leak and refused to connect. The tunnel is now built even when the leak badge is red.

### Assistant tab — one box for the key
- The "Add a new provider" form and the remove-provider button are gone.
- The blue "AI provider" box now holds only: the Gemini / OpenAI / Claude picker, the key field, Save/Forget, and a "get a key" link per service.
- New "Connection log" panel: live session log plus an "Ask AI to analyse log" button — the user's own key and addresses are redacted before anything is sent to the AI.

## بررسیِ اصالت

- چک‌سامِ SHA-256 در `SHA256SUMS.txt`.
- درایورِ Wintun از سایتِ رسمی و psiphon-tunnel-core از سورسِ بالادستِ خودِ آن‌ها در لحظهٔ بیلد گرفته می‌شود، نه از شخصِ سوم.
- ساختِ آداپتورِ Wintun هنگامِ نصب به دسترسیِ مدیر نیاز دارد.

<p align="center">
  <img src="docs/media/hero.jpg" alt="ساتسوما: فایل را بکش، یک کلید بزن و روی گلبرگ رها کن" width="100%">
</p>

<p align="center">
  <a href="https://github.com/thesaikodelik/satsuma-windows/releases/latest/download/Satsuma-Setup.exe"><img src="https://img.shields.io/badge/download-Satsuma--Setup.exe-ff6e17?style=for-the-badge&logo=windows11&logoColor=white" alt="دانلود ساتسوما برای ویندوز"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-11-0078d4?logo=windows11&logoColor=white" alt="Windows 11">
  <img src="https://img.shields.io/badge/version-1.1.0-ff6e17" alt="نسخه ۱٫۱٫۰">
  <img src="https://img.shields.io/badge/license-MIT-2ea44f" alt="مجوز MIT">
  <img src="https://img.shields.io/badge/made%20with-Rust-b7410e?logo=rust&logoColor=white" alt="ساخته‌شده با Rust">
  <img src="https://img.shields.io/badge/size-12%20MB-6e40c9" alt="۱۲ مگابایت">
</p>

<p align="center">
  <a href="README.md">English</a> · <b>فارسی</b>
</p>

<p align="center">
  <img src="docs/media/demo.gif" width="100%" alt="کشیدن Sunset.jpg از File Explorer و زدن Shift، چرخ تبدیل را زیر نشانگر باز می‌کند؛ رها کردن روی PNG، فایل Sunset.png را کنار اصلی می‌سازد.">
  <br>
  <sub><a href="docs/media/demo.mp4">تماشا به‌صورت ویدیو (MP4)</a></sub>
</p>

<div dir="rtl">

**ساتسوما** هر بار کشیدن فایل را به یک میان‌بر تبدیل می‌کند. وقتی فایلی را می‌کشید، یک بار
**Shift**، **Ctrl** یا **Alt** را بزنید تا چرخی از گزینه‌ها درست زیر نشانگر باز شود. فایل را روی یک
گلبرگ رها کنید و نتیجه کنار فایل اصلی ذخیره می‌شود: عکس PNG می‌شود، ویدیو کم‌حجم می‌شود،
PDFها یکی می‌شوند و عکس پس‌زمینه می‌گیرد. همه‌چیز روی کامپیوتر خودتان انجام می‌شود و چیزی
آپلود نمی‌شود.

این نسخه‌ی ویندوزی [Satsuma](https://github.com/sonatina-dream/satsuma) از sonatina-dream است که
به‌شکل بومی برای ویندوز ۱۱ و با ظاهر *Liquid Glass* خودش ساخته شده.

</div>

<p align="center"><img src="docs/media/highlights-fa.png" width="100%" alt="همه‌ی فرمت‌ها، یک ضربه، خصوصی، خودش به‌روز می‌شود"></p>

<div dir="rtl">

---

## 🎡 سه چرخ، یک حرکت

</div>

<table>
  <tr>
    <td width="33%" align="center"><img src="docs/media/wheel-convert.png" alt="چرخ تبدیل برای JPG"></td>
    <td width="33%" align="center"><img src="docs/media/wheel-tools.png" alt="چرخ ابزارها برای عکس، با گلبرگ Annotate روشن"></td>
    <td width="33%" align="center"><img src="docs/media/wheel-actions.png" alt="چرخ کارها"></td>
  </tr>
  <tr>
    <td dir="rtl"><b>Shift: تبدیل.</b> همه‌ی فرمت‌هایی که این فایل می‌تواند بشود. وسط چرخ، حجم نتیجه را قبل از رها کردن نشان می‌دهد.</td>
    <td dir="rtl"><b>Ctrl: ابزارها.</b> فشرده‌سازی، کوتاه کردن، برش، ویرایش، یادداشت‌گذاری، پوشاندن، مرتب کردن صفحه‌ها، ویرایش مشخصات و بیشتر.</td>
    <td dir="rtl"><b>Alt: کارها.</b> اشتراک‌گذاری، Float، کپی، جابه‌جایی، کپی مسیر و باز کردن ترمینال همان‌جا.</td>
  </tr>
</table>

<div dir="rtl">

۱. یک یا چند فایل را **شروع به کشیدن** کنید؛ از File Explorer، دسکتاپ یا هر برنامه‌ای.<br>
۲. در حالی که کلیک را نگه داشته‌اید **یک بار یک کلید را بزنید**. کلید دیگر، چرخ را عوض می‌کند.<br>
۳. **روی یک گلبرگ رها کنید.** رها کردن در وسط، کار را لغو می‌کند.

<img src="docs/media/wheel-progress.png" align="left" width="230" alt="چرخ در حال تبدیل ویدیو به WebM؛ حلقه‌ی باریک نارنجی لبه‌اش پر می‌شود و وسطش ۴۵٪ نوشته">

**چرخ کار را نشان می‌دهد.** بعد از رها کردن، چرخ سر جایش می‌ماند: حلقه‌ی باریکی از نور لبه‌اش را
پر می‌کند و وسط چرخ درصد را می‌شمارد. نشانگر را روی وسط ببرید تا **Cancel** ظاهر شود؛ وقتی کار تمام شد
تیک می‌خورد. یک پنل کوچک هم نتیجه را خبر می‌دهد:

<img src="docs/media/hud.png" width="380" alt="در حال تبدیل به PNG">

فایلی برای کشیدن ندارید؟ گزینه‌ی **Convert Files…** در منوی ساتسوما همین چرخ را برای فایل‌هایی که
انتخاب می‌کنید باز می‌کند؛ با کلیدهای جهت حرکت کنید و Enter را بزنید.

<br clear="left">

## 🔁 چه چیزهایی را تبدیل می‌کند

| | تبدیل به |
| --- | --- |
| 🖼 **عکس**: JPG، PNG، WebP، HEIC، AVIF، TIFF، BMP، SVG | به یکدیگر، PDF یا Word (متن عکس خوانده می‌شود) |
| 🎬 **ویدیو**: MP4، MOV، MKV، WebM، AVI، WMV | به یکدیگر، GIF یا MP3 |
| 🎵 **صدا**: MP3، M4A، WAV، FLAC، OGG، Opus، AIFF، WMA | به یکدیگر |
| 📄 **PDF** | Word، **PowerPoint**، **Excel**، JPG، PNG، EPUB، TXT، Markdown |
| 📽 **اسلاید**: PowerPoint (PPT، PPTX) | PDF، JPG، PNG |
| 📝 **سند**: Word، Markdown، RTF، HTML، TXT، EPUB | PDF، Word، TXT، HTML، EPUB، PNG، JPG |
| 📊 **جدول**: Excel، ODS، CSV، TSV | به یکدیگر یا PDF |
| 🗜 **فایل فشرده**: ZIP، TAR، GZ | به یکدیگر |

<img src="docs/media/wheel-convert-pdf.png" align="left" width="230" alt="چرخ تبدیل برای PDF، با گلبرگ PPTX روشن و حجم ۱٫۳ مگابایت">

- **PDF به PowerPoint** هر صفحه را روی یک اسلاید می‌گذارد.
- **PDF به Excel** صفحه‌ها را می‌خواند (اسکن‌شده‌ها را هم) و متن را در ردیف و ستون می‌چیند.
- **اسلاید به PDF** با Microsoft PowerPoint انجام می‌شود، و اگر PowerPoint نباشد با LibreOffice.

عکس، سند، PDF، جدول، زیرنویس و فایل فشرده بدون نیاز به چیز دیگری کار می‌کنند. برای صدا و
ویدیو [ffmpeg](https://ffmpeg.org) لازم است: `winget install Gyan.FFmpeg`

<br clear="left">

## 🧰 ابزارها

خیلی از ابزارها اول یک پنجره‌ی کوچک باز می‌کنند تا دقیقاً انتخاب کنید چه بشود. همه‌ی این‌ها روی
کامپیوتر خودتان انجام می‌شود.

### 🖼 عکس

</div>

<table>
  <tr>
    <td width="50%"><img src="docs/media/tool-annotate.png" alt="یادداشت‌گذاری روی عکس دریاچه: فلش، بیضی، دو شماره و نوشته‌ی Swim here!"></td>
    <td width="50%"><img src="docs/media/tool-redact-photo.png" alt="پوشاندن عکس: رمز Wi-Fi روی کارت کلبه محو شده، اسم شبکه نه"></td>
  </tr>
  <tr>
    <td dir="rtl"><b>یادداشت‌گذاری</b>: فلش، کادر، بیضی، خط، هایلایت، متن و نشانگر شماره‌دار، با هفت رنگ و سه ضخامت.</td>
    <td dir="rtl"><b>پوشاندن</b>: هر جا را علامت بزنید محو، پیکسلی یا سیاه می‌شود. <b>Find Text</b> و <b>Find Faces</b> نوشته‌ها و چهره‌ها را خودشان علامت می‌زنند.</td>
  </tr>
</table>

<table>
  <tr>
    <td width="33%"><img src="docs/media/editor-crop.png" alt="برش عکس"></td>
    <td width="33%"><img src="docs/media/editor-background.png" alt="افزودن پس‌زمینه"></td>
    <td width="33%"><img src="docs/media/editor-edit.png" alt="ویرایش عکس"></td>
  </tr>
  <tr>
    <td align="center" dir="rtl"><sub><b>برش عکس</b>: نسبت‌های آماده یا اندازه‌ی دقیق</sub></td>
    <td align="center" dir="rtl"><sub><b>افزودن پس‌زمینه</b>: گرادیان، رنگ یا عکس خودتان</sub></td>
    <td align="center" dir="rtl"><sub><b>ویرایش عکس</b>: ۲۰ تنظیم با پیش‌نمایش زنده</sub></td>
  </tr>
</table>

<div dir="rtl">

همچنین: **فشرده‌سازی** (با شدت دلخواه یا تا حجم مشخص، و حداکثر اندازه‌ی ضلع)، **حذف متادیتا**
(مشخصات دوربین و GPS)، و برای چند عکس **ساخت PDF** و **کلاژ** (شبکه، ردیف یا ستون، با فاصله و رنگ
پس‌زمینه).

### 🎬 ویدیو

</div>

<table>
  <tr>
    <td width="33%"><img src="docs/media/tool-trim-video.png" alt="کوتاه کردن ویدیو: دسته‌های شروع و پایان روی نوار فریم‌های دریاچه، از ۰:۰۳ تا ۰:۱۴"></td>
    <td width="33%"><img src="docs/media/tool-crop-video.png" alt="برش ویدیو: کادر ۹:۱۶ روی صحنه‌ی دریاچه"></td>
    <td width="33%"><img src="docs/media/tool-redact-video.png" alt="پوشاندن ویدیو: خورشید از ۰:۰۴ تا ۰:۱۴ پیکسلی شده"></td>
  </tr>
  <tr>
    <td dir="rtl"><b>کوتاه کردن</b>: دسته‌ها را روی نوار فریم‌ها بکشید و پیش‌نمایش بزرگ را ببینید، یا زمان دقیق را تایپ کنید.</td>
    <td dir="rtl"><b>برش</b>: کادر را بکشید، یا ۱۶:۹، ۹:۱۶، ۱:۱، ۴:۳ یا ۴:۵ را بزنید.</td>
    <td dir="rtl"><b>پوشاندن</b>: کادرها محو، پیکسلی یا سیاه می‌شوند؛ در کل ویدیو یا فقط یک بخش.</td>
  </tr>
</table>

<table>
  <tr>
    <td width="50%"><img src="docs/media/tool-speed.png" alt="تغییر سرعت: ۲ برابر با حفظ زیر و بمی صدا؛ ۰:۲۰ می‌شود ۰:۱۰"></td>
    <td width="50%"><img src="docs/media/tool-compress.png" alt="فشرده‌سازی تا حجم مشخص: الان ۵٫۶ مگابایت، بعد حدود ۲٫۸"></td>
  </tr>
  <tr>
    <td dir="rtl"><b>سرعت</b>: از ۰٫۲۵ تا ۴ برابر؛ اگر بخواهید زیر و بمی صدا عوض نمی‌شود.</td>
    <td dir="rtl"><b>فشرده‌سازی</b>: Balanced، Strong، یا <b>تا یک حجم مشخص</b> که جایی که می‌فرستید قبول کند.</td>
  </tr>
</table>

<div dir="rtl">

همچنین: **عکس از فریم‌ها** (۱ تا ۳۰ فریم در طول ویدیو، PNG یا JPG)، **تقسیم** (به چند قسمت مساوی
یا تکه‌هایی با طول مشخص)، **بی‌صدا کردن**، **ویرایش مشخصات**، و برای چند ویدیو **اتصال**.

### 🎵 صدا

</div>

<table>
  <tr>
    <td width="50%"><img src="docs/media/tool-bleep.png" alt="بیپ صدا: دو بخش از یک پیام صوتی که شماره تلفن گفته می‌شود روی موج صدا علامت خورده"></td>
    <td width="50%"><img src="docs/media/tool-metadata.png" alt="ویرایش مشخصات: عنوان، هنرمند، آلبوم، سال، سبک و توضیح یک پیام صوتی"></td>
  </tr>
  <tr>
    <td dir="rtl"><b>بیپ</b>: بخش‌هایی را روی موج صدا علامت بزنید تا بیپ یا بی‌صدا شوند.</td>
    <td dir="rtl"><b>ویرایش مشخصات</b>: عنوان، هنرمند، آلبوم، سال، سبک و توضیح؛ یا پاک کردن همه.</td>
  </tr>
</table>

<div dir="rtl">

<img src="docs/media/tool-visualize.png" align="left" width="46%" alt="صدا به ویدیو: افقی، عمودی یا مربع، با موج متحرک یا ثابت">

همچنین: **کوتاه کردن** روی موج صدا (یا فقط حذف سکوت اول و آخر)، **مونو/استریو** با تنظیم بلندی
هر طرف، **یکسان‌سازی صدا**، **فشرده‌سازی** و **صدا به ویدیو** (افقی، عمودی یا مربع، با موج متحرک یا
ثابت).

<br clear="left">

### 📄 PDF

<img src="docs/media/tool-organize.png" align="left" width="46%" alt="مرتب کردن PDF: شش صفحه‌ی دفترچه‌ی سفر؛ صفحه‌ی ۶ بالا آمده و صفحه‌ی ۴ نود درجه چرخیده">

**مرتب کردن صفحه‌ها**: همه‌ی صفحه‌ها به‌صورت تصویر کوچک. با کشیدن ترتیبشان را عوض کنید، بچرخانید،
تکرار یا حذف کنید و ذخیره کنید.

همچنین: **فشرده‌سازی**، **تقسیم**، **ویرایش مشخصات** (عنوان، نویسنده، موضوع و کلیدواژه‌ها)، و برای
چند PDF **ادغام**.

<br clear="left">

### 📌 Float

<img src="docs/media/float.png" align="left" width="110" alt="قفسه‌ی Float">

**Float** فایل‌ها را در یک قفسه‌ی کوچک که همیشه رو است دم دست نگه می‌دارد؛ هر وقت لازم شد
آن‌ها را به هر برنامه‌ای بکشید.

<br clear="left">

## 🎨 شیشه‌ی مایع

<img src="docs/media/settings-appearance.png" align="left" width="42%" alt="تنظیمات ظاهر">

- **Liquid**: شیشه‌ی مات که پشتش را محو می‌کند، با هایلایتی که مثل مایع بین گلبرگ‌ها جاری
  می‌شود.
- **Solid**: چرخ مات با رنگ‌های روشن یا تیره‌ی ویندوز.
- یک **اسلایدر شفافیت** برای هر دو، و پیش‌نمایش زنده برای امتحان کردنشان.

<br clear="left">

## ⚙️ به سلیقه‌ی خودتان

</div>

<table>
  <tr>
    <td width="33%"><img src="docs/media/settings-general.png" alt="تنظیمات عمومی"></td>
    <td width="33%"><img src="docs/media/shortcuts.gif" alt="تنظیمات کلیدها با آموزش متحرک"></td>
    <td width="33%"><img src="docs/media/settings-files.png" alt="تنظیمات فایل‌ها"></td>
  </tr>
  <tr>
    <td align="center" dir="rtl"><sub><b>General</b>: برنامه‌ی ترمینال، Float، اجرا هنگام ورود</sub></td>
    <td align="center" dir="rtl"><sub><b>Shortcuts</b>: کلید هر چرخ</sub></td>
    <td align="center" dir="rtl"><sub><b>Files</b>: شدت فشرده‌سازی، اندازه‌ی عکس، تعداد تکه‌های ویدیو</sub></td>
  </tr>
</table>

<div dir="rtl">

## 🔄 همیشه به‌روز

ساتسوما خودش نسخه‌های جدید را پیدا می‌کند و در پس‌زمینه دانلود می‌کند. وقتی آماده شد، وسط چرخ
**Update ready** نوشته می‌شود:

- **فایل‌هایتان را همان‌جا رها کنید** تا ساتسوما همان لحظه به‌روز شود و دوباره باز شود، یا
- هیچ کاری نکنید: همین که بیکار شد خودش به‌روز می‌شود و با نسخه‌ی جدید برمی‌گردد.

در *Settings › About* هم می‌توانید خودتان بررسی کنید.

## 📦 نصب

<img src="docs/media/setup-welcome.png" align="left" width="50%" alt="نصب‌کننده‌ی ساتسوما">

۱. فایل **[Satsuma-Setup.exe](https://github.com/thesaikodelik/satsuma-windows/releases/latest/download/Satsuma-Setup.exe)** را دانلود کنید.<br>
۲. اجرایش کنید و **Install** را بزنید. فقط برای حساب شما نصب می‌شود و **دسترسی مدیر لازم ندارد**.<br>
۳. آیکون پرتقالی ساتسوما در انتهای سمت راست تسک‌بار ظاهر می‌شود (شاید زیر **^** باشد).

> برنامه‌های تازه ممکن است تا وقتی زیاد دانلود نشده‌اند پیام *Windows protected your PC* بگیرند.
> **More info** و بعد **Run anyway** را بزنید. ساتسوما متن‌باز است؛ می‌توانید همه‌ی کدش را
> بخوانید یا خودتان آن را بسازید.

برای حذف از **Settings → Apps** ویندوز استفاده کنید. می‌توانید تنظیماتتان را نگه دارید.

<br clear="left">

## 🗂 تازه‌ها

جدیدترین نسخه، فایل‌هایش و **همه‌ی تغییرات همه‌ی نسخه‌ها تا امروز** در
**[آخرین ریلیز](https://github.com/thesaikodelik/satsuma-windows/releases/latest)** است. فقط به همان
نیاز دارید؛ بعد از آن ساتسوما خودش به‌روز می‌شود.

## 🔒 حریم خصوصی

فایل‌های شما هیچ‌وقت از کامپیوترتان خارج نمی‌شوند. ساتسوما فقط برای پرسیدن از GitHub که نسخه‌ی
جدیدی هست یا نه به اینترنت وصل می‌شود. بدون حساب کاربری، بدون تبلیغ و بدون ردیابی.

## 🛠 ساختن از کد

راهنمای ساخت، خط فرمان و ساختار پروژه در [README انگلیسی](README.md) (بخش Build from source) است.

## 💛 قدردانی

- [**Satsuma**](https://github.com/sonatina-dream/satsuma) از **sonatina-dream**، برنامه‌ی مک که این
  نسخه از آن ساخته شده.
- ایده‌ی کار از [Tangerine](https://tangerineformac.com) برای مک گرفته شده.
- آیکون‌های کارت‌ها: [Fluent Emoji](https://github.com/microsoft/fluentui-emoji) از مایکروسافت (MIT).
- نسخه‌ی ویندوز از [**thesaikodelik**](https://github.com/thesaikodelik). اگر وقتتان را صرفه‌جویی
  کرد، می‌توانید [یک قهوه مهمانم کنید](https://github.com/sponsors/thesaikodelik). ☕

منتشرشده با [مجوز MIT](LICENSE).

</div>

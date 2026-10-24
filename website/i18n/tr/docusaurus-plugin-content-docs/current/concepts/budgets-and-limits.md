---
title: Bütçeler ve sınırlar
sidebar_label: Bütçeler ve sınırlar
description: Ajan çalıştırmalarının neye mal olabileceğini (görev, grup ve gün başına) ve bilgisayardan ne kadar alabileceğini (işlemci, bellek, disk, GPU, süre) sınırlayın ve bir çalıştırmayı istediğiniz an durdurun.
---

# Bütçeler ve sınırlar

Ajan çalıştırmalarını iki tür üst sınır denetim altında tutar:

- **Bütçeler**: ajanların bildirdiğine göre çalıştırmaların ne kadar paraya
  mal olabileceği.
- **Sınırlar**: bir çalıştırmanın bilgisayardan ne kadar alabileceği:
  işlemci, bellek, disk, GPU ve süre.

İkisi de **üç kapsamda** ayarlanabilir.

## Üç kapsam

| Kapsam | Bütçe | Sınırlar |
| --- | --- | --- |
| **Her görev** | Tüm görevlerin herhangi bir 24 saatte birlikte ne kadara mal olabileceği. | Bir grubun ve bir görevin hiçbir şey belirlemediği yerde geçerlidir. |
| **Bir grup** | Grubun görevlerinin, tüm çalıştırmaları boyunca ve/veya herhangi bir 24 saatte birlikte ne kadara mal olabileceği. | Görevin hiçbir şey belirlemediği yerde, alan alan grubun her görevine uygulanır. |
| **Tek görev** | Bu görevin çalıştırmalarının birlikte ne kadara mal olabileceği. | Görevin kendi değerleri, alan alan. |

Sınırlarda bir görevin kendi değeri grubunkinin, bir grubunki de her görev
için olanın önüne geçer. Bütçelerde ise **bir çalıştırmanın üzerindeki her
bütçe geçerlidir ve en sıkı olanı belirler**.

Uygulamada *Settings → Budget* ve *Settings → Limits* her seferinde tek bir
kapsamı gösterir; kapsam **Applies to** altında seçilir (Every task, One
group, One task). Kart kapsamı büyük harflerle adlandırır (EVERY TASK,
GROUP, THIS TASK ONLY), böylece bir değişiklik asla istemediğiniz yere
düşmez. Bir görevin ya da grubun ⋮ menüsü doğru kapsamı açar.

## Bütçeler

```sh
kule budget TASK 5           # bu görevin çalıştırmaları birlikte 5 USD'ye mal olabilir
kule budget TASK 4.50 EUR    # başka bir para birimi (ISO 4217)
kule budget TASK off         # bu görev için bütçe yok
kule budget TASK             # bir sonraki çalıştırmasının üzerindeki her bütçeyi listele

kule group budget ID 20          # bir grup: görevlerinin tüm çalıştırmaları boyunca
kule group budget ID 10 --daily  # bir grup: herhangi bir 24 saatte

kule daily-budget 20         # tüm görevler birlikte, herhangi bir 24 saatte
```

Bir bütçe nasıl uygulanır:

- Üzerindeki bir bütçe harcandığında **hiçbir çalıştırma başlamaz**.
- Bir çalıştırma, kalanların en sıkısıyla sınırlanır. Maliyetini çalışırken
  bildiren bir ajanın çalıştırması, oraya vardığında **iptal edilir**.
- Bir ret, harcanan bütçenin adını verir.
- **Bir çalıştırma, ajanı başlamadan önce payını ayırır**: harcayabileceği
  en fazla tutar; görevin, grubunun, grubun günlük ve günlük bütçeden
  kalanların en sıkısı. Diğer çalıştırmaların ayırdığı harcanmış sayılır;
  böylece aynı bütçe altında aynı anda başlayan çalıştırmalar kalanı
  paylaşır. Kule payları süreçleri arasında birer birer ayırır (Kule'nin
  evindeki `budget.lock` altında) ve her birini görevin `reserved.jsonl`
  dosyasında tutar.
- Çalıştırmaların ayırdığından başka bir şey kalmadığında hiçbir çalıştırma
  başlamaz; ret, bütçenin ve görevlerin adını verir:
  ``what is left of <budget> (<amount>) is set aside for runs at work, or that did not end, in task <id>: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end``.
- Bir çalıştırmanın kullanımı kaydedildiğinde payı kapanır: bundan sonra
  yalnızca harcadığı sayılır.
- **Kullanımı hiç gelmeyen bir pay tamamen sayılır**: Kule durdurulmuştur ya
  da çalıştırmanın harcadığı kaydedilememiş veya okunamamıştır. Günlük bir
  bütçede pay 24 saatle birlikte düşer. Görevin bütçesinde ve grubun tüm
  çalıştırmalar üzerindeki bütçesinde, siz onu `kule budget TASK --release`
  ile bırakana kadar kalır; görevde bir ajan çalışırken bu reddedilir.
  Bıraktığınız, `usage.jsonl` dosyasında kayıtlı kalır.
- Bir pay, ayrıldığı sırada görevin bulunduğu grup için sayılır.
- Yazılamayan bir pay hiçbir çalıştırma başlatmaz:
  `Kule could not set aside what the run may spend (…), so it does not start`.
- Bir çalıştırma, görevinin **çalıştığı sırada** bulunduğu grup için
  sayılır; böylece bir görevi gruptan çıkarmak, harcadığını grubun
  hesabında bırakır.
- **Okunamayan bir kayıt sayılır**; hiç harcanmamış sayılmaz. Kule onun ne
  zaman ve hangi grup için yazıldığını bilemez; bu yüzden sayılabileceği her
  bütçe çalıştırmaları tutar: görevin kendi bütçesi, şu an bulunduğu grubun
  iki bütçesi ve günlük bütçe. Ret, bütçenin adını ve kaç kayıt olduğunu
  verir:
  `Kule cannot tell what <budget> has left: <N> record(s) of what runs spent … cannot be read; mend or remove them, or turn the budget off, to run again`.
  Bir çökme yüzünden yarım kalan bir kayıt, bir sonraki çalıştırmanın
  kaydını da beraberinde götürmez.

:::warning Bir bütçe yalnızca ajanların bildirdiğini sınırlar

- **Claude Code** maliyeti her turun sonunda bildirir; bu yüzden bir tur
  bütçeyi aşabilir ve reddedilen bir sonraki çalıştırma olur.
- ChatGPT ile oturum açmış **Codex** token'ları bildirir ama maliyeti
  bildirmez.
- **Gemini** ikisini de bildirmez.

Abonelikle oturum açmış bir ajan hiç maliyet bildirmeyebilir; bir bütçe
bildirilmeyeni sınırlayamaz.

:::

Her çalıştırmanın ne kullandığı (token'lar, bağlam penceresi ve ajan
söylediğinde maliyet) görevin `usage.jsonl` dosyasında tutulur.
`kule status` ve uygulamanın görev başlığı bunları toplar. Okunamayan
kayıtlar orada da sayılır: `spent:` satırlarına ` (N records cannot be read)`
eklenir, başlıktaki etiket de kırmızıyla *N usage records unreadable · budget held* der.
Ayrılanlar da gösterilir: `spent:` satırları ve bütçe listeleri
` (<amount> set aside for runs at work or that did not end)` ekler,
`kule status --json` bunu `spent` altında `held` ve `held_by` olarak verir,
başlıktaki etiket de *<amount> set aside* der ve bunu kalandan düşer.

Pencere başka yerdeyken uygulama, bir bütçenin %80'i harcandığında ve
tamamı harcandığında size haber verir.

## Sınırlar

```sh
kule limits                                # her görevin sınırları ve her birinin nereden geldiği
kule limits memory=4 cpu=2                 # bunları her görev için ayarla
kule limits --group ID memory=8 gpu=off    # bir gruptaki her görev için
kule limits --task TASK run=30 idle=5      # tek bir görev için
kule limits --task TASK memory=inherit     # grubunkini / her görevinkini yeniden al
kule limits cpu=none                       # işlemci sınırı yok
```

| Alan | Birim | Aşılınca |
| --- | --- | --- |
| `cpu` | çekirdek (`1.5` = bir buçuk) | Çalıştırma durdurulmaz, **yavaşlatılır**: Kule süreçlerini sırayla duraklatıp sürdürür (`SIGSTOP`/`SIGCONT`, her seferinde saniyenin onda biri kadar). |
| `memory` | GB, çalıştırmanın tüm süreçleri birlikte | Süreçler **hemen öldürülür**; çalıştırma durur. |
| `disk` | Görev açıldığından beri kopyanın, geçici dosyalarının ve ev değişikliklerinin büyüyebileceği GB | Çalıştırma **durur**. Hiçbir dosya bunun ötesine yazılamaz. |
| `gpu` | `on` / `off` | Kapalıyken: macOS'ta Metal'e (ve OpenCL'e) hiçbir aygıt verilmez; Linux'ta `/dev/dri` ve NVIDIA'nın aygıtları salt okunur kalır. |
| `servers` | `on` / `off` (varsayılan olarak kapalı) | macOS: kapalıyken, ağ trafiği proxy'den geçerken çalıştırmanın programları bağlantı dinlemez; açıkken dinlerler; bu, kabul ettiğiniz daha zayıf yalıtımdır: Node, Python ve Ruby sunucuları bu bilgisayara yönlendirilir, Kule de diğerlerini sandbox'ın dışından ağınızdan uzak tutar (bkz. [`kule isolation`](../cli/isolation.md)). Linux'ta her sunucu, ne derse desin bu bilgisayarla sınırlı tutulur. |
| `run` | dakika, tek çalıştırma | Çalıştırma **durur**. |
| `task` | saat, bir görevin tüm çalıştırmaları birlikte | Çalıştırma **durur** ve siz yükseltene kadar hiçbiri başlamaz. |
| `idle` | hiçbir şey yapmadan geçen dakika | Çalıştırma **durur**. |

Nasıl sayıldıklarına dair notlar:

- **Bellek**, macOS'ta süreçlerin ayak izi (footprint), Linux'ta orantılı
  paylarıdır.
- **Disk** yalnızca görevin eklediğini sayar ve bir tarama elverdiği kadar
  sık (en fazla saniyede bir) ölçülür. Büyük bir klasör kendi sınırını
  tüketmez; görevin başladığı paket önbellekleri de tüketmez. Bu
  önbellekler, disk sınırı altındaki ilk çalıştırma başlarken bir kez
  ölçülür ("Measuring the task's package caches for its disk limit");
  önbellekleriniz büyükse bu biraz sürebilir. O andan sonra görevin onlara
  eklediği sayılır. Disk sınırı yoksa hiçbir şey onları taramaz ve bir
  çalıştırmanın gösterdiği disk rakamı onları dışarıda bırakır.
- **Hiçbir şey yapmamak**; ajandan mesaj, araç çağrısı ya da çıktı
  gelmemesi, işlemci kullanılmaması ve yeni süreç açılmaması demektir. Uzun
  ve sessiz bir derleme hiçbir şey yapmamak değildir. Sizi bekleyen bir soru
  da sayılmaz.
- **Tek çalıştırma**, yanıtlarınızı beklemek dahil çalıştırmanın tüm
  süresini sayar.
- Hiçbir sistem GPU'nun bir payının ayarlanmasına izin vermez; bu yüzden
  `gpu` yalnızca açık ya da kapalıdır.
- macOS sandbox'ı bir sunucunun dinlediği port'u görür ama adresini görmez;
  bu yüzden `servers` de yalnızca açık ya da kapalıdır.
- Bir çalıştırma başladığı sınırları korur; bir değişiklik bir sonraki
  çalıştırmadan itibaren geçerlidir.
- Terminalden `kule run` da aynı sınırlara tabidir.

Bir sınır bir çalıştırmayı durdurduğunda Live hangisi olduğunu ve neyi
ölçtüğünü söyler:

```text
Stopped: the run's processes held 4.2 GB, over its memory limit of 4 GB
```

Bir çalıştırma sürerken istemin altındaki çubuk, sınırlarına göre ne
aldığını gösterir (`CPU 140% / 200% · 1.2 GB / 4 GB · disk +350 MB ·
7 processes`) ve işlemci sınırı onu geri tuttuğu sürece *slowed*
(yavaşlatıldı) ekler.

## Stop

Uygulamadaki **Stop** (Mod+Shift+K), çalışmakta olan çalıştırmayı
istediğiniz an durdurur: turu iptal edilir ve ajan turu on saniye içinde
bitirmezse öldürülür. O ana kadar değiştirdikleri, incelemeniz için kopyada
kalır.

## İlgili sayfalar

- [`kule budget`](../cli/budget.md), [`kule daily-budget`](../cli/daily-budget.md), [`kule limits`](../cli/limits.md), [`kule group`](../cli/group.md)
- [Gruplar](groups.md)
- [Live](../app/live.md), [Settings](../app/settings.md)

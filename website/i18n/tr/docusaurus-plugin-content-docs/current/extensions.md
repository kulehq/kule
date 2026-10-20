---
title: Kule eklentisi yazmak
sidebar_label: Eklentiler
description: Sandbox'lanmış bir JavaScript eklentisiyle Kule'ye komutlar, paneller, temalar, ayarlar, inceleme notları, ajan profilleri ve MCP sunucuları ekleyin.
---

# Kule eklentisi yazmak

:::note Bir önizleme

Eklenti biçimi (`"kule_extension": 1`) ve SDK, Kule 1.0'dan önce hâlâ
değişebilir. Eklentileri bozan her değişiklik, ne yapılması gerektiğiyle
birlikte [changelog'da](https://github.com/kulehq/kule/blob/main/CHANGELOG.md)
belirtilecek. Sizi neyin engellediğini bize söyleyin:
[issues](https://github.com/kulehq/kule/issues).

:::

Eklentileri terminalden kurmak, onaylamak ve yönetmek için bkz.
[`kule ext`](cli/ext.md). Uygulamada **Settings → Extensions**'ı kullanın
(bkz. [Settings](app/settings.md)).

Bir eklenti Kule'ye şunları ekler: komutlar, paneller, temalar, ayarlar,
bir incelemeye notlar, ajan profilleri ve ajanlar için MCP sunucuları. Bir
klasördeki düz JavaScript ve HTML'dir; bir `.kulext` dosyası olarak
paylaşılır.

Kule bir eklentiye bir ajana davrandığı gibi davranır: güvenmediği kod
olarak. Eklenti, kapalı bir listeden izin ister. Kişi tam olarak gördüğü
sürümü onaylar ve her değişiklik yine onu bekler. Eklentinin kodu,
Kule'den başka hiçbir şeye ulaşamayan sandbox'lanmış çerçevelerde (frame)
çalışır ve Kule yalnızca bu izinlerin kapsadığı çağrıları yanıtlar.

## Başlangıç {#start}

```sh
kule ext new me.hello        # bir klasör: manifest, ana betik, panel, tipler
kule ext dev hello           # klasörden kur, canlı okunur
kule ext approve me.hello    # çalışmasına izin ver
```

Paneli komut paletinden (`Show Hello`) açın ya da sekmesini istediğiniz
yere sürükleyin. Siz yazarken kod değişiklikleri olduğu gibi çalışır;
`kule-extension.json`'daki bir değişiklik ise yeniden onay bekler.

`kule ext approve`, neyi onaylayacağını gösterir ve önce sorar; bir betik
`--yes` verir. Uygulama açıkken `kule ext` değişikliklerini daemon üzerinden
yapar; böylece uygulama bunları hemen duyar.

Klasör, yalnızca sizin yazabildiğiniz bir yerde olmalıdır. Kodu onaysız
çalıştığı için Kule, bir ajanın yeniden yazabileceği klasörleri reddeder:
`/tmp`, `/var/tmp`, `/dev/shm`, Kule'nin ev dizini ya da bir ajanın kendi
dizinleri altındakiler.

Paylaşmak için:

```sh
kule ext pack hello          # me.hello-0.1.0.kulext
kule ext info me.hello-0.1.0.kulext
kule ext install me.hello-0.1.0.kulext
```

Masaüstü uygulaması aynısını **Settings → Extensions** altında yapar.

[`examples/extensions/`](https://github.com/kulehq/kule/tree/main/examples/extensions)
klasöründe okumanız için üç örnek var:

| Örnek | Ne gösterir |
| --- | --- |
| `hello` | Bir panel, bir komut, bir ayar ve bir tema |
| `lockfile-notes` | Bir inceleme yardımcısı: değişen lockfile'ları not eder |
| `webhook-notify` | Görevleri izlemek ve bir webhook'a göndermek |

## Dil ve araçlar {#language-and-tools}

Bir eklenti Node'da değil, pencerenin web motorunda çalışır (macOS'ta
WebKit, Linux'ta WebKitGTK): modern JavaScript, DOM ve CSS. `require`
yoktur; SDK'nın sunduklarından başka dosya sistemi ve ağ da yoktur.

- **Betikler:** klasik `<script src="…">` ve ES modülleri (`<script
  type="module">`, `import "./util.js"`) ikisi de çalışır. İçe aktarmalar
  yalnızca eklentinin kendi dosyalarından gelir.
- **TypeScript:** `.js`'e derleyin (`tsc` ya da `esbuild` ile) ve `.js`'i
  gönderin. Tipler için `kule ext new`'in yazdığı `kule-ext.d.ts`'i
  `tsconfig.json`'ınıza ekleyin ya da bir dosyanın en üstüne
  `/// <reference path="kule-ext.d.ts" />` koyun. Bir `.ts` dosyası pakette
  bulunabilir ama çalışmaz.
- **npm paketleri:** bir çerçevenin ağı ve yalın (bare) içe aktarmaları
  yoktur; bu yüzden `import x from "lodash"` başarısız olur. Onları kendi
  dosyanızda paketleyin (bundle), örneğin `esbuild src/main.ts --bundle
  --format=esm --outfile=main.js`.
- **Satır içi kod yok:** sayfanın politikası betiklere ve stillere yalnızca
  eklentinin dosyalarından izin verir. Bu yüzden satır içi `<script>`,
  `style="…"` niteliği, `eval` ya da `new Function` yoktur. Bir betikten
  `element.style` ayarlamak sorun değildir.
- **Tarayıcı depolaması yok:** `localStorage`, çerezler ve IndexedDB opak
  bir origin'i reddeder. `kule.storage`'ı kullanın.
- **Dosyalar:** html, js, css, json, png, svg, woff2, toml, md, txt, ts ya
  da uzantısız bir ad. Adlar harf, rakam, `-`, `_` ve `.` içerir. Gizli
  dosyalar (`.git`, `.DS_Store`) paketlerken dışarıda bırakılır.

## Manifest: `kule-extension.json` {#the-manifest-kule-extensionjson}

```json
{
  "kule_extension": 1,
  "id": "acme.notes",
  "name": "Notes",
  "version": "1.0.0",
  "publisher": "acme",
  "description": "One sentence.",
  "main": "main.js",
  "permissions": ["tasks.read", "net"],
  "hosts": ["hooks.slack.com"],
  "contributes": {
    "commands": [{ "id": "scan", "title": "Scan", "keys": "Mod+Alt+S" }],
    "panels": [{ "id": "notes", "title": "Notes", "entry": "panel.html", "needsTask": true }],
    "configuration": [
      { "key": "level", "title": "Level", "type": "enum", "values": ["info", "warn"], "default": "warn" }
    ],
    "themes": ["dusk.kule-theme.json"],
    "agents": "agents.toml",
    "mcpServers": [{ "name": "db", "command": "npx", "args": ["-y", "@acme/db-mcp@1.2.3"] }]
  }
}
```

Kule her alanı denetler. Bilinmeyen bir alan ya da izin eklentinin
reddedilmesine yol açar; hiçbir şey yok sayılmaz.

| Alan | Kurallar |
| --- | --- |
| `id` | `publisher.name`; her parça `a-z 0-9 -` karakterlerinden 1–20 tane; `publisher` ile başlar |
| `publisher` | "not verified by anyone" (kimse tarafından doğrulanmadı) olarak gösterilir |
| `name`, `description` | Denetim ya da yazı yönü karakterleri içermeyen metin (40 ve 300 karakter) |
| `version` | `X.Y.Z`, isteğe bağlı bir `-pre` ya da `+build` ile |
| `main` | Uygulama açıkken gizli bir çerçevede çalışan bir `.js` dosyası; komutlar için gereklidir |
| `hosts` | Düz host adları (`api.example.com`); `net` ile ve yalnızca onunla |
| komut ve panel id'leri | `a-z 0-9 -` karakterlerinden 1–18 tane. Bir panelin id'si aynı zamanda onu gösteren komutun id'sidir; bu yüzden bir komut ve bir panel aynı id'yi paylaşamaz |
| `configuration` | `string`, `boolean`, `number` ya da `enum` tipinde en fazla 32 ayar. Formu Kule çizer ve değerleri Kule saklar |

Manifest'teki yollar düzdür: harfler, rakamlar, `-`, `_` ve `.`; `..` ve
gizli adlar olmadan.

## İzinler {#permissions}

| İzin | Eklentiye şunu sağlar |
| --- | --- |
| `tasks.read` | Görevleri görmek: adlar, klasörler, kaç değişiklik olduğu, bir ajanın çalışıp çalışmadığı. Bütçeler, harcamalar ya da ev dizini değişiklikleri değil |
| `events.read` | Ajanlar çalışırken söylediklerini ve yaptıklarını, görevler oluşturulduğunda, commit edildiğinde, değişiklikleri atıldığında, geri alındığında ya da kaldırıldığında duymak |
| `questions.read` | Kişiyi bekleyen soruları görmek. Onları yanıtlayamaz |
| `review.read` | Bir görevin değişikliklerini ve politikanın kararlarını okumak. Kişinin ev dizininde bekleyenleri değil |
| `review.annotate` | Bir görevin incelemesine not eklemek. Yalnızca not |
| `files.read` | Bir görevin kopyasındaki dosyaları okumak |
| `storage` | Kendine ait en fazla 1 MiB JSON saklamak |
| `notify` | Eklentinin adıyla, dakikada 5 bildirim göstermek |
| `net` | Listelenen `hosts`'lara https üzerinden `fetch` |

Hiçbir izin şunlara ulaşamaz:
- gizli değerler;
- bir soruyu yanıtlamak;
- commit, değişiklikleri atmak (roll back) ya da undo;
- politikalar, kurallar, bütçeler ya da ayar dosyaları;
- ajan onayları;
- terminal;
- daemon'un token'ı.

Onay ekranı, bir eklenti hem kişinin işini okuyup hem de dışarı veri
gönderebildiğinde uyarır.

## SDK {#the-sdk}

Bir eklentinin her sayfası Kule'nin SDK'sını yanından yükler:

```html
<script src="_kule/sdk.js"></script>
```

`kule ext new`'in yazdığı `kule-ext.d.ts` tipleri içerir. Her çağrı bir
promise döndürür. Reddedilen bir çağrı, nedenini söyleyen bir `Error` ile
reddedilir; örneğin `files.read needs the files.read permission, which
Notes was not given`.

```js
await kule.ready;                          // bağlandı
kule.extension;                            // { id, name, version }
kule.context;                              // { main: true } ya da { panel, task }

kule.commands.register("scan", (args) => { /* … */ });
await kule.commands.execute("ext.acme.notes.scan");
await kule.commands.execute("view.files"); // Kule'nin yalnızca bir panel gösteren birkaç komutu (aşağıda)

kule.events.on("task", ({ task, change }) => { /* … */ });

await kule.tasks.list();                   // tasks.read
await kule.tasks.get(id);
await kule.review.get(task);               // review.read
await kule.review.annotate(task, [{ path, level: "warn", text }]); // review.annotate
await kule.files.list(task, "src");        // files.read
await kule.files.read(task, "README.md");
await kule.storage.get();                  // storage
await kule.storage.set({ seen: 3 });
await kule.config.get();                   // eklentinin kendi ayarları
await kule.ui.notify("Title", "Body");     // notify
await kule.net.fetch("https://hooks.slack.com/…", { method: "POST", headers, body }); // net
```

### Olaylar {#events}

| Olay | Veri | Gereken |
| --- | --- | --- |
| `run` | `{ task, seq, event }`: bir ajanın olayları | `events.read` |
| `task` | `{ task, change }`: `created`, `committed`, `rolled_back`, `undone`, `removed`, `went_back`, `synced` | `events.read` |
| `questions` | Şu an bekleyen sorular | `questions.read` |
| `review` | `{ task, digest }`: yeni değişikliklerle bir inceleme çizildi | `review.read` |
| `task-selected` | `{ task }`: kişi başka bir görev seçti | — |
| `theme` | `{ scheme, colors }`. SDK `--kule-<part>` CSS değişkenlerini zaten ayarlamıştır (`--kule-bg`, `--kule-text`, `--kule-accent`, …) | — |
| `config` | `{ key, value }`: kişi bir ayarı değiştirdi | — |

Olaylar yalnızca masaüstü uygulaması açıkken gelir.

## Yaşam döngüsü {#lifecycle}

- **Kod ne zaman çalışır:** bir eklenti, masaüstü uygulaması açıkken ve
  eklenti açık ve şimdiki haliyle onaylanmışken çalışır. Uygulama kapalıyken
  hiçbir şey çalışmaz ve o sırada olan olaylar yeniden oynatılmaz.
- **Ana betik:** uygulamanın başlangıcından (ya da onaydan) eklenti
  kapatılana, kaldırılana ya da değiştirilene kadar gizli bir çerçevede
  çalışır. Bir değişiklik, eklenti yeniden onaylanana kadar onu durdurur.
- **Paneller:** her panelin sayfası, sekmesi gizliyken bile başlangıçta
  yüklenir. Bir paneli başka bir bölgeye taşımak sayfasını yeniden yükler.
- **Her yükleme tazedir:** sayfa yeni bir bağlantı alır ve `kule.ready`
  yeniden çözülür (resolve). Kalıcı olması gerekenleri değişkenlerde değil
  `kule.storage`'da tutun. `kule.context.task`, sayfa yüklendiğinde seçili
  olan görevdir; sonrakiler için `task-selected`'i dinleyin.
- **Siz yazarken:** `kule ext dev` ile kurulan bir klasör canlı okunur.
  Settings → Extensions'taki kartında **Reload** vardır; bu, çerçevelerini
  kodun şimdiki haliyle yeniden başlatır.

## Hata ayıklama {#debugging}

- **İletiler:** her çerçevenin `console.log`, `info`, `warn` ve `error`
  çağrıları, yakalanmamış hatalar ve işlenmemiş promise retleri pencereye
  gider. Eklentinin Settings → Extensions'taki kartı son 100 tanesini, en
  yenisi önce olmak üzere listeler. Bunlardan biri bir hata olduğunda liste
  kendiliğinden açılır.
- **Reddedilen çağrılar:** reddedilen bir çağrı, nedenini söyleyen bir
  `Error` ile reddedilir. Onu yakalayıp `console.error` ile yazdırın; orada
  da görünür.
- **İnceleyici (inspector):** Kule'nin bir geliştirme derlemesinde
  (`cargo run -p kule-desktop`) Tauri, WebKit'in inceleyicisini açar. Bir
  panelin içinde sağ tıklayın ve Inspect Element'i seçin.
- **Kurmadan önce:** `kule ext info FOLDER` manifest'i Kule'nin okuyacağı
  gibi okur; `kule ext pack FOLDER` da bir paketin taşıyamayacağı her şeyi
  reddeder.

## Katkılar {#contributions}

- **Komutlar**, komut paletinde ve Settings → Shortcuts'ta
  `ext.<id>.<command>` olarak görünür. Kule'nin ya da başka bir eklentinin
  zaten kullandığı önerilen bir tuş düşürülür; kişi komuta bir tuş
  verebilir.
- **Paneller**, pencerenin bölgelerinde Kule'nin kendi panelleri gibi durur;
  yeni bir panel sağa gider. Kule her birinin üstüne, eklentinin adını
  veren ve çerçevenin üzerine boyayamayacağı bir çizgi çizer.
- **Temalar**, Kule'nin kendi temaları gibi denetlenen, yalnızca renk
  içeren `.kule-theme.json` dosyalarıdır. Appearance'ta "From extensions"
  altında listelenirler.
- **Bir incelemeye notlar**, eklentinin adıyla "From extensions" altında
  görünür. Düz metindir ve hiçbir şeyi onaylayamaz ya da reddedemez.
- **Ajan profilleri** (`agents.toml`, `kule agents show claude-code`
  biçiminde), kişinin kendi profilleriyle aynı denetimlerden geçer. Her biri
  onaylanana kadar Settings → Agents'ta (ya da
  `kule agents approve NAME` ile) bekler.
- **MCP sunucularının** her biri, kişi ajanların onu kullanmasına izin
  verene kadar bekler; eklentinin kartında ya da
  `kule ext mcp approve EXT/NAME` ile. Kart her değişkenin değerini
  gösterir.
  - Bir sunucu tek bir kesin kayıt (registry) paketini başlatır, başka
    hiçbir şeyi değil: `npx [-y] name@1.2.3 …` ya da `uvx name==1.2.3 …`.
  - Paketten önceki seçenekler (`--with`, `--from`, `--package`,
    `--registry`, …), URL'ler, yollar, git tanımları, takma adlar (alias) ve
    ekstralar (extras) reddedilir.
  - Bir paket yöneticisini ya da bir yorumlayıcıyı yönlendiren değişkenler
    (`NPM_*`, `UV_*`, `PIP_*`, `PYTHON*`, `NODE_*`, `GIT_*`, …) de
    reddedilir.
  - Ajan sunucuyu kendi sandbox'ının içinde ve görevin proxy'sinin
    arkasında başlatır. Sunucu, ajanın oturumu dahil, ajanın okuyabildiği
    her şeyi okuyabilir ve politika eriştiği her host için soru sorar.

## Bir çerçevenin yapabildikleri ve yapamadıkları {#what-a-frame-can-and-cannot-do}

Kule bir eklentinin dosyalarını `127.0.0.1`'den rastgele bir anahtar
altında sunar. Çerçeveleri sandbox'lıdır (yalnızca `allow-scripts`, opak
bir origin) ve içerik politikaları onları kendi dosyalarıyla sınırlar:
- ağ yok (`connect-src 'none'`);
- kendine ait çerçeve yok;
- form yok;
- `localStorage` ya da çerez yok (`kule.storage`'ı kullanın);
- aygıt, pano ya da tam ekran yok (`Permissions-Policy`).

Pencere, çerçevelerin yalnızca eklenti sunucusunun tam port'undan
gelmesine izin verir; böylece kendini başka bir yere yönlendiren bir
çerçeve bilgisayardaki başka bir sunucuya ulaşamaz. Pencerede WebRTC
kapalıdır: içerik politikası onu yönetemez. WebRTC kapanmadan hiçbir
eklenti çalışmaz.

Tauri için oradaki bir çerçeve, Kule'nin komutlarının hiçbirine ulaşamayan
uzak bir origin'dir. Bir çerçeve Kule ile yalnızca yüklendiğinde kendisine
verilen MessagePort üzerinden konuşur. O port üzerinden gelen şey, çerçeve
kendini ne olarak tanıtırsa tanıtsın, port'un yapıldığı eklentiye aittir.

Bir çerçeve yalnızca eklentinin kendi komutlarını ve Kule'nin bir panel ya
da bölge gösteren kısa bir komut listesini çalıştırabilir. Liste şudur:
`view.tasks`, `files`, `live`, `review` ve `timeline`, `chat.toggle`, ve
`layout.left/main/bottom/right`. Bu hem `commands.execute` için hem de
çerçevenin içinde basılan bir kısayol için geçerlidir. Bunlardan herhangi
birinden sonra klavye çerçeveye geri döner. Bir çerçeve terminali
başlatamaz, paleti açamaz, commit edemez, değişiklikleri atamaz ya da
silemez.

`kule` yayıncısı ve ilk sözcüğü "Kule" olan adlar Kule'nin kendisine
aittir; hiçbir eklenti onları alamaz.

Bir eklenti pencereyi kullanılamaz hale getirirse, uygulamayı
`--no-extensions` ile (ya da `KULE_NO_EXTENSIONS=1` ile) başlatın ve
eklentiyi, uygulama olmadan da çalışan `kule ext disable ID` ile kapatın.

## Paket {#the-package}

Bir `.kulext`, Kule'nin diske hiçbir zaman açmadığı bir zip'tir. Bir paket
kurulduğunda her girdisi denetlenir:
- eklentilerin taşıdığı türden düz bir yol: html, js, css, json, png, svg,
  woff2, toml, md, txt, ts ya da uzantısız bir ad;
- bağlantı (link) ve şifreleme yok;
- dosya listesi tutarlı: hiçbir ad iki kez yok ve başlıklar birbiriyle
  uyuşuyor;
- açıldığında dosya başına en fazla 5 MiB ve toplamda 40 MiB, paketli
  halde 20 MiB, 500 dosya.

Onay, paketin tamamının blake3 özetini sabitler. Kule bir dosyayı sunmadan
önce bunu yeniden denetler. `kule ext pack` aynı klasörden aynı baytları
üretir; böylece kişinin onayladığı özet, tam olarak okuduğu şeyi adlandırır.

## Paylaşmak ve güncellemek {#sharing-and-updating}

Henüz bir pazar yeri (marketplace) yok. `.kulext` dosyasının kendisini
paylaşırsınız (bir indirme, bir release eki). Kişi onu Settings →
Extensions → Install from a file üzerinden ya da `kule ext install FILE` ile
kurar.

- **Özet (digest):** `kule ext pack` onu yazdırır. İnsanlar kurmadan önce
  `kule ext info FILE` ile denetleyebilsin diye onu dosyanın yanında
  yayımlayın. Yayıncıyı kimse doğrulamaz; dosyanın sizin olduğunu kanıtlayan
  şey özettir.
- **Güncellemek:** `version`'ı artırın, yeniden paketleyin ve yeni dosyayı
  paylaşın. Onu eskisinin üzerine kurmak eklentinin depolamasını ve
  ayarlarını korur (yeni manifest'te artık olmayan bir ayar düşürülür).
  Kişi yeniden onaylayana kadar eklentiyi bekletir ve onay, yeni olan her
  izni ya da host'u belirtir.
- **Kimlik:** id, eklentinin kimliğidir. Aynı id'ye sahip bir paket, onu
  kim yapmış olursa olsun onun bir güncellemesidir; bu yüzden size ait bir
  yayıncı adı seçin.

## Bir bakışta sınırlar {#limits-at-a-glance}

| Ne | Sınır |
| --- | --- |
| Paket | Paketli 20 MiB; açılmış 40 MiB; 500 dosya; dosya başına 5 MiB |
| Manifest | 64 KiB; 50 komut, 8 panel, 32 ayar, 8 tema, 8 MCP sunucusu, 16 host |
| Bir çerçeveden gelen istek | 1 MiB |
| Çağrılar | Eklenti başına her 10 saniyede 200; tuşlar ve iletiler dahil |
| `storage` | 1 MiB JSON |
| `ui.notify` | Dakikada 5; başlık 80 karakter, gövde 300 |
| `net.fetch` | Dakikada 60; listelenen host'lara 443 üzerinden https; 15 sn; dışarı 1 MiB, içeri 4 MiB; yönlendirme (redirect) yok |
| Bir incelemeye notlar | Eklenti başına görev başına 50, her biri 500 karakter |
| Saklanan iletiler | Eklenti başına son 100, her biri 2.000 karakter |

## Neler nerede {#where-things-are}

`<KULE_HOME>/extensions/` şunları tutar:

| Yol | Ne tutar |
| --- | --- |
| `installed.json` | Neyin kurulu, açık ya da kapalı olduğu |
| `approved.json` | Kişinin onayladıkları |
| `mcp-approved.json` | Ajanların kullanabileceği MCP sunucuları |
| `packages/` | Paketler |
| `data/<id>/storage.json` | Bir eklentinin sakladıkları |
| `data/<id>/config.json` | Ayarları |
| `data/<id>/network.jsonl` | Her fetch için bir satır: yöntem, host, durum, boyut. Asla yol ya da sorgu değil |

Bir eklentiyi kaldırmak, onaylarıyla birlikte onun tamamını kaldırır.

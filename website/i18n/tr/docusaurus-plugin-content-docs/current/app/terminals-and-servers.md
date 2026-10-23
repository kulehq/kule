---
title: Terminaller ve sunucular
sidebar_label: Terminaller ve sunucular
description: Bir görevin kopyasında, sandbox'ında kendi kabuğunuzu açın ve bir görevin çalıştırdığı sunuculara tarayıcınızdan erişin.
---

# Terminaller ve sunucular

## Terminaller {#terminals}

**Terminal** paneli, görevin kopyasında açtığınız kendi kabuklarınızı tutar;
istediğiniz kadar açabilirsiniz, her biri bir sekmedir.

| Eylem | Tuşlar |
| --- | --- |
| Terminali göster ya da gizle | Ctrl+\` |
| Başka bir terminal aç | Ctrl+Shift+\` |
| Kopyayı terminal uygulamanızda, sandbox dışında aç (görevin ⋮ menüsünde *Open in terminal app*) | Mod+T |
| Terminalde metin bul | macOS'ta ⌘F, diğerlerinde Ctrl+Shift+F |
| Bir http ya da https bağlantısını aç | ⌘/Ctrl+tık |
| Sonraki, önceki terminal; terminali temizle ya da kapat | varsayılan yok; *Settings → Shortcuts* altında tuş atayın |

:::note Terminal görevin sandbox'ında çalışır

Terminal, oturum kabuğunuzu (`$SHELL -l`) kopyada, [`kule exec`](../cli/exec.md)
bir komutu nasıl çalıştırıyorsa öyle, **görevin sandbox'ının içinde**
çalıştırır; panelin köşesinde *in the sandbox* yazar. İş kontrolü (job
control) ve Ctrl+C her zamanki gibi çalışır.

- Yalnızca kopyaya ve görevin tmp klasörüne yazar.
- Ev klasörü, görevin ev katmanıdır: `~` içine yazdıkları **Home** altında
  incelemeniz için yakalanır.
- Ağı, görevin proxy'sinden geçer ve politikaya göre değerlendirilir.
  Politikanın size soracağı bir sunucu reddedilir, çünkü terminalden kimseye
  sorulmaz.
- Ajanların kendi durumu ve kimlik bilgileri ondan gizlenir.

Oradan kopyada değiştirdikleriniz, ajanın işi gibi incelenir ve commit
edilir.

:::

- Linux ve Windows'ta kabuktan yalnızca Ctrl+Shift kısayolları ve fonksiyon
  tuşları çıkar; böylece Ctrl+L ve diğerleri kabuğa ait kalır.
- Bir görevdeki ilk terminalin açılması, görevin paket önbellekleri
  hazırlanırken birkaç saniye sürebilir.
- Görevde bir ajan çalışırken Kule **terminal açmayı reddeder**.
- **Bir görevde terminal açıkken** o görevde hiçbir ajan çalıştırması
  başlamaz; **Commit**, **Sync**, **Roll back**, **Undo**, bir adıma geri
  dönmek ve ev değişikliklerini uygulamak ya da geri almak bekler. Kule
  orada çalışanı sonlandırmak yerine *a terminal is open on this task's
  copy; close it first* der; böylece kaydetmediğiniz hiçbir şey kaybolmaz.
  Önce terminalin sekmesini kapatın (ya da `exit` yazın), sonra devam edin.
  [`kule shell`](../cli/shell.md) de terminal sayılır.

### Sandbox'ın dışında {#outside-the-sandbox}

*Open in terminal app* (Mod+T) kopyayı kendi terminal uygulamanızda,
*Open the copy in your editor* (⇧⌘O, diğerlerinde Ctrl+Shift+O) ise
editörünüzde açar. İkisi de sandbox'ın **dışında**, sizin kimliğinizle
çalışır; bu yüzden Kule önce sorar:

- Görevde bir ajan çalışırken ve görev kopyada kendiliğinden çalışacak bir
  şeyi değiştirdiği sürece reddedilirler: git config ya da hook'lar
  (prompt'unuzdaki `git status` bunları çalıştırırdı), `rust-toolchain.toml`
  ya da `.nvmrc` gibi bir sürüm dosyası, bir `package.json`'ın adını
  verdiği paket yöneticisi, `.envrc` ya da bir editörün
  ayarları. Önce bunları inceleyip commit edin ya da değişiklikleri atın.
  Bkz.
  [kendiliğinden çalışanlar](../concepts/git.md#what-runs-by-itself-protects-your-terminal-too).
- Kopyada commit edilmemiş değişiklikler varken Kule değişen dosyaları
  listeler ve kopyanın orada açılıp açılmayacağını sorar. Elinizden gelirse
  önce onlara Review'da bakın, sonra *Open in …* ya da *Cancel*'ı seçin.
  Her seferinde sorar.
- Kule kopyayı o uygulamada açtığını hatırlar. Uygulamanın kapandığını
  göremediği için **görevdeki bir sonraki çalıştırmadan önce onu
  kapatmanızı ister**: uygulamayı kapatın, sonra *I closed it, start the
  run*'ı seçin. Komut satırından [`kule closed TASK`](../cli/closed.md)
  çalıştırın.

Terminal uygulaması, [`KULE_TERMINAL`](../reference/environment.md)'ın
adını verdiği ya da sisteminizinkidir.

## Görevin çalıştırdığı sunucular {#servers-the-task-runs}

Görevin kopyasındaki bir şey bir port'u dinlediğinde (ajanın
`npm run dev`'i ya da görevin terminalinde sizin başlattığınız bir sunucu),
görev başlığında `localhost:5173 ↗` gibi bir **etiket** (chip) görünür.
Sunucuyu tarayıcınızda açmak için üzerine tıklayın.

Ajanın başlattığı sunucular yalnızca bu bilgisayarda dinler: ağınızdaki
hiçbir şey (bir kafenin Wi-Fi'ı, ofis LAN'ı) ajanın çalıştırdığı şeye
ulaşamaz. Görevin terminalinde sizin başlattığınız sunucular aynı
sandbox'ta çalışır; macOS'ta tek bir fark var, aşağıda.

- **Linux**'ta Kule, program ne olursa olsun her dinleme adresini
  loopback'e çevirir.
- **macOS**'ta sandbox, bir sunucunun dinlediği port'u görür ama adresini
  asla görmez; bu yüzden onu bu bilgisayarla sınırlı tutamaz. Bir görevin ağ
  trafiği proxy'den geçerken, onun için sunuculara izin vermediğiniz sürece
  programları **hiç dinlemez**: bu, Settings → Limits'te ya da
  `kule limits --task ID servers=on` ile ayarlanan `servers`
  [sınırıdır](../concepts/budgets-and-limits.md). Reddedilen bir sunucu,
  komuttan sonra, ona nasıl izin verileceğiyle birlikte listelenir.
  Sunuculara izin vermek daha zayıf yalıtımdır; bunu Policy altında kabul
  edersiniz (bkz. [`kule isolation`](../cli/isolation.md)). İzin verildikleri
  sürece:
  - Node, Python ve Ruby sunucuları, her adresi dinlemek istediklerinde
    loopback'e yönlendirilir;
  - diğer programlar (Go, Rust, Java) için Kule port'u **korur** (shield) ve
    etikette 🛡 görünür: ajanın çalıştırmalarında, görevin terminallerinde,
    `kule exec`'te ve `kule shell`'de aynı şekilde; sunucu, onu ne başlatmış olursa olsun,
    görevin sandbox'ından bulunur;
  - kendine ait bir ağ adresini dinleyen bir sunucu durdurulur; Kule
    bakmadan hemen önceki anda ağdan bir bağlantının ulaştığı bir sunucu da
    durdurulur; Live nedenini söyler.

Etiketler ayrıca neyin dikkatinizi gerektirdiğini de söyler:

| Etiket | Anlamı |
| --- | --- |
| Düz, ↗ ile | Yalnızca bu bilgisayardaki bir sunucu. Açmak için tıklayın. |
| 🛡 ile (macOS) | Port'unu Kule'nin ağdan koruduğu bir sunucu. |
| **Uyarılı kehribar** | Ağın hâlâ erişebildiği bir sunucu; genellikle kendi başlattığınız bir sunucu: Kule'nin sandbox'ı dışında (örneğin terminal uygulamanızda). Kule sizinkilere dokunmaz. |
| **Gri** (Linux) | Sandbox'ın kendi ağının içindeki, bu bilgisayarın erişemediği bir sunucu. |

Ajanın başlattığı bir sunucu, çalıştırmasıyla birlikte sona erer. Görevin
terminalinde başlattığınız bir sunucu ise o terminal kapanınca sona erer.

## Terminalden {#from-a-terminal}

[`kule shell TASK`](../cli/shell.md), kabuğunuzu panelin terminali gibi
kopyada, görevin sandbox'ında açar; `kule shell TASK --outside` ise onu
sandbox'ın dışında, *Open in terminal app* ile aynı ret durumları ve aynı
soruyla açar. Kule'nin penceresinin kopyayı açtığı bir terminal
uygulamasını ya da editörü kapattıktan sonra
[`kule closed TASK`](../cli/closed.md) çalıştırın.
[`kule path TASK`](../cli/path.md), `cd "$(kule path TASK)"` için kopyanın
nerede olduğunu yazdırır ve orada bir şeyin görevin değiştirdiklerini
çalıştıracağı durumlarda stderr'e uyarı yazar.

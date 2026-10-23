---
title: Bölüm bölüm Settings
sidebar_label: Settings
description: Ajanlardan ve politikadan temalara, kısayollara, eklentilere ve profillere kadar Kule'nin Settings ekranının her bölümü ve her denetimin ne yaptığı.
---

# Bölüm bölüm Settings

Settings'i **Mod+,** ile ya da pencerenin sol altındaki dişliyle açın.
Bölümler üstte sekmeler halinde durur; dar bir pencerede sekme satırı yana
kayar.

| Bölüm | Ne işe yarar |
| --- | --- |
| [Agents](#agents) | Kule'nin çalıştırabildiği ajanlar ve sizin ekleyip onaylayacağınız ajanlar. |
| [Policy](#policy) | Tüm görevler için balanced ya da careful, her grup için bir politika ve bu bilgisayardaki yalıtım. |
| [Rules](#rules) | Kule'ye bir daha sormamasını söyledikleriniz. |
| [Secrets](#secrets) | Bir görevin komutlarının görmeden kullandığı API anahtarları. |
| [Budget](#budget) | Ajan çalıştırmalarının neye mal olabileceği: tüm görevlerin, bir grubun, tek bir görevin. |
| [Limits](#limits) | Bir çalıştırmanın bu bilgisayardan ne kadar kullanabileceği. |
| [Appearance](#appearance) | Temalar, hareket (motion), yazı tipleri ve kopyalama. |
| [Shortcuts](#shortcuts) | Her komut ve tuşları. |
| [Extensions](#extensions) | Kurma, onaylama, açma ya da kapatma, kaldırma. |
| [Profile](#profile) | Başka bir bilgisayara götürmek için tercihleriniz tek bir dosyada. |

## Agents {#agents}

Kule'nin kendi ajanları (Claude Code, Codex, Gemini) ve sizin
`~/.kule/agents.toml` dosyasındaki ajanlarınız. Her birinin neye ihtiyaç
duyduğu için bkz. [Ajanlar](../using-agents.md).

- Her ajan açılıp kapanan bir karttır. Kapalı bir kart etiketlerini, not
  sayısını ve komutunu gösterir. Bekletilen (onaylanmamış ya da bir
  denetimden geçemeyen) ya da eksik (programı kurulu olmayan) bir ajan açık
  başlar; böylece nedenini görürsünüz. **Open all** / **Close all** tüm
  kartlara uygulanır.
- **Add from the ACP registry**,
  [ACP kayıt listesindeki](https://agentclientprotocol.com/registry)
  ajanları listeler ve birinden taslak bir profil alır; **Add by hand**
  doldurmanız için boş bir profil açar.
- **Try it**, bir taslağı boş bir klasörde, taslaktan daha geniş
  açılmadan bir kez çalıştırır ve sandbox'ın ona reddettiklerini onay
  kutuları olarak listeler: yazmak istediği dizinler ve erişmek istediği
  host'lar. Almasını istediklerinizi işaretleyin (bir telemetri host'u
  dışarıda kalabilir). Linux reddedilen dosyaları bildirmez; bu yüzden
  orada yalnızca host'lar önerilir.
- Eklediğiniz bir ajan yalnızca, açtığı şeyleri (kendi klasörleri, host'ları
  ve anahtarları) siz **onayladıktan** sonra ve üzerinde yapılan her
  değişiklikten sonra yeniden onayladığınızda çalışır.

## Policy {#policy}

Neyin kendiliğinden geçtiği, neyin sizi beklediği ve neyin hiçbir zaman
geçmediği. Bkz. [Politika](../concepts/policy.md).

- **For every task (tüm görevler için)**: **Balanced** (varsayılan) ya da
  **Careful**. `~/.kule/policy.cedar` içindeki kendi politikanız bunların
  yerine geçer; Kule onun üzerine hiçbir zaman yazmaz.
- **Groups (gruplar)**: her [grup](../concepts/groups.md), *Every task's*
  politikasıyla (varsayılan), geri kalanlar için ne geçerli olursa olsun
  Kule'nin **Balanced** ya da **Careful** politikasıyla, veya **Its own…**
  ile, yani grup için yazdığınız ve saklanmadan önce denetlenen bir
  politikayla değerlendirilir. Bir deponun retleri (kendi
  `.kule/policy.cedar` dosyasında) ve Kule'ye bir daha sormamasını
  söyledikleriniz yine geçerlidir. Bir değişiklik, görevin bir sonraki
  commit'inden ve çalıştırmasından itibaren sayılır.
- **Isolation on this computer (bu bilgisayardaki yalıtım)**: bilgisayarın
  ajanlara tam yalıtımlarını veremediği yerlerde (yetkisiz kullanıcı
  namespace'leri olmayan Linux), Policy neyin eksik olduğunu listeler ve siz
  **Run agents with what this computer gives** kutusunu işaretleyene kadar
  hiçbir ajan çalışmaz. Bu ayar geçerli olduğu sürece başlık çubuğunda
  *Weaker isolation* yazar. Bkz. [Platformlar](../platforms.md).

## Rules {#rules}

Kule'ye bir daha sormamasını söyledikleriniz: bir bağlantıdaki *Approve and
remember* ya da Review'da bekletilen bir dosyadaki *Don't ask again in this
folder* (bu, önce neleri geçireceğini gösterir). Her kuralın bir
**Forget** düğmesi vardır (onaylamak için yeniden tıklayın); bundan sonra
Kule yeniden sorar.

Reddedilen bir değişiklik hiçbir zaman burada olmaz; bir deponun kendi
kurallarını tuttuğu `.kule/` altındaki hiçbir şey de burada olmaz.

## Secrets {#secrets}

Bir görevin komutlarının görmeden kullandığı API anahtarları. Bkz.
[Gizli değerler](../concepts/secrets.md).

Birini saklamak için şunları doldurun:

| Alan | Ne yazılır |
| --- | --- |
| **Name** | Komutların onu bulacağı değişken, örneğin `STRIPE_KEY`. |
| **Goes to** | Gidebileceği host'lar, boşluk ya da virgülle ayrılmış; örneğin `api.stripe.com`. |
| **Value** | Gizli değerin kendisi. Kule'nin ev dizininde, yalnızca sizin okuyabileceğiniz şekilde saklanır; bir daha hiç gösterilmez. |
| **For** | Tek bir klasör ya da tüm klasörler. |

Ardından **Keep secret**'a basın. Bir görevdeki komut `$STRIPE_KEY` içinde
bir yer tutucu bulur; görevin proxy'si gerçek değeri yalnızca bu gizli
değerin host'larına giden isteklere, HTTPS üzerinden koyar, başka hiçbir
yere koymaz. Bir klasörün `.env` dosyalarında gizli değere benzeyen değerler
de aynı şekilde kopyanın dışında tutulur.

Bölüm, değerlerin nerede mühürlendiğini söyler: sisteminizin anahtar
zincirinin (macOS keychain) ya da Secret Service'in (Linux) tuttuğu bir
anahtarla; bunlardan biri olmayan ya da Kule ev dizini `~/.kule` dışında
olan bir bilgisayarda ise Kule'nin ev dizinindeki bir dosyada tutulan bir
anahtarla. Her gizli değerin bir **Forget** düğmesi vardır (onaylamak için
yeniden tıklayın).

## Budget {#budget}

Ajan çalıştırmalarının, ajanlarının bildirdiği kadarıyla, neye mal
olabileceği. Bkz. [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md).

**Applies to**, her seferinde bir kapsam seçer: **Every task** (tüm
görevler), **One group** (bir grup) ya da **One task** (tek bir görev;
ardından hangisi olduğu). Aşağıdaki kart kapsamı büyük harflerle adlandırır
(EVERY TASK, GROUP, THIS TASK ONLY), kenarı o kapsamın rengindedir ve orada
ayarladığınız şeyin kimi bağladığını söyler. Her değişiklik kapsamın adıyla
onaylanır.

| Kapsam | Ne ayarlarsınız |
| --- | --- |
| Every task | Tüm görevlerin herhangi bir 24 saat içinde toplam neye mal olabileceği. |
| One group | Grubun görevlerinin tüm çalıştırmaları boyunca ve herhangi bir 24 saat içinde toplam neye mal olabileceği; ikisi de isteğe bağlıdır. |
| One task | O görevin çalıştırmalarının toplam neye mal olabileceği. *What holds its next run*, onun üzerindeki her bütçeyi listeler ve bağlayıcı olanı işaretler. |

Çalıştırmaların harcadıklarına dair okunamayan bir kayıt, düzeltilene ya da
kaldırılana veya bütçe kapatılana kadar sayılabileceği her bütçeyi tutar;
görev başlığındaki etiket kırmızıyla *N usage records unreadable · budget held* der.

Çalıştırmaların başlarken ayırdığı, her çalıştırmanın kullanımı gelene kadar
harcanmış sayılır. *Spent* ve *Last 24 h* satırları bunu söyler
(*· X set aside for runs at work or that did not end*) ve kalandan düşer.
Bitmeyen bir çalıştırmanın payını bırakmak için komut satırında
[`kule budget TASK --release`](../cli/budget.md) kullanın.

Sınır olmaması için bir alanı boş bırakın. *Where budgets are set*, bütçesi
olan her kapsamı listeler; açmak için bir kapsama tıklayın. Bir görevin ya
da grubun ⋮ menüsü (*Budget…*) kendi kapsamını doğrudan açar.

## Limits {#limits}

Her çalıştırmanın bu bilgisayardan ne kadar kullanabileceği. İşlemci
sınırının üstünde bir çalıştırma yavaşlatılır; bellek, disk ya da süre
sınırının üstünde durdurulur ve Live nedenini söyler.

Budget'takiyle aynı **Applies to** seçicisi vardır. Bir grup ya da görev
için her alan, değerinin nereden geldiğini söyler:

| Kapsam | Her alan şu olabilir |
| --- | --- |
| Every task | Bir değer ya da sınır olmaması için boş. |
| One group | Grubun her görevi için **From every task** (varsayılan), **No limit** ya da **The group's own** değeri. |
| One task | Miras aldığı değer (grubu o alanı ayarlıyorsa **From group …**, yoksa **From every task**), **No limit** ya da **Its own** değeri. |

Alanlar şunlardır: işlemci (çekirdek), bellek (GB), disk (GB), **GPU may be
used** (açık ya da kapalı), **Servers (macOS) may listen** (varsayılan olarak
kapalı; bkz. [Terminaller ve sunucular](terminals-and-servers.md)), tek bir
çalıştırma (dakika), bir görevin tüm çalıştırmaları (saat) ve hiçbir şey
yapmama (dakika). *Groups and tasks
with limits of their own*, bir şey ayarlayan her kapsamı listeler. Süren bir
çalıştırma, başladığı sınırları korur. Her birinin nasıl ölçüldüğü için bkz.
[Bütçeler ve sınırlar](../concepts/budgets-and-limits.md).

## Appearance {#appearance}

- **Themes (temalar)**: Kule'nin kendi temaları (bilgisayarı izleyen
  System, Light, Dark, Coral, Nord, Solarized Light ve iki yüksek kontrastlı
  tema), sizinkiler ve eklentilerin getirdikleri. Bunlardan herhangi
  birinden canlı önizlemeyle bir tema oluşturun; Kule okunamayacak kadar
  soluk renk çiftleri için uyarır. Bir tema bir `.kule-theme.json` dosyasına
  aktarılır ve oradan alınır; bu dosya pencerenin adlandırılmış parçaları
  için renkler (`#rrggbb`, alınırken denetlenir) taşır, başka hiçbir şey
  taşımaz.
- **Motion (hareket)**: bir şey değiştiğinde görülen küçük hareketler:
  menüler onları açan şeyden büyüyerek çıkar, sekme satırı kayar, Live'a
  yeni bir olay yükselir, bir soru açılır ve iki kez nabız gibi atar, görev
  satırları yeni yerlerine kayar, iyi biten bir çalıştırma onay işaretini
  çizer. **As the computer says** (varsayılan), bilgisayarın "hareketi
  azalt" ayarı açıkken bunu kapatır; **On** ve **Off** kendi başlarına karar
  verir.
- **Fonts (yazı tipleri)**: pencerenin metni için bir tane, kod için (ajan
  sohbeti, terminaller, diff'ler ve Preview'daki kod) bir tane; her biri
  bilgisayarınızdaki yazı tiplerinden ve kendine ait bir boyutla: metin 11
  ile 20 px arası (Kule'ninki 14), kod 10 ile 22 px arası (13). Kod listesi
  eş aralıklı (monospace) yazı tiplerini içerir; herhangi bir yazı tipinin
  adını yazabilirsiniz. **Kule's fonts** ikisini de geri getirir. Mod+= ve
  Mod+- iki boyutu birlikte değiştirir. Bir profilin getirdiği ama bu
  bilgisayarda olmayan bir yazı tipi Kule'nin kendisine geri döner ve içe
  aktarma bunu söyler.
- **Copying (kopyalama)**: ajan sohbetinde, Review'da ya da bir terminalde
  *Copy text when you select it with the mouse* (fareyle seçtiğiniz metni
  kopyala); işaretçinin yanında küçük bir *Copied* görünür. Yazı
  yazdığınız alanlar her zamanki gibi kopyalar.

Kullanılan tema ve kendi temalarınız Kule'nin ev dizinindeki `themes.json`
dosyasında saklanır.

## Shortcuts {#shortcuts}

Her komut ve tuşları. Ada ya da tuşlara göre **arayın** ("theme" ya da "⌘⇧"
gibi), kendi tuşlarınızı ayarlamak için bir komuta tıklayın ya da **Put back
every default** ile tüm varsayılanları geri getirin. Tercihleriniz Kule'nin
ev dizinindeki `shortcuts.json` dosyasında saklanır. Bkz.
[Klavye](keyboard.md).

## Extensions {#extensions}

Eklentiler Kule'ye komutlar, paneller, temalar ve yardımcılar ekler. Her
biri sandbox'lanmış bir çerçevede (frame) çalışır ve yalnızca sizin
onayladığınız şeyleri yapabilir; üzerinde yapılan bir değişiklik yine sizi
bekler. Hiçbiri bir soruyu yanıtlayamaz, commit edemez, değişiklikleri
atamaz, gizli değerlerinizi okuyamaz ya da bir politikayı değiştiremez.

- **Install from a file…** bir `.kulext` kurar; **Load a folder you are
  writing…** üzerinde çalıştığınız bir klasörü kurar ve canlı okur.
- Her eklenti istediği izinleri sade sözcüklerle listeler (örneğin *Read
  files in a task's copy* ya da *Send and receive data over the internet,
  to the hosts listed*); **Approve**, **Turn on** / **Turn off** ve
  **Remove** (onaylamak için yeniden tıklayın) düğmeleriyle.
- Açık bir eklenti *Runs* der; pencerede hiçbir eklenti çalışamıyorken ise
  nedeniyle birlikte *Does not run now* der: Kule bu sefer eklentisiz
  başladı (güvenli mod), eklentiler sunulamıyor ya da pencerenin WebRTC'si
  kapanana kadar bekliyorlar.
- Bir eklentinin ajanlara sunduğu **MCP sunucularının** her biri kendi
  onayını bekler: **Let agents use it** ya da **Stop offering it**. Bir ajan
  böyle bir sunucuyu kendi sandbox'ının içinde başlatır.
- Bir eklentinin getirdiği ajan profilleri sizi *Settings → Agents* altında
  bekler.

Bkz. [Eklentiler](../extensions.md).

## Profile {#profile}

Tercihleriniz tek bir `.kule-profile.json` dosyasında; hepsi ya da
işaretlediğiniz kısımlar: Kule'yi başka bir bilgisayarda buradaki gibi
kurmak ya da bir kopyasını saklamak için.

| Kısım | Ne tutar |
| --- | --- |
| Appearance | Kullanılan tema, kendi temalarınız, hareket, kopyalama ve yazı tipleri. |
| Shortcuts | Kendi ayarladığınız kısayollar. |
| Policy | Kule'nin balanced ya da careful politikası. |
| Daily budget | Tüm görevlerin bir günde toplam neye mal olabileceği. |
| Limits | Tüm görevlerin çalıştırmalarının ne kadar kullanabileceği: işlemci, bellek, disk, GPU, süre. |
| Your agents | Eklediğiniz ajan profilleri; diğer bilgisayarda onay beklerler. |

**İçe aktarırken** Kule, bir şey değişmeden önce dosyanın ne içerdiğini
gösterir ve uygulanacak kısımları siz seçersiniz. Temalar sizinkilere
katılır; geri kalanlar profildekini alır. Gizli değerler, hatırlanan
onaylar ve kendi yazdığınız bir politika hiçbir zaman bir profile girmez.

## Terminalden {#from-a-terminal}

[`kule agents`](../cli/kule-agents.md), [`kule policy`](../cli/policy.md),
[`kule group policy`](../cli/group.md), [`kule isolation`](../cli/isolation.md),
[`kule rules`](../cli/rules.md), [`kule secrets`](../cli/secrets.md),
[`kule budget`](../cli/budget.md), [`kule daily-budget`](../cli/daily-budget.md),
[`kule limits`](../cli/limits.md) ve [`kule ext`](../cli/ext.md) aynı şeyleri
terminalden yapar. Görünüm, kısayollar ve profiller yalnızca uygulamada
vardır.

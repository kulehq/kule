---
title: Kule'nin güvenlik modeli
sidebar_label: Güvenlik modeli
description: Kule neyi, kime karşı koruyor, sınırları nerede ve bir güvenlik açığı nasıl bildirilir.
---

# Kule'nin güvenlik modeli

Kule, Zero Trust (sıfır güven) üzerine kuruludur ve OWASP'ı izler: Top 10,
Application Security Verification Standard (ASVS) ve LLM ile ajan
uygulamaları için Top 10. Bu sayfa Kule'nin neyi, kime karşı koruduğunu ve
sınırların nerede olduğunu anlatır.

:::warning Bir güvenlik açığını bildirmek

Açığı, [SECURITY.md](https://github.com/kulehq/kule/blob/main/SECURITY.md)'nin
söylediği gibi gizli olarak bildirin (deponun **Security** sekmesi →
**Report a vulnerability**; onu yalnızca bakımcılar görür). Asla herkese
açık bir issue, pull request ya da tartışmada bildirmeyin.

:::

## Neler korunuyor {#what-is-protected}

- **Klasörleriniz**: bir ajanın yaptığı hiçbir şey, politika ya da siz izin
  verene kadar onlara ulaşmaz; ulaşan da incelenen şeydir.
- **Ev dizininiz ve diğer projeleriniz**: bir ajan bir kopyada, bir
  sandbox'ın içinde çalışır; bilgisayarın geri kalanı, platformun izin
  verdiği ölçüde onun erişiminin dışındadır.
- **Gizli değerleriniz**: API anahtarları ve token'lar Kule'de kalır; ajan
  yer tutucularla çalışır.
- **Ağınız**: bir ajan, Kule'nin proxy'si üzerinden yalnızca politikanın
  izin verdiği host'lara ulaşır, başka hiçbir şeye ulaşmaz.
- **Siz**: sorular, incelemeler ve onaylar orada gerçekten ne olduğunu
  gösterir; hiçbir şey sizin yerinize yanıt vermez ya da onaylamaz.

## Kime güvenilmiyor {#who-is-not-trusted}

Varsayılan olarak hiçbir şeye güvenilmez:

- **Ajana**; yazdığı ya da yazdırdığı her şeye, başlattığı her sürece ve
  yaptığı her isteğe. Ajanın okuduğu bir dosyadaki bir prompt injection'ın
  başarılı olacağı varsayılır; tutunan şey Kule'nin sınırlarıdır.
- Kule'nin önizleme ya da inceleme için açtığı **dosyalara** ve ajanın
  onlara koyduklarına: adlar, içerikler, bağlantılar ve hedefleri.
- **İçe aktarılan profillere ve temalara**, **eklentilere** ve **MCP
  sunucularına**: her biri girişte denetlenir ve çalışmadan önce sizin
  tarafınızdan onaylanır.
- **Diğer yerel kullanıcılara ve süreçlere** ve **ağa**.

## Sınırlar {#the-boundaries}

| Sınır | Onu ne tutuyor |
|---|---|
| Ajan ↔ klasörünüz | Copy-on-write (yazınca kopyala) bir kopya; etkiler taranır, Cedar politikasıyla değerlendirilir (hatalar ret demektir) ve yalnızca incelendiği haliyle commit edilir. Bir deponun kendi politikası yalnızca ret ekleyebilir. |
| Ajan ↔ bilgisayar | Sandbox (yalıtılmış ortam): macOS'ta Seatbelt; Linux'ta Landlock, seccomp ve namespace'ler; Windows'ta bir süreç konteyneri. Yazma yalnızca kopyada ve görevin geçici klasörlerinde; gizli değerler ve kimlik bilgileri gizli; daha zayıf bir sandbox, bu söylenmeden hiçbir zaman kullanılmaz. |
| Ajan ↔ ağ | Her bağlantı Kule'nin proxy'sinden geçer; proxy politikanın adlandırdığı host'lara izin verir, bağlandığı adresi denetler ve özel (private) adresleri erişim dışında tutar. |
| Ajan ↔ gizli değerler | Ajanın ortamında yer tutucular; proxy bunları yalnızca dışarı giderken gizli değerin kendi host'una giden isteklerde doldurur ve geri gelende gizler. |
| Pencere ↔ daemon | Yalnızca sizin kullanıcınızın açabildiği bir soket, bir token, boyut sınırları. |
| Pencere ↔ güvenilmeyen metin | Yalnızca DOM çağrıları ve `textContent` ile oluşturulur; satır içi betiklere izin vermeyen sıkı bir içerik güvenlik politikası (CSP); bağlantılar yalnızca http(s) olarak açılır. |
| Eklentiler ↔ Kule | Kendi origin'lerinde sandbox'lı çerçeveler; her çağrı onayladığınız izinlere karşı denetlenir; ağ yalnızca onaylanmış herkese açık host'lara. |
| Sürüm ↔ siz | Her dosya için checksum'lar, derleme kaynağı kanıtı (build provenance) ve bir CycloneDX SBOM; sabitlenmiş bir araç zincirinden (`rust-toolchain.toml`); macOS'ta, notlarında da söylendiği gibi ad hoc imzalı, hiçbir zaman noter onaylı (notarized) değil. Sürüm workflow'unun hiçbir işi (job) gizli bir değer tutmaz; yayımlama bir bakımcının onayını bekleyen bir ortamda çalışır ve ondan önce hiçbir şey yayımlanmaz. |

### Bir görevin komutlarının hâlâ okuyabildikleri {#what-a-tasks-commands-can-still-read}

Projelerinizin, oturumlarınızın ve uygulamaların verilerinin bulunduğu
yerlerde okuma varsayılan olarak reddedilir (bkz. [Sandbox](concepts/sandbox.md#what-is-hidden));
bilgisayarın geri kalanı, araçlar ihtiyaç duyduğu için okunabilir kalır:

- sistem: `/usr`, `/bin`, `/sbin`, `/etc`, `/opt`, `/Library`, `/System`,
  `/Applications` ve benzerleri;
- ev dizininizdeki araç zincirleri: `~/.cargo`, `~/.rustup`, `~/.nvm`,
  `~/.pyenv`, `~/.dotnet` ve `~/.gradle` gibi nokta klasörleri, Kule'nin
  içlerinde gizlediği kimlik bilgileri hariç;
- `~/Library`'nin geri kalanı (`Caches`, `Developer`, …), gizli olarak
  listelenenler hariç;
- `PATH`'inizin gösterdiği klasörler, nerede olurlarsa olsunlar;
- görevin kendi kopyası, geçici klasörü ve ev katmanı.

Ev dizininizin dışında olup Kule'nin hiç açmadığı ya da ev dizininizin
yalnızca bağlantıyla gösterdiği bir proje de okunabilir. Ajanlarla üzerinde
çalıştığınız bir projeyi ev dizininizde tutun ya da Kule'de açın; böylece
diğerlerinden gizlenir.

## Kule'nin kapsamadığı şeyler {#what-kule-does-not-cover}

Kule yalnızca siz karar verene kadar dünyaya ulaşmadan bekletebildiği
etkileri tutar: klasörünüzdeki bir değişiklik commit'i, bir bağlantı
proxy'yi bekler. Kopyası, diff'i ve geri alması olmayan bir eylem, örneğin
ekranınızdaki bir uygulamada bir tıklama, bir tuş vuruşu ya da gönderilen
bir mesaj, bekleyemez. Bu yüzden Kule onu tasarım gereği desteklemez:

- **Bir görevin içinde reddedilir.** Sandbox, komutları ve ajanın başlattığı
  MCP sunucularını, platformun izin verdiği ölçüde ekrandan, klavyeden,
  panodan ve diğer uygulamalardan uzak tutar.
- **Araç çağrıları için bir kapı yoktur.** Bir araç çağrısı, ajanın yapmak
  üzere olduğu şeyi anlatmasıdır ve Kule bu anlatıma güvenmez. Onun yerine
  aracın kendi bildirimine güvenmek, bu güveni üçüncü taraf koda devretmek
  olurdu.
- **Bir görevin dışında Kule hiçbir şeyi korumaz.** Kule'nin dışında
  çalıştırdığınız bir ajan ya da araç, ekranı süren bir MCP sunucusu da
  dahil, Kule açıkken bile incelenmez.

## Katkıda bulunanlar için {#for-contributors}

Her değişiklik şu kurallara uyar:

- **Varsayılan olarak reddet.** Ret listeleri yerine izin listeleri; karar
  veremeyen bir denetim reddeder.
- **Hiçbir şey sessizce zayıflamaz.** Daha zayıf bir mod reddedilir ya da
  kişi onu bilerek seçer ve geçerli olduğu sürece görür.
- Her süreç, token, izin ve CI işi için **en az yetki**.
- **Her sınırın, o sınır olmadan başarısız olan bir testi vardır**; asla
  mock'larla değil, gerçek süreçler ve gerçek dosya sistemi üzerinde.
- **Bir değişiklik bitmeden önce**, dokunduğu OWASP kategorilerini gözden
  geçirin: erişim denetimi, injection ve XSS, gizli değerler, SSRF, commit
  edilen ya da kurulan şeyin bütünlüğü, tedarik zinciri, kayıt (logging).
- Düzeltme yayımlanana kadar **herkese açık yerlerde güvenlik açığı
  ayrıntısı yok** (commit'ler, CHANGELOG, issue'lar, dokümanlar).

Bir sürümden önce, CI'ın yanı sıra `scripts/release-gate.sh` macOS'ta ve
Linux'ta çalışır: workflow'lar actionlint ve zizmor'dan, lockfile'lar
osv-scanner'dan (kabul edilen tavsiyeler (advisory), `deny.toml`'daki gibi
`osv-scanner.toml`'da), derlenen ikili dosya `cargo audit bin`'den
(cargo-auditable ile derlenmiş) geçer; ayrıca bir çalışma klasörünün env
dosyalarına yerleştirilen kanarya gizli değerler, bir görevde maskelenmiş
olmalı ve çalışma klasörü dışında hiçbir yerde bulunmamalıdır. Her araç
sabitlenmiş bir sürümde çalışır ve hiçbiri taradığı şeyi hiçbir yere
göndermez. Sürüm derlendikten sonra `scripts/release-gate.sh attest <tag>`,
her dosyanın kaynak kanıtını release workflow'una ve etikete (tag) karşı
doğrular. CI, workflow ve kanarya denetimlerini her değişiklikte çalıştırır.

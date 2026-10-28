---
title: Platformlar
sidebar_label: Platformlar
description: Kule'nin macOS, Linux ve Windows'ta neleri desteklediği, sandbox'ın üçü arasında nasıl farklılaştığı ve henüz neyin yapılmadığı.
---

# Platformlar

Kule **macOS**, **Linux** ve **Windows 11**'de çalışır. Sandbox üçünde
farklı bir mekanizmadır (Seatbelt, ad alanlarıyla Landlock, bir süreç
konteyneri) ve üçünde de aynı kural geçerlidir: bir komut Kule'nin verdiği
dışında hiçbir şeyi okumaz (bkz. [`kule tools`](cli/tools.md)); Windows'ta
da Kule'nin verdiği dışında hiçbir şeyi
okuyamaz. Bir komutun nereye yazabildiği, ağının nereden geçtiği ve
klasörünüze neyin ulaştığı üçünde de aynıdır.

## Bir bakışta destek

| | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Sürümler | macOS 13 ya da sonrası | x86_64 sürüm dosyaları (Ubuntu 22.04'te derlenir, Fedora'da test edilir); diğer mimariler kaynak koddan | Windows 11 24H2, 26100.9278 ya da sonraki güncellemeyle; x64 ve ARM64 |
| Sürüm dosyaları | Universal `.dmg` (Apple silicon ve Intel) | `.deb`, `.rpm`, `.tar.gz` olarak yalnızca `kule` | Kullanıcıya özel bir kurucu (`-setup.exe`); `kule` ve iki programı `.zip` olarak |
| Sandbox | Seatbelt | Landlock (Linux 6.2 ya da sonrası), seccomp, ad alanları (namespaces) | Her komutun çevresinde bir süreç konteyneri (Microsoft'un `mxc-sdk`'sı) |
| Sandbox içinde ağ kuralları | Evet | Linux 6.7 ya da sonrası | Evet |
| Sandbox'ın reddettikleri listelenir | Evet, komuttan sonra ve `sandbox.jsonl` içinde | Hayır (Landlock yalnızca denetim kaydına (audit log) bildirir); komut yine de bir izin hatası alır | Hayır; komut bir erişim hatası alır |
| Görevin kopyası | APFS klonu: neredeyse hiç yer kaplamaz | Btrfs ve XFS'te reflink; ext4'te tam kopya | ReFS'te ve Dev Drive'da blok klonu; NTFS'te tam kopya |
| Başlangıçta paket önbellekleri | npm ve cargo sizinkilerden klonlanır | Boş: ilk kurulum yeniden indirir | Boş: ilk kurulum yeniden indirir |
| `/tmp` | Kapalı: `TMPDIR` görevin kendine ait | Her görevin kendine ait | `TEMP` ve `TMP` görevin kendine ait |
| Görevin ev dizini | Dosyalarınız görevin ev dizinine bağlanır, yazmalar inceleme için yakalanır | Aynı | Boş, görevin kendine ait: profilinizden hiçbir şey okunmaz ya da katmanlanmaz |
| Kendine ait süreçler | – | Evet: bir komut başka hiçbir programı görmez ve ona sinyal gönderemez | Komutla birlikte biter: başlattığı hiçbir şey ondan sonra yaşamaz |
| Kendine ait ağ ad alanı | – | Evet, erişebildiği tek şey proxy | Konteynerin kendi ağı: kapalı, açık ya da yalnızca proxy |
| Sunucular bu bilgisayarla sınırlı | Görevin `servers` sınırı izin vermedikçe hiçbiri dinlemez (kabul ettiğiniz daha zayıf yalıtım); izin verildiğinde Node, Python ve Ruby loopback'e yönlendirilir, diğer programların portları korunur (shield) | Dinlenen her adres loopback'e çevrilir | İçerideki bir sunucu yalnızca görevindir: bu bilgisayardan ya da ağdan hiçbir şey ona ulaşmaz |
| GPU kapalıyken | Metal (ve OpenCL) hiçbir aygıt alamaz | `/dev/dri` ve NVIDIA aygıtları salt okunur | – |
| Gizli değer deposunun anahtarı | Keychain | Secret Service | Credential Manager |
| Dosya yöneticisinde göster | Finder | Files (D-Bus `FileManager1`) | – |

## Windows: sandbox'ın ihtiyaç duydukları

- **Windows 11 24H2 (yapı 26100), 26100.9278 ya da sonraki güncellemeyle**,
  ya da daha yeni bir Windows. Kule'nin kullandığı süreç konteyneri o
  güncellemeyle geldi. Windows Server kapsanmaz: oradaki konteyner bir yolu
  gizleyemez.
- **Bir masaüstü oturumu.** Bir hizmetten ya da uzak bir kabuktan (SSH,
  zamanlanmış görev) bir görevin çalıştırdığı programlar başlayamaz.

İkisinden biri eksikse sandbox'lı komutlar **gerekçesiyle reddedilir**;
terminalde ve *Settings → Policy* altında. Windows'ta kabul edilecek daha
zayıf bir mod yoktur: konteyner bir komuta yalıtımının tamamını verir ya da
hiçbirini, ve hiçbiri daha zayıf bir sandbox değildir.

Konteynerin içinde neyin geçerli olduğu, standart bir kullanıcı hesabında
ve güvenlik duvarı varsayılanlarındayken ölçüldü (depodaki
[`docs/windows-measurements.md`](https://github.com/kulehq/kule/blob/main/docs/windows-measurements.md)):

- Bir komut yalnızca verileni okur: görevin kopyası ve geçici klasörü
  (yazılabilir), Windows, Program Files, `PATH`'inizdeki araçlar ve araç
  zincirlerinin kendilerini kurduğu klasörler (`.cargo`, `.rustup`,
  `%LOCALAPPDATA%\Programs`, npm'in ve pnpm'in klasörleri, …). Profiliniz,
  diğer projeleriniz, Credential Manager, pano ve kayıt defteri erişim
  dışıdır; bir hakka giden yoldaki klasör listelenir ama okunmaz, yolun
  yanındakiler listelenmez.
- Bir görevin komutları, görevin klasörlerinde **kendilerine ait, boş bir
  ev dizini** alır: `USERPROFILE`, `APPDATA`, `TEMP` ve `TMP` orayı
  gösterir; `HOMEDRIVE` ve `HOMEPATH` geçirilmez. Profilinizden hiçbir şey
  okunmaz ya da katmanlanmaz; `kule home`'un gösterecek bir şeyi yoktur.
- Ağ, görevin politikasının dediği gibi kapalıdır, açıktır ya da yalnızca
  proxy'nindir. Bir görevin **proxy'si kendine ait bir süreç olarak**,
  `kule-proxy.exe`, görevin gizli değerlerini tutan, ağa ulaşan ve sizin
  hiçbir şeyinize ulaşamayan bir uygulama konteynerinde (app container)
  çalışır; proxy'yi atlayan bir bağlantı anında başarısız olur. Kule hiçbir
  güvenlik duvarı kuralı yazmaz.
- **Terminal** (`kule shell` ve uygulamanın terminalleri) kabuğu, Kule'nin
  programlarının yanında duran yardımcısı `kule-pty-helper.exe` aracılığıyla
  görevin konteynerinin içinde bir sahte konsolda (pseudo console) açar.
  Yardımcısı bir komutun yazabileceği bir klasörde, Kule'nin komutlardan
  gizlediği bir klasörde (Kule'nin ev dizini, çalışma alanı) ya da
  profilinizi tutan bir klasörde duran bir terminali Kule reddeder.
- **Kule'nin ev dizini** (`%USERPROFILE%\.kule`) ile programların klasörü
  (kurucu için `%LOCALAPPDATA%\Kule`) ayrı kalmalıdır: bir komut terminal
  yardımcısının klasörünü okuyabilir, Kule'nin ev dizinini ise asla okumaz.
  `KULE_HOME` programların klasörünün içine ayarlanmışsa ya da `.zip`
  Kule'nin ev dizinine açılmışsa komutlar bu gerekçeyle reddedilir.
- Konteynerin içindeki **Windows PowerShell**, her uygulama konteynerinin
  getirdiği kilitleme politikasıyla Constrained Language modunda çalışır:
  cmdlet'ler, betikler ve başlattıkları programlar çalışır, .NET çağrıları
  (`[System.IO.File]::...`, `Add-Type`) çalışmaz. Orada uygulama verisi
  klasörünü bulamadığı için komut geçmişini kabuk neredeyse oraya, görevin
  kopyasına yazar; inceleme bunu gösterir.
- **`kule exec --reveal PATH` Windows'ta okuma hakkı verir**: hiç
  gizlenmemiş bir yol verilmeden okunamaz; bu yüzden onu açmak salt okunur
  olarak verir.

CI, Windows sandbox testlerini GitHub'ın Windows 11 ARM64 koşucusunda iki
hedef için de (x64, Windows'un emülasyonu altında) yönetici olarak koşar;
standart kullanıcı koşuları ve yerel x64 donanımı, her sürümden önce bir
test makinesinde kapsanır.

## Linux: sandbox'ın ihtiyaç duydukları

- En az **Landlock ABI 3 (Linux 6.2)**: dosya kısaltmayı (truncation)
  kapsayan ilk sürüm. Daha eski bir çekirdekte sandbox kurulamaz ve
  komutlar çalışmaz.
- Ağ kuralları için **Landlock ABI 4 (Linux 6.7)**. Daha eski bir
  çekirdekte, ağ trafiği proxy'den geçmesi gereken bir komut açık
  bırakılmak yerine reddedilir.
- Komutun kendine ait bir ağı, süreçleri, System V IPC'si ve makine adı
  olması için **ayrıcalıksız kullanıcı ad alanları** (unprivileged user
  namespaces). Ubuntu 24.04 bunları varsayılan olarak reddeder:

  ```sh
  sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0
  ```

  Bunlar olmadan Kule, siz daha zayıf yalıtımı kabul edene kadar hiçbir
  ajanı çalıştırmaz (bkz. [`kule isolation`](cli/isolation.md)); kabul
  ettiğiniz sürece de neyin eksik olduğunu söyler.

Görevin klasörleri dışındaki Unix soketlerine, Landlock'un bunlar için
koyduğu kural (ABI 9) sayesinde ulaşılamaz. Daha eski çekirdeklerde çalışma
zamanı dizininiz (`/run/user/<uid>`; oturum veri yolu (session bus) ile
Docker'ın ve Podman'ın rootless soketlerinin bulunduğu yer) komut için boş
bir dizinle örtülür; komut başladığında var olan diğer her soket de
örtülür.

Bir ajanın kendi durum dizinindeki ayarları yalnızca o ajan için salt
okunur olarak bağlanır (mount). Kullanıcı ad alanları yoksa ve bağlantı
(link) olan ya da henüz var olmayan ayarlar için bunlar bunun yerine
çalıştırmadan sonra geri konur; geri koymanın yerini aldığı her şey
`~/.kule/kept/replaced`'e taşınır, asla kaybolmaz.

## Henüz değil

- Proxy üzerinden **HTTP/2** (istemciler HTTP/1.1'e geri döner) ve
  WebSocket yükseltmeleri.
- **Windows**'ta sandbox'ın reddettikleri komuttan sonra listelenmez;
  komut bir erişim hatası alır. Görevin ev dizini sizin hiçbir dosyanızı
  tutmaz; bu yüzden bir ajanın kendi ayarları içeri taşınmaz (`kule home`
  boştur). Pencerenin *Explorer'da göster*'i henüz yok. Kurucu ve
  programlar kod imzası taşımaz: SmartScreen yayımcının bilinmediğini
  söyler; [Kurulum](getting-started/install.md) indirdiğinizi önce nasıl
  doğrulayacağınızı anlatır.
- **macOS**'ta bir görevin kendine ait bir `/tmp`'si yoktur: macOS sandbox'ı
  bir yolu taşıyamaz, bu yüzden `/tmp` kapalı kalır ve araçlar `TMPDIR`'ı
  kullanır. `/tmp`'ye sabit bir yolla yazan bir araç reddedilir ve ret bunu
  söyler.
- **macOS**'ta sandbox bir sunucunun dinlediği adresi ayırt edemez; bu
  yüzden bir görevin sunucuları ancak siz onlara izin verdiğinizde dinler.
  O zaman Node, Python ve Ruby sunucuları bu bilgisayara yönlendirilir,
  diğerleri de sandbox'ın dışından ağdan uzak tutulur.
- **macOS**'ta, bir görevin ağ trafiği proxy'den geçerken sistemin sertifika
  hizmeti görevin komutlarına kapalı kalır. Sertifikaları yalnızca bu hizmet
  üzerinden doğrulayan bir araç, bir görevde HTTPS kullanamaz. pip 24.2 ve
  sonrası `--use-deprecated=legacy-certs` ile çalışır (daha eski pip bu
  seçeneği reddeder; bu yüzden Kule onu kendisi eklemez). Go'ya,
  `SSL_CERT_FILE`'ın gösterdiği kök sertifikaları kullanması söylenir.
- macOS'ta **NuGet'in ve Maven'ın** araçları yalnızca sistemin kök
  sertifikalarına güvenir; bu yüzden onların depoları, yazmalar dahil,
  görülmeden geçer; Go'nun modül proxy'si de öyle.
- **Gemini**, AI Studio'dan alınmış ve bir dosyada saklanan bir API anahtarı
  ister: `GEMINI_FORCE_FILE_STORAGE=true gemini`, ardından `/auth`.

## İlgili

- [Kurulum](getting-started/install.md)
- [Sandbox](concepts/sandbox.md)
- [Sorun giderme](troubleshooting.md)

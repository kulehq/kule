---
title: Platformlar
sidebar_label: Platformlar
description: Kule'nin macOS'ta ve Linux'ta neleri desteklediği, sandbox'ın ikisi arasında nasıl farklılaştığı ve henüz neyin yapılmadığı.
---

# Platformlar

Kule **macOS** ve **Linux**'ta çalışır. Windows daha sonraki bir sürüm için
planlanıyor.

## Bir bakışta destek

| | macOS | Linux |
| --- | --- | --- |
| Sürümler | macOS 13 ya da sonrası | x86_64 sürüm dosyaları (Ubuntu 22.04'te derlenir, Fedora'da test edilir); diğer mimariler kaynak koddan |
| Sürüm dosyaları | Universal `.dmg` (Apple silicon ve Intel) | `.deb`, `.rpm`, `.tar.gz` olarak yalnızca `kule` |
| Sandbox | Seatbelt | Landlock (Linux 6.2 ya da sonrası), seccomp, ad alanları (namespaces) |
| Sandbox içinde ağ kuralları | Evet | Linux 6.7 ya da sonrası |
| Sandbox'ın reddettikleri listelenir | Evet, komuttan sonra ve `sandbox.jsonl` içinde | Hayır (Landlock yalnızca denetim kaydına (audit log) bildirir); komut yine de bir izin hatası alır |
| Görevin kopyası | APFS klonu: neredeyse hiç yer kaplamaz | Btrfs ve XFS'te reflink; ext4'te tam kopya |
| Başlangıçta paket önbellekleri | npm ve cargo sizinkilerden klonlanır | Boş: ilk kurulum yeniden indirir |
| `/tmp` | Kapalı: `TMPDIR` görevin kendine ait | Her görevin kendine ait |
| Kendine ait süreçler | – | Evet: bir komut başka hiçbir programı görmez ve ona sinyal gönderemez |
| Kendine ait ağ ad alanı | – | Evet, erişebildiği tek şey proxy |
| Sunucular bu bilgisayarla sınırlı | Görevin `servers` sınırı izin vermedikçe hiçbiri dinlemez (kabul ettiğiniz daha zayıf yalıtım); izin verildiğinde Node, Python ve Ruby loopback'e yönlendirilir, diğer programların portları korunur (shield) | Dinlenen her adres loopback'e çevrilir |
| GPU kapalıyken | Metal (ve OpenCL) hiçbir aygıt alamaz | `/dev/dri` ve NVIDIA aygıtları salt okunur |
| Gizli değer deposunun anahtarı | Keychain | Secret Service |
| Dosya yöneticisinde göster | Finder | Files (D-Bus `FileManager1`) |

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

- **Windows**.
- Proxy üzerinden **HTTP/2** (istemciler HTTP/1.1'e geri döner) ve
  WebSocket yükseltmeleri.
- **macOS**'ta bir görevin kendine ait bir `/tmp`'si yoktur: macOS sandbox'ı
  bir yolu taşıyamaz, bu yüzden `/tmp` kapalı kalır ve araçlar `TMPDIR`'ı
  kullanır. `/tmp`'ye sabit bir yolla yazan bir araç reddedilir ve ret bunu
  söyler.
- **macOS**'ta sandbox bir sunucunun dinlediği adresi ayırt edemez; bu
  yüzden bir görevin sunucuları ancak siz onlara izin verdiğinizde dinler ve
  o zaman sandbox'ın dışından ağdan uzak tutulur.
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

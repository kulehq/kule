---
title: Sandbox
sidebar_label: Sandbox
description: Bir görevdeki komutun ya da ajanın neleri yapabildiği ve yapamadığı - nereye yazabildiği, neyin ondan gizlendiği, aygıtları, önbellekleri ve süreçleri - ve bunun nasıl ayarlanacağı.
---

# Sandbox

Bir görevin çalıştırdığı her komut, ister [`kule exec`](../cli/exec.md) ile
ya da görevin [terminalinde](../app/terminals-and-servers.md) siz başlatın
ister bir ajan çalıştırsın, işletim sisteminin sandbox'ı
(yalıtılmış ortamı) içinde çalışır:

- **macOS**: sistemin kendi sandbox'ı olan Seatbelt.
- **Linux**: Landlock, seccomp ve ad alanları (namespaces).

Sandbox kurulamazsa komut **çalışmaz**. Kule size söylemeden asla daha az
yalıtıma geri dönmez (bkz.
[Linux'ta daha zayıf yalıtım](#weaker-isolation-on-linux)).

## Bir komut nereye yazabilir

Yalnızca şunlara:

- görevin **kopyasına**;
- görevin kendi `TMPDIR`'ına ve paket önbelleklerine;
- `/tmp`'ye (Linux'ta her görevin kendine ait bir `/tmp`'si vardır;
  macOS'ta ise görevler sistemin `/tmp`'sini paylaşır, çünkü macOS
  sandbox'ı bir yolu taşıyamaz).

Geri kalan her şey salt okunur ya da gizlidir.

**Ev dizininize** yapılan yazmalar (`~/.zshrc`, `~/.config`, `git config
--global`) reddedilmez: görevin [ev katmanına](home-layer.md) düşer ve
incelemenizi bekler.

## Neler gizlenir

Bir komut şunları okuyamaz:

- **kimlik bilgileri**: `~/.ssh`, `~/.aws`, `~/.npmrc`, cargo'nun depo
  token'ı ve benzerleri;
- **kendi dosyalarınız**: Documents, Desktop, Downloads, Pictures, Movies,
  Music, iCloud Drive ve bulut depolama klasörleri;
- **çalışma klasörünün yanındaki projeler**: çalışma klasörünüzün içinde
  bulunduğu klasör, o klasör ev dizininizdeyse (ev dizininin kendisi, bir
  nokta klasörü ya da `~/Library` değilse);
- **Kule'nin ev klasörü** (`~/.kule`): diğer görevler, ayarlar, gizli
  değerler, imzalama anahtarı;
- **diğer ajanların oturumları ve konuşma kayıtları**.

Bu gizli klasörlerin içinde, araçlar ihtiyaç duyduğu için birkaç şey
okunabilir kalır: çalışma klasörünün git dizini (bir worktree'nin yanındaki
deposu) ve `PATH` ile SDK değişkenlerinizin gösterdiği klasörler.

## Bir komut ortamından ne alır

Yalnızca araçların kendilerini bulmak için ihtiyaç duyduğu şeyler aktarılır.
Ortamınızdaki token'lar ve ajan soketleri dışarıda kalır. Bir değişkeni
bilerek aktarmak için:

```sh
kule exec TASK --pass-env MY_SETTING -- ./build.sh
```

Bir API anahtarı için bunun yerine bir [gizli değer](secrets.md) kullanın:
komut değeri asla görmez, bir yer tutucu alır.

## Bir çalışma klasörünün derlendiği projeler

Bir proje çoğu zaman yanındaki bir projeden derlenir: `Cargo.toml`'da
`path = "../shared"`, `package.json`'da `file:../ui`,
`..\Shared\Shared.csproj`. Bu klasörler varsayılan olarak gizlidir. Birini
klasörün görevlerine **salt okunur** olarak açmak için:

```sh
kule reveal add ../shared
```

Kule, klasörün kendi manifest'lerinin adlandırdıklarını önerir (`kule
reveal list`), ama birini yalnızca siz açarsınız. Uygulamada: *Settings →
Rules* ya da başlıktaki "builds from" çipi. Kopya, açtığınız projelere
giden bağlantıların (link) yanında durur; böylece `../shared` kopyadan da
klasörden olduğu gibi çalışır. Bkz. [`kule reveal`](../cli/reveal.md).

## Paket önbellekleri

npm, cargo, pip, uv, Go, NuGet, Yarn ve pnpm **görevin kendine ait
önbelleklerini** alır; böylece bir komutun önbellekte bıraktığı hiçbir şey
sonra çalıştırdığınız bir derlemeye ulaşmaz. Sizin önbellekleriniz salt
okunur kalır. macOS'ta cargo ve npm önbellekleri sizinkilerin klonu olarak
başlar.

## Aygıtlar (Linux)

Bir komut aygıtları adlarıyla alır: `/dev/null`, `zero`, `full`, `tty`,
kendi terminalleri, okumak için `random` ve `urandom` ve GPU
kapatılmamışsa GPU'nunkiler. `/dev`'in tamamını, yani bir kameranın,
mikrofonun ya da USB aygıtının bulunacağı yeri asla almaz. Daha fazlasına
ihtiyaç duyan bir ajan bunları profilinde `devices` altında adlandırır;
Kule diskler, bellek, klavye ve fare, konsollar ve terminallere ulaşanları
reddeder.

## Süreçler ve sunucular

Linux'ta komutlar kendilerine ait süreçler alır: bir komut başka hiçbir
programı görmez ve ona sinyal gönderemez; arkasında çalışır bıraktığı her
şey de onunla birlikte biter. Arka planda başlatılan bir sunucu, onu
başlatan ajan ya da terminal komutu yaşadığı sürece yaşar.

Bir görevin başlattığı sunucular **yalnızca bu bilgisayarda** dinler, asla
ağınızda değil. Bkz. [Terminaller ve sunucular](../app/terminals-and-servers.md).

## Ekrana ihtiyaç duyan programlar

Pencere sunucusuna (window server) ihtiyaç duyan bir program bir görevde
çalışamaz (`--headless` ile bile LibreOffice bunlardan biridir). Bu
olduğunda Live bunu söyler ve ajana da bir kez söylenir.

## Sandbox'ı tek bir komut için ayarlama

[`kule exec`](../cli/exec.md) şu seçenekleri alır:

| Seçenek | Etkisi |
| --- | --- |
| `--allow-write PATH` | Komutun `PATH` altına da yazmasına izin verir (tekrarlanabilir). |
| `--hide PATH` | `PATH`'i de gizler (tekrarlanabilir). |
| `--reveal PATH` | Varsayılan olarak gizli bir yolu salt okunur olarak okumasına izin verir (tekrarlanabilir). |
| `--pass-env NAME` | Bir ortam değişkenini aktarır (tekrarlanabilir). |
| `--no-network` | localhost dahil TCP'yi kapatır. |
| `--open-network` | Proxy'yi ve politikayı atlayarak her yere bağlanır. |
| `--no-sandbox` | Hiç sandbox yok: komut, sizin yazabildiğiniz her yere yazabilir. |

Ajanlar sandbox'larını **profillerinden** alır (hangi durum dizinini ve
oturumu kullanabildiği, sunucuları, anahtarları). Bkz.
[Ajanlar](../using-agents.md).

## Sandbox'ın reddettikleri

macOS'ta sandbox'ın reddettikleri komuttan sonra ekrana yazılır ve görevin
`sandbox.jsonl` dosyasında saklanır:

```text
sandbox: refused file-read-data /Users/me/.config/gh/config.yml (gh)
sandbox: refused file-read-data /Users/me/.ssh/id_ed25519 (ssh)
```

Uygulama reddedilenleri [Timeline](../app/timeline.md)'da gösterir. Linux,
Landlock'un reddettiklerini yalnızca denetim kaydına (audit log) bildirir;
bu yüzden listelenmezler; komut yine de bir izin hatası alır.

## Linux'ta daha zayıf yalıtım {#weaker-isolation-on-linux}

Linux'ta tam yalıtım, Ubuntu 24.04'ün varsayılan olarak reddettiği
ayrıcalıksız kullanıcı ad alanlarına (unprivileged user namespaces)
ihtiyaç duyar. Bunlar olmadan, siz daha zayıf yalıtımı bilerek kabul
edene kadar **Kule hiçbir ajanı çalıştırmaz**:

```sh
kule isolation                     # neyin eksik olduğu
kule isolation --accept-weaker on  # bu bilgisayarın verdikleriyle çalıştır
```

Uygulamada: *Settings → Policy*, *Run agents with what this computer
gives*. Bu geçerli olduğu sürece başlık çubuğunda **Weaker isolation**
yazar. Bkz. [`kule isolation`](../cli/isolation.md) ve
[Platformlar](../platforms.md).

## İlgili

- [`kule exec`](../cli/exec.md), [`kule reveal`](../cli/reveal.md), [`kule isolation`](../cli/isolation.md)
- [Ağ ve sorular](network-and-questions.md)
- [Platformlar](../platforms.md)
- [Güvenlik modeli](../security.md)

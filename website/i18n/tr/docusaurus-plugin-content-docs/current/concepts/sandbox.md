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
- **Windows 11**: her komutun çevresinde, Kule'nin verdiği dışında hiçbir
  şeyi okumayan bir süreç konteyneri (Microsoft'un `mxc-sdk`'sı). Neye
  ihtiyaç duyduğu ve neyin farklı olduğu için bkz.
  [Platformlar](../platforms.md).

Sandbox kurulamazsa komut **çalışmaz**. Kule size söylemeden asla daha az
yalıtıma geri dönmez (bkz.
[Linux'ta daha zayıf yalıtım](#weaker-isolation-on-linux)); Windows'ta
daha zayıf bir mod hiç yoktur.

## Bir komut nereye yazabilir

Yalnızca şunlara:

- görevin **kopyasına**;
- görevin kendi `TMPDIR`'ına ve paket önbelleklerine;
- Linux'ta, görevin kendine ait olan `/tmp`'ye. macOS'ta sandbox bir yolu
  taşıyamaz; bu yüzden `/tmp` sistemin `/tmp`'si olurdu, diğer
  programlarınız ve görevlerinizle paylaşılırdı: komutlar oraya yazmaz ve
  `TMPDIR` (ve zsh'nin `TMPPREFIX`'i) görevin kendisininkini gösterir.

Geri kalan her şey salt okunur ya da gizlidir.

**Ev dizininize** yapılan yazmalar (`~/.zshrc`, `~/.config`, `git config
--global`) reddedilmez: görevin [ev katmanına](home-layer.md) düşer ve
incelemenizi bekler.

## Neler gizlenir {#what-is-hidden}

Bir komut şunları okuyamaz:

- **kimlik bilgileri**: `~/.ssh`, `~/.aws`, `~/.npmrc`, cargo'nun depo
  token'ı ve benzerleri;
- **kendi dosyalarınız**: Documents, Desktop, Downloads, Pictures, Movies,
  Music, iCloud Drive ve bulut depolama klasörleri ile ev dizininizin en
  üstündeki, ayarları dışında kalan her şey (`.` ile başlayan adlar ve
  `~/Library` dışındakiler): Kule'nin açtığı ya da açmadığı diğer
  projeleriniz ve oradaki dosyalarınız. Ev dizininizin en üstündeki bir
  bağlantı olduğu gibi bırakılır;
- **çalışma klasörünün yanındaki projeler**: çalışma klasörünüzün içinde
  bulunduğu klasör, o klasör ev dizininizdeyse (ev dizininin kendisi, bir
  nokta klasörü ya da `~/Library` değilse);
- **Kule'nin açtığı diğer her proje**, nerede olursa olsun, ev dizininizde
  ya da dışında; çalışma klasörünün kendisinde olduğu gibi yanındaki
  projelerle birlikte;
- **uygulamaların ve araçların ayarlarını ve verilerini tuttuğu yerler**:
  `~/.config`, `~/.local/share`, `~/Library/Application Support` ve
  `~/Library/Preferences`; bir uygulamanın oturumu ve yeni bir aracın
  ayarları da. Araç zincirlerinin, kabukların ve istemlerin orada okuduğu
  okunabilir kalır: git, jj, uv, pip, Poetry (`auth.toml` hariç), Ruff,
  mise, Go, Flutter, Matplotlib, fish, zsh, Nushell, Starship ve direnv
  ayarları; uv, mise, pnpm, fnm ve pipx'in `~/.local/share` içinde
  tuttukları, man sayfaları ve fontlarla birlikte;
- **diğer araçların oturumları**: GitHub Copilot, Firebase ve ayarlarını
  onunla birlikte tutan araçlar, Wrangler, Fly, Doppler, SOPS, Podman'ın
  `auth.json` dosyası, Yarn'ın ve Bun'ın ev dizininizdeki ayarları,
  NuGet'in kendi `NuGet.Config` dosyası;
- **editörlerin** oturum ve eklenti gizli değerleri depoları (VS Code,
  Insiders sürümü, Cursor, Windsurf, VSCodium) ve geçmişlerin yanında
  **kaydedilmiş kabuk oturumları** (`~/.zsh_sessions`, `~/.bash_sessions`);
- **Kule'nin ev klasörü** (`~/.kule`): diğer görevler, ayarlar, gizli
  değerler, imzalama anahtarı;
- **diğer ajanların oturumları ve konuşma kayıtları**.

Bu gizli klasörlerin içinde, araçlar ihtiyaç duyduğu için birkaç şey
okunabilir kalır: çalışma klasörünün git dizini (bir worktree'nin yanındaki
deposu), `PATH` ile SDK değişkenlerinizin gösterdiği klasörler ve
`PATH`'inizdeki bir program bir bağlantıysa gösterdiği klasör (Claude
Code'un yerel kurulumu, uv'nin araçları). macOS'ta tercihleri sistem
üzerinden okuyan bir program da aynı sınıra uyar: gizli bir uygulamanın
tercihlerine o yoldan da ulaşılmaz.

Gizli bir şeyi tek bir komutun okumasına izin vermek için komutu
`--reveal YOL` ile çalıştırın (bkz. [`kule exec`](../cli/exec.md)): Kule'nin
onun içinde gizlediği gizli kalır.

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

## Süreçler ve sunucular {#processes-and-servers}

Linux'ta komutlar kendilerine ait süreçler alır: bir komut başka hiçbir
programı görmez ve ona sinyal gönderemez; arkasında çalışır bıraktığı her
şey de onunla birlikte biter. Arka planda başlatılan bir sunucu, onu
başlatan ajan ya da terminal komutu yaşadığı sürece yaşar. Her sistemde,
bir ajan çalıştırmasının ya da `kule exec`'in sandbox'ta çalışır bıraktığı
her şey, ebeveynine ne olmuş olursa olsun, o bitince biter; çalıştırma
sürerken de sınırları onu sayar.

Kule neyi sonlandırdığını, süreçlerin çalıştığı adlarla, sizin çalıştığınız
yerde söyler:

- Bir ajan çalıştırması: Live'da ya da `kule run` çıktısında bir not:
  `Kule ended <names>, which the agent left running in its sandbox, as the run ended`.
- `kule exec`: `kule: ended <names>, still running in the task's sandbox as the command ended`.
- `kule shell` ve uygulamanın terminali (macOS):
  `Kule ended <names>, which the shell left running in its sandbox`.
- Commit, rollback, undo, sync, bir adıma geri dönmek ve ev değişikliklerini
  uygulamak ya da geri almak, Kule kopyaya yazmadan önce görevin
  sandbox'ında hâlâ çalışan her şeyi sonlandırır. Komut satırında:
  `kule: Kule ended <name>, still running in the task's sandbox, to <what you asked>`
  ya da `kule: Kule ended <N> processes still running in the task's sandbox, to <what you asked>: <names>`.
  Uygulamada aynı sözler bir bildirim ve Live'da bir not olarak görünür.

Her ad, altında kaç süreç çalıştığıyla birlikte bir kez listelenir
(`sleep ×2, node`); en çok sekiz ad, sonra kaç tane daha olduğu. Bir ad 32
karakterde kesilir ve düz metin olarak gösterilir.

Bir görevin başlattığı sunucular **yalnızca bu bilgisayarda** dinler, asla
ağınızda değil. Sandbox'ın bir sunucunun adresini ayırt edemediği macOS'ta,
bir görevin programları ancak onun için sunuculara izin verdiğinizde
dinler; bu, kabul ettiğiniz daha zayıf yalıtımdır. Bkz. [Terminaller ve sunucular](../app/terminals-and-servers.md).

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

## Sandbox'ın reddettikleri {#what-the-sandbox-refused}

macOS'ta sandbox'ın reddettikleri komuttan sonra ekrana yazılır ve görevin
`sandbox.jsonl` dosyasında saklanır:

```text
sandbox: refused file-read-data /Users/me/.config/gh/config.yml (gh)
sandbox: refused file-read-data /Users/me/.ssh/id_ed25519 (ssh)
```

Kule bunları macOS'un sistem günlüğü akıttıkça okur ve listeyi günlüğün
deposuyla karşılaştırır: komutlarınkilerden sonra kendi yaptığı bir ret
orada görünmeli, depo da akışın getirmediği bir ret göstermemelidir; çünkü
günlük, geride kalan bir akış okuyucusuna borçlu olduklarının bir kısmını
haber vermeden düşürür. İki şey bir reddi listenin dışında bırakabilir;
Kule ikisini de, çıkış kodunu değiştirmeden söyler:

- Kule beklemeyi bıraktığında günlük, çok yoğun bir bilgisayarda, henüz
  yetişmemişti ya da akış geride kalmıştı: Kule deponun gösterdiklerini
  ekler ve *some refusals may be missing from what is listed* der.
- Günlük, sandbox kullanılırken çekirdeğin mesajlarından bazılarını
  düşürdü. macOS bunu yoğun bir Mac'te, çok yazan herhangi bir sistem
  parçası yüzünden birkaç saniyede bir yapar ve kimin mesajlarını
  düşürdüğünü söylemez. Kule ne zaman olduğunu sakin, gri bir satırda
  söyler: o anda yapılmış bir ret listede olmaz.

Her iki durumda da sandbox reddettiği her şeyi reddetmiştir; eksik olan
yalnızca listedir.

Uygulama reddedilenleri [Timeline](../app/timeline.md)'da gösterir;
[Live](../app/live.md)'da biten bir çalıştırmanın satırı da onları listeler.
Linux, Landlock'un reddettiklerini yalnızca denetim kaydına (audit log)
bildirir; bu yüzden listelenmezler; komut yine de bir izin hatası alır.
Kule hiç ret yokmuş gibi göstermek yerine bunu söyler: başarısız olan bir
ajan çalıştırmasından ya da kabuktan sonra, Live'da ise her çalıştırmadan
sonra
`Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error`
yazar. Windows, process container'ının reddettiklerinin yalnızca bir kısmını
kaydeder; Kule'nin adıyla reddettiği klasörlerdekilerin hiçbirini kaydetmez
([ölçüm](https://github.com/kulehq/kule/blob/main/docs/windows-measurements.md)).
Bu yüzden Kule orada da ret listelemez ve aynı yerlerde
`Windows does not tell Kule what the sandbox refused; each refusal reached the command as an error`
yazar.

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

Kendi namespace'leri olmadan (bu durumda, ya da ağı açık çalıştırılan bir
komutta), bir komutun seccomp filtresi onun ötesine uzanan çağrıları adıyla
reddeder: `ptrace`, `process_vm_writev`, `keyctl`, `userfaultfd`,
`perf_event_open` ve `setns`, host'un sysctl ayarları ne derse desin `EPERM`
ile başarısız olur. Bu durumda sandbox içinde bir debugger ya da `strace`
çalışmaz.

## İlgili

- [`kule exec`](../cli/exec.md), [`kule reveal`](../cli/reveal.md), [`kule isolation`](../cli/isolation.md)
- [Ağ ve sorular](network-and-questions.md)
- [Platformlar](../platforms.md)
- [Güvenlik modeli](../security.md)

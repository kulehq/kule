---
title: Sorun giderme
sidebar_label: Sorun giderme
description: Kule'de sık karşılaşılan sorunlar, nedenleri ve çözümleri - kurulum, ajanlar, Linux'ta sandbox, sertifikalar, disk alanı, commit'ler ve daemon.
---

# Sorun giderme

Her başlıkta **belirti**, **neden** ve **çözüm** var.

## Kurulum ve başlatma

### macOS Kule'yi açmayı reddediyor

**Belirti:** "Kule can't be opened because Apple cannot check it for
malicious software" iletisi ya da indirdikten sonra uygulamanın
başlamaması.

**Neden:** Kule'nin macOS uygulaması Apple tarafından notarize edilmemiştir,
ad hoc imzalıdır.

**Çözüm:** önce indirdiğinizi doğrulayın (`SHA256SUMS` ve `gh attestation
verify`; bkz. [Kurulum](getting-started/install.md)). Sonra ya **System
Settings → Privacy & Security**'yi açıp Kule için **Open Anyway**'i seçin
ya da bir kez şunu çalıştırın:

```sh
xattr -dr com.apple.quarantine /Applications/Kule.app
```

### macOS, keychain'deki "Kule secrets" için erişim istiyor

**Belirti:** bir keychain penceresi, Kule'nin **Kule secrets** öğesini
kullanmasına izin vermenizi istiyor.

**Neden:** Kule [gizli değerlerinizi](concepts/secrets.md), keychain'in bu
adla sakladığı bir anahtarla mühürler. macOS, o öğe için henüz güvenmediği
bir program onu okuduğunda sorar. Kaynak koddan derlenen her sürüm her
derlemede yeni bir ad hoc imza alır; bu yüzden macOS her derlemeden sonra
yeniden sorar.

**Çözüm:** izin verin (giriş parolanızı girin; *Always Allow*, Kule'nin bu
derlemesi için soruyu bitirir).

### `kule: command not found`

**Neden:** `kule` `PATH`'inizde değil.

**Çözüm:** onu `PATH`'inizdeki bir klasöre bağlayın, örneğin
`ln -sf /Applications/Kule.app/Contents/MacOS/kule ~/.local/bin/kule`;
kaynak koddan derlediyseniz `target/release/kule`'yi. `kule --version` ile
kontrol edin.

## Ajanlar

### Bir ajan eksik görünüyor ya da başlayamıyor

**Belirti:** ilk açılış ekranı ya da `kule agents list` bir ajanın
çalışamayacağını söylüyor.

**Nedenler ve çözümler:**

- **Node.js eksik.** Claude Code ve Codex `npx` ile başlar. Node.js'i kurun,
  sonra bir terminalde `npx --version`'ı kontrol edin.
- **Uygulama Finder'dan ya da bir başlatıcıdan açıldı.** Uygulama `PATH`'i
  giriş kabuğunuzdan (login shell) okur; böylece `~/.zprofile`, `~/.zshrc`
  ya da `~/.bash_profile` içinde ayarlanan araçlar bulunur. Yalnızca
  etkileşimli, giriş olmayan bir kabuğun ayarladığı bir araç ise bulunmaz.
  Onun `PATH` satırını giriş profilinize koyun.
- **Ajana oturum açılmamış.** Kule olmadan yaptığınız gibi bir terminalde
  bir kez oturum açın (`codex login`, Claude Code'un kendi oturum açması,
  Gemini'nin `/auth`'u).
- **Eklediğiniz bir profil onaylanmamış** ya da onaydan sonra değişmiş:
  `kule agents show NAME`, ardından `kule agents approve NAME` çalıştırın.
  Bkz. [Ajanlar](using-agents.md).

`kule agents list`, çalışamayan her ajanın neden çalışamadığını söyler.

### Gemini'ye oturum açılamıyor

**Neden:** Google, Code Assist oturum açmayı artık Kule gibi istemcilere
sunmuyor ve sandbox keychain'i salt okunur tutuyor.

**Çözüm:** Google AI Studio'dan alınmış ve bir dosyada saklanan bir API
anahtarı kullanın:

```sh
GEMINI_FORCE_FILE_STORAGE=true gemini
```

ardından Gemini içinde `/auth`.

### Bir çalıştırma başlamadan reddediliyor: bütçe bitti

**Neden:** çalıştırmayı kapsayan bütçelerden biri (görevinki, grubununki ya
da günlük olan) bitmiş. Ret iletisi hangisi olduğunu söyler.

**Çözüm:** `kule budget TASK`, görevin bir sonraki çalıştırmasını kapsayan
her bütçeyi listeler. Sınırı belirleyeni artırın ya da kaldırın (`kule
budget TASK off`, `kule group budget ID off`, `kule daily-budget off`).
Bkz. [Bütçeler ve sınırlar](concepts/budgets-and-limits.md).

### Bir çalıştırma kendiliğinden durdu

**Neden:** bir [sınır](concepts/budgets-and-limits.md) onu durdurdu. Live
hangisi olduğunu ve neyi ölçtüğünü söyler (`Stopped: the run's processes
held 4.2 GB, over its memory limit of 4 GB`).

**Çözüm:** `kule limits --task TASK` sınırları ve her birinin nereden
geldiğini gösterir; durduran sınırı artırın.

## Linux

### Linux: ajanlar başlamıyor, "weaker isolation"

**Belirti:** hiçbir ajan çalışmıyor; *Settings → Policy* ya da `kule
isolation`, bu bilgisayarın sandbox içindeki bir komuta veremediklerini
listeliyor.

**Neden:** Kule'nin Linux sandbox'ı **ayrıcalıksız kullanıcı ad
alanlarına** (unprivileged user namespaces) ihtiyaç duyar. Ubuntu 24.04
bunları varsayılan olarak reddeder.

**Çözüm (önerilen):** bunlara izin verin:

```sh
sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0
```

Yeniden başlatmadan sonra da kalıcı olması için ayarı `/etc/sysctl.d/`
altındaki bir dosyaya yazın.

**Ya da** daha zayıf yalıtımı bilerek kabul edin: `kule isolation
--accept-weaker on` ya da *Settings → Policy*'de *Run agents with what this
computer gives*. Bu geçerli olduğu sürece başlık çubuğunda **Weaker
isolation** yazar. `kule isolation --accept-weaker off` geri döndürür. Bkz.
[`kule isolation`](cli/isolation.md).

### Linux: komutlar bir Landlock hatasıyla başarısız oluyor

**Neden:** çekirdek çok eski. Kule'nin sandbox'ı en az Landlock ABI 3'e
(Linux 6.2), ağ kuralları için ABI 4'e (Linux 6.7) ihtiyaç duyar. Kule bir
komutu, olması gerekenden daha az yalıtımla çalıştırmayı reddeder.

**Çözüm:** Linux 6.7 ya da sonrasını kullanın. `uname -r` ile kontrol
edin.

### Linux: bir komut "permission denied" alıyor ama hiçbir şey listelenmiyor

**Neden:** Landlock reddettiklerini yalnızca denetim kaydına (audit log)
bildirir; bu yüzden Kule onları macOS'taki gibi listeleyemez. Komut yine de
hatayı alır.

**Çözüm:** yol, sandbox'ın izin verdiklerinin dışında (bkz.
[Sandbox](concepts/sandbox.md)). Çalışma klasörünün yanındaki bir proje
için `kule reveal add PATH`; tek bir komut için `kule exec --reveal PATH`
ya da `--allow-write PATH`.

## Ağ ve sertifikalar

### Bir komut bir sunucuya ulaşamıyor

**Belirti:** `CONNECT tunnel failed, response 403` ya da
`net: example.com:443 · refused, would wait for approval; the command line asks no one`.

**Neden:** [politika](concepts/policy.md) o sunucuyu bir insana soruyor.
Komut satırında soracak kimse olmadığı için hemen reddeder.

**Çözüm:** işi uygulamada çalıştırıp soruyu onaylayın; ya da `kule rules
allow HOST:443 --yes` ile bu klasörde o sunucuya izin verin. Yalnızca tek
bir komut için `kule exec --open-network`, proxy'yi ve politikayı devre
dışı bırakır.

### macOS'ta pip sertifika hatalarıyla başarısız oluyor

**Neden:** pip 24.2 ve sonrası sertifikaları sistemin kendi hizmeti
üzerinden doğrular; bu hizmet, bir görevin ağ trafiği proxy'den geçerken o
görevde kapalı kalır.

**Çözüm:** `pip install --use-deprecated=legacy-certs …`.

### macOS'ta NuGet, Maven ya da Go

Bunların macOS'taki araçları yalnızca sistemin kök sertifikalarına güvenir;
bu yüzden depoları proxy'den, yazmalar dahil, görülmeden geçer; Go'nun
modül proxy'si de öyle. Go'ya sertifikaları `SSL_CERT_FILE`'ın gösterdiği
kök sertifikalarla doğrulaması söylenir. Düzeltilecek bir şey yok; yalnızca
proxy'nin bu bağlantıların içine bakmadığını bilin.

### Bir istemci HTTP/2'de ya da WebSocket'te başarısız oluyor

**Neden:** proxy henüz HTTP/2'yi ve WebSocket yükseltmelerini
desteklemiyor. İstemciler genellikle HTTP/1.1'e geri döner.

## Disk alanı

### `~/.kule` çok büyük görünüyor

**Neden:** Finder, `du` ve disk kullanım araçları her klonu tam boyutuyla
sayar. macOS'ta bunun çoğu sizin kendi dosyalarınızla ortaktır.

**Çözüm:** bunun yerine boş alana bakın (`df -h` ya da System Settings →
General → Storage). Btrfs'te `btrfs filesystem du` kullanın. Bkz.
[Görevler ve kopyalar](concepts/tasks-and-copies.md).

### Linux'ta her görev klasörün tam boyutu kadar yer kaplıyor

**Neden:** ext4 blokları paylaşamaz; bu yüzden kopya, `.git` ve
`node_modules` dahil tam bir kopyadır.

**Çözüm:** çalışma klasörlerini ve Kule'nin ev klasörünü (`KULE_HOME`)
reflink destekli Btrfs ya da XFS üzerinde, aynı birimde tutun. Artık
ihtiyacınız olmayan görevleri silin (`kule rm TASK`); çöp boşaltıldığında
yerleri geri gelir.

## İnceleme ve commit

### `kule commit`: "not committed; rerun with --approve"

**Neden:** bazı değişiklikler onayınızı bekliyor (kural adıyla belirtilir,
örneğin `manifests-and-lockfiles`).

**Çözüm:** onlara bakın (`kule status TASK`), sonra `kule commit TASK
--approve` ya da `kule rollback TASK`.

### `kule commit`: "the workspace changed since Kule last saw it"

```text
kule: the workspace changed since Kule last saw it, at: hello.txt (both changed lines 1–3)
hint: roll back, or rerun the task on the current workspace
```

**Neden:** siz (ya da başka bir görev), bu görevin de değiştirdiği bir
dosyayı klasörde değiştirdiniz.

**Çözüm:** önce klasörün değişikliklerini `kule sync TASK` ile kopyaya
alın; birleştirebildiğini birleştirir. Ya da değişiklikleri atıp görevi
yeniden çalıştırın. Bkz.
[Eşitleme ve birleştirme](concepts/sync-and-merging.md).

### `kule sync`: "nothing was brought in"

**Neden:** ikisinin de değiştirdiği bir dosya birleşmiyor (aynı satırlar,
metot, bölüm ya da anahtar).

**Çözüm:** önce görevin o dosyadaki değişikliğini commit edin ya da atın;
ya da kopyayı olduğu gibi bırakın.

### Kopya üzerinde bir terminal ya da editör açılmıyor

**Belirti:** "the copy's git config changed, and git there would run what
it says outside the sandbox; review it, then commit or roll it back before
opening a terminal".

**Neden:** görev, kopyada sandbox'ın dışında kendiliğinden çalışacak bir
şeyi değiştirdi: git config'ini ya da hook'larını (kabuk isteminizdeki
`git status` onları çalıştırırdı), `rust-toolchain.toml` ya da `.nvmrc`
gibi bir sürüm dosyasını, `.envrc`'yi ya da bir editörün ayarlarını. Ya da
görevde bir ajan çalışıyor: işi bitene kadar bekleyin.

**Çözüm:** değişikliği Review'da (ya da `kule status` ile) inceleyin,
sonra commit edin ya da atın. Bu tür değişiklikler yalnızca kopyayı
sandbox'ın dışında açanları durdurur (*Open in terminal app*, *Open the
copy in your editor*, `kule shell --outside`); görevin terminali ve
`kule shell` sandbox'ta çalışır ve yine de açılır.

### Commit, sync ya da bir çalıştırma bir terminali bekliyor

**Belirti:** "a terminal is open on task …'s copy (held by process …);
close it first, then commit" (ya da *then sync*, *then apply its home
changes*, *then start work*).

**Neden:** görevde bir terminal açık: uygulamanın **Terminal** panelindeki
bir sekme ya da `kule shell`. Kule orada çalışanı sonlandırmaz; böylece
kaydetmediğiniz hiçbir şey kaybolmaz.

**Çözüm:** terminalin sekmesini kapatın ya da içinde `exit` yazın, sonra
yeniden deneyin.

### Kopyayı bir uygulamada açtıktan sonra çalıştırma reddediliyor

**Belirti:** uygulama bir çalıştırmadan önce *Close … first* diye soruyor
ya da `kule run` ve `kule exec` "Kule opened task …'s copy in … outside the
sandbox; … Close it, then say so: in the app, or with `kule closed …`"
diyor.

**Neden:** *Open in terminal app* ya da *Open the copy in your editor*
kopyayı sandbox'ın dışında açtı ve Kule o uygulamanın kapandığını
göremiyor.

**Çözüm:** terminal uygulamasını ya da editörü kapatın, sonra uygulamada
*I closed it, start the run*'ı seçin ya da
[`kule closed TASK`](cli/closed.md) çalıştırın.

### Ekrana ihtiyaç duyan bir program başarısız oluyor (LibreOffice ve diğerleri)

**Neden:** bir görev pencere sunucusuna (window server) ulaşamaz.
LibreOffice `--headless` ile bile çöker ve macOS onun beklenmedik şekilde
kapandığını bildirebilir. Live bununla ilgili bir satır gösterir ve ajana
da bir kez söylenir.

**Çözüm:** bir görevin içinde yok. O adımı Kule'nin dışında yapın.

### "Open with its app" reddediliyor

**Neden:** Kule yalnızca adının söylediği şey olan, bir bağlantı (link)
üzerinden ulaşılmayan belgeleri, resimleri, sesleri ve videoları, hiçbir
ajan çalışmıyorken açar. Asla bir programı, bir betiği, bir web sayfasını,
SVG'yi ya da makro içeren bir Office dosyasını açmaz: bunlar sandbox'ın
dışında çalışırdı. PDF, Word, Excel, PowerPoint, OpenDocument ve CSV dosyaları,
başka bir yerden bir şey yükleyecek ya da bir şey çalıştıracak olduğunda
reddedilir: mesaj neyin olduğunu söyler.

**Çözüm:** çalıştırmanın bitmesini bekleyin ya da dosyaya Preview'da bakın.

## Daemon

### Uygulama, daemon'ın başka bir protokol konuştuğunu söylüyor

**Belirti:** "the Kule daemon speaks protocol N (version), this app M".

**Neden:** Kule'nin başka bir derlemesinden kalan bir daemon bu ev klasörü
için hâlâ çalışıyor; örneğin yeniden derlediğiniz ya da güncellediğiniz
için.

**Çözüm:** uygulamadan çıkın ve eski daemon'ın bitmesini bekleyin: hiçbir
pencere, çalışan bir ajan ya da bekleyen bir soru olmadan geçen on
dakikanın sonunda kendiliğinden çıkar. Hiçbir çalıştırma sürmüyorsa
sürecini kendiniz de sonlandırabilirsiniz (`kule daemon` ya da, uygulama
başlattıysa, `kule-desktop daemon`). Sonra uygulamayı yeniden açın.

### "socket path … is longer than … bytes; use a shorter KULE_HOME"

**Neden:** daemon `<home>/kuled.sock` üzerinde dinler ve Unix soket
yollarının kısa bir uzunluk sınırı vardır.

**Çözüm:** `KULE_HOME`'u daha kısa bir yola ayarlayın.

### Daemon'ın kayıtları nerede?

Kule'nin ev klasöründeki `kuled.log`'da (`~/.kule/kuled.log`). Ön planda
izlemek için bir terminalde `kule daemon` çalıştırın. Bkz.
[`kule daemon`](cli/daemon.md).

## Hâlâ takıldınız mı?

- `kule help COMMAND` her komutu açıklar; ayrıca
  [CLI başvurusuna](cli/index.md) bakın.
- [github.com/kulehq/kule](https://github.com/kulehq/kule/issues) adresinde
  bir issue açın.
- Bir güvenlik sorunu mu? Onu
  [SECURITY.md](https://github.com/kulehq/kule/blob/main/SECURITY.md)'nin
  söylediği gibi gizli olarak bildirin; asla herkese açık bir issue'da
  değil.

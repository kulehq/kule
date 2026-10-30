---
title: kule tools
sidebar_label: kule tools
description: Her görevin komutlarının kendi klasörleri dışında neleri okuyabildiği ve Kule'nin tanımadığı bir araç klasörünün onlara nasıl okutulacağı.
---

# kule tools

Bir görevin komutları **yalnızca verilen yerleri okur**: görevin kendi
klasörlerini, sistemi, araç zincirlerinizi ve buraya eklediğiniz klasörleri.
Bu macOS, Linux ve Windows'ta aynıdır. `kule tools` listeyi, her kökün nereden
geldiğiyle birlikte gösterir ve Kule'nin tanımadığı bir araç klasörünü her
görev için **salt okunur** olarak eklemenizi sağlar.

## Sözdizimi

```sh
kule tools [SEÇENEKLER] [KOMUT]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-tools-list) | Bir görevin komutlarının okuyabildiği her kökü kaynağıyla, ve adı verilip reddedilenleri listeler (varsayılan). |
| [`add`](#kule-tools-add) | Her göreve bir klasörü okutur. |
| [`remove`](#kule-tools-remove) | Geri alır. |

## Açıklama

[Sandbox](../concepts/sandbox.md) okumayı verir, varsaymaz. Kökler şuradan
gelir:

| Kaynak | Nedir |
| --- | --- |
| `task` | Görevin kopyası, geçici klasörü ve ev katmanı: okuma ve yazma. |
| `system` | İşletim sisteminin kendi programları, kütüphaneleri ve ayarları: `/usr`, `/bin`, `/System/Library`, `/Library/Developer`, Homebrew, Xcode ve bütünüyle `/etc`; ancak orada adıyla kapatılan birkaç kimlik dosyası dışında (`pip.conf`, `docker/config.json`, `rclone.conf`, `samba/smb.conf`, `environment`, `profile.d`). Linux'ta ayrıca `/proc`, `/sys`, `/run` altındaki çözücü dosyaları ve `/var` ile `/tmp` altında dar yerler; `/opt`, `/tmp` ya da `/run` asla bütünüyle değil. |
| `env` | Komutlara aktarılan değişkenlerin araçlarınızın yerini söylediği yerler: her `PATH` girdisi (ve ev dizininizin dışındaki bir `bin`'in üstündeki aracın kendi klasörü, `/opt/gradle/bin` → `/opt/gradle`; kendi klasörlerinizin ve uygulamaların ayar ve verilerini tuttuğu yerlerin içinde de, `~/sdk/go1.22/bin` → `~/sdk/go1.22`, ama oradaki bir uygulamanın klasörünün tamamı asla), `JAVA_HOME`, `GOROOT`, `DOTNET_ROOT` ve Kule'nin aktardığı diğerleri. |
| `tool` | Araç zincirlerinin ev dizininizde varsayılan olarak, hiçbir değişken göstermeden okuduğu kendi klasörleri: `~/.cargo`, `~/.rustup`, `~/.npm`, `~/.m2`, `~/.gradle`, `~/.nuget` (içindeki NuGet ayarları hariç; kimlik bilgisi taşıyabilir, NuGet kendi ayarlarını yapar), `~/.dotnet`, `~/go`, `~/.cache`, `~/.local/bin`, uv'nin `~/.local/share` altındaki klasörü, pipx'in evi nerede tutuyorsa orası (pipx 1.5 öncesinden kalan `~/.local/pipx`, `~/.local/share/pipx`, macOS'ta `~/Library/Application Support/pipx`), `~/Library/Java/JavaVirtualMachines` ve araçların ev dizininizin en üstünde okuduğu ayar dosyaları (`.gitconfig`, `.editorconfig`, `.tool-versions`, …). |
| `person` | `kule tools add` ile eklediğiniz. |
| `admin` | Yöneticinin dosyasının adını verdiği (aşağıda). |
| `reveal` | Bir çalışma klasörünün yanındaki proje, o klasörün görevleri için ([`kule reveal`](reveal.md)). |
| `--reveal` | Tek bir komut için bir yol ([`kule exec --reveal`](exec.md)). |

Verilen bir kökün içinde kimlik dosyaları adıyla kapalı kalır: `~/.cargo`
içinde `~/.cargo/credentials.toml`, `~/.m2` içinde `~/.m2/settings.xml`.

Her kaynak **tek bir doğrulayıcıdan** geçer; doğrulayıcı, Kule'yi
zayıflatacak bir yolu kim adlandırmış olsa da reddeder: ev dizininizin
kendisi ya da üstündeki bir klasör, başka bir kişinin ev dizini, Kule'nin
home'u (diğer görevlerin kopyaları, politika, anahtarlar), bir kimlik
klasörü, kendi klasörlerinizden biri (Documents, bir çalışma klasörünün
yanındaki projeler), uygulamaların ayar ve veri tuttuğu yerler (`~/.config`,
`~/Library/Application Support`), bir diskin ya da volume'ün kökü, birinci
seviye bir sistem klasörü (`/opt`, `/usr`) ve bir görevin yazdığı yerler.
Kendi klasörlerinizden ya da ayar depolarından birinin *içindeki* bir klasör
eklenebilir (`~/Documents/sdk`); klasörün kendisi eklenemez. Karar, çözülmüş
yol üzerinde verilir ve her çalıştırmadan önce yeniden verilir. Hiçbir şey
çalışma klasöründen ya da bir görevin kopyasından gelmez: bir ajan kendi
sandbox'ını genişletemez.

### Bir aracın Kule'nin tanımadığı bir klasöre ihtiyacı olduğunda

Komut reddi bir hata olarak görür (`Operation not permitted`, `Permission
denied`) ve koşu bunu listeler:

```console
$ kule exec 1046fb30 -- ./build.sh
sandbox: refused file-read-data /opt/corp/sdk/lib/tool.jar (java)
  A task's commands read only what is granted (kule tools list). If a tool
  needs this, `kule tools add '/opt/corp/sdk/lib'` lets every task read that
  folder, never write it; Kule refuses a folder that would weaken it (your
  home, a credential folder, Kule's own).
```

Öneri, Kule'nin kendi ret kaydından gelir (macOS'ta sistem günlüğü, Linux'ta
[`kule exec --trace`](exec.md) ile `strace`); komutun yazdırdığından asla.
Klasörü olabildiğince dar ekleyin ve yeniden çalıştırın:

```console
$ kule tools add /opt/corp/sdk
kept as 1: every task reads /opt/corp/sdk, never writes it
```

Linux retleri kendiliğinden bildirmez: listelemek için komutu bir kez
`kule exec --trace` ile çalıştırın.

### Yöneticinin dosyası

Bir yönetici, bir bilgisayardaki her göreve klasörleri bir TOML dosyasıyla
verebilir:

| Sistem | Dosya |
| --- | --- |
| macOS | `/Library/Application Support/Kule/tools.toml` |
| Linux | `/etc/kule/tools.toml` |
| Windows | `%ProgramData%\Kule\tools.toml` |

```toml
paths = ["/opt/corp/sdk", "/srv/toolchains"]
```

Dosya yalnızca **root (Windows'ta bir yönetici) dosyanın ve klasörünün
sahibiyken ve başka hiç kimse ikisine de yazamazken** okunur. Aksi halde yok
sayılır ve `kule tools list` bunu söyler: herkesin düzenleyebileceği bir
dosya, herkesin her görevi genişletmesine izin verirdi. Yolları sizinkilerle
aynı doğrulayıcıdan geçer.

## kule tools list

```console
$ kule tools
no folder added: kule tools add PATH lets every task read one

every task's commands read, and never write:
  system  /Library/Developer
  system  /System/Library
  system  /bin
  system  /opt/homebrew
  system  /private/etc
  system  /usr
  env     /Users/me/.cargo/bin
  env     /opt/homebrew
  tool    /Users/me/.cargo
  tool    /Users/me/.gitconfig
  tool    /Users/me/.rustup
  system  /etc whole, but for 7 names (pip.conf, docker/config.json, …)
named but refused:
  admin   /opt: a first-level system folder; name a folder inside it
the task's own folders, and what `kule reveal` opens, besides
```

## kule tools add

```sh
kule tools add <YOL>
```

```console
$ kule tools add /opt/corp/sdk
kept as 1: every task reads /opt/corp/sdk, never writes it
$ kule tools add ~
kule: /Users/me: your home, or a folder above it
$ kule tools add ~/.ssh
kule: /Users/me/.ssh: a path Kule keeps from every command
$ kule tools add /opt
kule: /opt: a first-level system folder; name a folder inside it
```

## kule tools remove

```console
$ kule tools remove 1
removed 1: not readable from the next command on
```

## Ayrıca bakınız

- [Sandbox](../concepts/sandbox.md): bir komutun neyi okuyup neye yazabildiği.
- [`kule reveal`](reveal.md): bir çalışma klasörünün yanındaki proje, tek klasör için.
- [`kule exec`](exec.md): tek komut için `--reveal`, Linux'ta `--trace`.

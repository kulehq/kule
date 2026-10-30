---
title: Kule'yi kurun
sidebar_label: Kurulum
description: Kule'yi macOS, Linux ya da Windows'ta kaynak koddan derleyin veya bir önizleme sürümü indirin; ardından ajanları kurup her şeyin çalıştığını doğrulayın.
---

# Kule'yi kurun

Kule iki programdan oluşur:

- **`kule`**, komut satırı aracı;
- **`kule-desktop`**, masaüstü uygulaması (macOS'ta `Kule.app`).

İkisi de aynı motoru ve aynı görevleri kullanır. Yalnızca komut satırını,
yalnızca uygulamayı ya da ikisini birden kullanabilirsiniz.

| Platform | Durum |
| --- | --- |
| macOS 13 (Ventura) ya da sonrası, Apple silicon ve Intel | Destekleniyor |
| Linux x86_64 (Ubuntu 22.04 ya da sonrası, Fedora, …) | Destekleniyor |
| Diğer mimarilerde Linux | Kaynak koddan derleyin; hazır sürüm dosyası yok |
| Windows 11 24H2, 26100.9278 ya da sonraki güncellemeyle; x64 ve ARM64 | Destekleniyor; sandbox'ın ihtiyaç duydukları için [Platformlar](../platforms.md) |
| Windows 10, Windows Server | Desteklenmiyor: süreç konteyneri yok |

:::note Bir önizleme

Kule bir **önizleme** (preview): bir önizlemeden ötekine değişiklikler
bekleyin. En son ön sürüm, [`0.2.0-preview.20`](https://github.com/kulehq/kule/releases/tag/v0.2.0-preview.20), sürümler sayfasında:
[indirin](#download-a-preview-release) ya da
[kaynak koddan derleyin](#build-from-source).

:::

## Kaynak koddan derleme {#build-from-source}

### 1. Gereksinimleri kurun

**Rust.** [rustup](https://rustup.rs)'ı kurun. Bir sürüm seçmeniz
gerekmez: deponun
[`rust-toolchain.toml`](https://github.com/kulehq/kule/blob/main/rust-toolchain.toml)
dosyası tam derleyici sürümünü (Rust 1.99.1) belirtir ve `cargo` onu ilk
derlemede kendisi kurar.

**macOS'ta:** Xcode komut satırı araçları.

```sh
xcode-select --install
```

**Linux'ta:** bir C araç zinciri, `pkg-config` ve masaüstü uygulamasının
(Tauri ve WebKitGTK) ihtiyaç duyduğu kütüphaneler. Kule'nin CI'ının
Ubuntu'da kurduğu paketler şunlar:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
```

Fedora'da Tauri'nin listesindeki karşılıkları `webkit2gtk4.1-devel`,
`libappindicator-gtk3-devel`, `librsvg2-devel` ve `libxdo-devel`; yanında
bir C derleyicisi ve `pkgconf` gerekir. Yalnızca komut satırını
istiyorsanız WebKitGTK kütüphanelerini atlayıp yalnızca `-p kule`'yi
derleyebilirsiniz (aşağıda).

**Windows'ta:** *Desktop development with C++* iş yüküyle Visual Studio
Build Tools (MSVC derleyicisi ve Windows 11 SDK) ve Git. Uygulama,
Windows 11'de hazır bulunan WebView2'yi kullanır. `rustup`,
`-pc-windows-msvc` araç zincirini kendisi seçer.

### 2. Derleyin

```sh
git clone https://github.com/kulehq/kule.git
cd kule
cargo build --release -p kule -p kule-desktop
```

İlk derleme birkaç dakika sürer. Bittiğinde:

| Dosya | Nedir |
| --- | --- |
| `target/release/kule` | Komut satırı aracı |
| `target/release/kule-desktop` | Masaüstü uygulaması |

Uygulama olmadan, yalnızca komut satırı:

```sh
cargo build --release -p kule
```

Windows'ta dosyalar `.exe` ile biter ve `kule`'nin yanında, her görev için
başlattığı iki program daha gerekir: `kule-proxy.exe` (görevin proxy'si,
kendi uygulama konteynerinde) ve `kule-pty-helper.exe` (terminalin sahte
konsolu, görevin konteynerinin içinde). Üçünü birlikte derleyin ve birlikte
tutun:

```powershell
cargo build --release -p kule -p kule-proxy -p kule-pty-helper
```

### 3. `kule`'yi `PATH`'inize ekleyin

Zaten `PATH`'inizde olan bir klasör seçin, örneğin `~/.local/bin`:

```sh
mkdir -p ~/.local/bin
ln -sf "$PWD/target/release/kule" ~/.local/bin/kule
kule --version
```

Windows'ta üç programı tutan klasörü kullanıcı `PATH`'inize ekleyin
(*Settings → System → About → Advanced system settings → Environment
Variables*) ya da PowerShell'de:

```powershell
$user = [Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::SetEnvironmentVariable('Path', "$user;$PWD\target\release", 'User')
```

Yeni bir terminal açın, sonra `kule --version`.

### 4. Uygulamayı başlatın

```sh
./target/release/kule-desktop
```

(Windows'ta `.\target\release\kule-desktop.exe`.)

Geliştirme sırasında `cargo run -p kule-desktop` de kullanabilirsiniz.

Uygulama Kule **daemon**'ını (arka plan sürecini) kendisi başlatır (kendi
programını `kule-desktop daemon` olarak çalıştırır); yani ayrıca
başlatmanız gereken bir şey yoktur. Daemon'ın ne yaptığı için
[`kule daemon`](../cli/daemon.md) sayfasına bakın.

## Bir önizleme sürümü indirme {#download-a-preview-release}

Her sürüm şu dosyaları içerir:

| Dosya | Ne için |
| --- | --- |
| `Kule_<version>_universal.dmg` | macOS 13 ya da sonrası; Apple silicon ve Intel tek bir uygulamada. `kule` komutu uygulamanın içindedir. |
| `Kule_<version>_amd64.deb` | Debian ve Ubuntu: `kule-desktop` ve `kule`'yi `/usr/bin`'e koyar. |
| `Kule-<version>.x86_64.rpm` | Fedora: aynısı, RPM olarak. |
| `kule-<version>-macos-universal.tar.gz` | macOS 13 ya da sonrası için yalnızca `kule` komutu. |
| `kule-<version>-linux-x86_64.tar.gz` | Linux x86_64 için yalnızca `kule` komutu. |
| `Kule_<version>_x64-setup.exe`, `Kule_<version>_arm64-setup.exe` | Windows 11: uygulamayı ve `kule` komutunu yalnızca sizin için, `%LOCALAPPDATA%\Kule` altına kurar; yönetici istemez. |
| `kule-<version>-windows-x64.zip`, `kule-<version>-windows-arm64.zip` | Windows 11 için yalnızca `kule` komutu, başlattığı iki programla (`kule-proxy.exe`, `kule-pty-helper.exe`); birlikte tutulur. |
| `kule-<version>.cdx.json`, `kule-desktop-<version>.cdx.json` | CLI'ın ve uygulamanın CycloneDX SBOM'u: derlendikleri her crate. |
| `SHA256SUMS` | Yukarıdaki her dosyanın sağlama toplamı. |

### Önce indirdiğinizi doğrulayın

Her dosyanın `SHA256SUMS` içinde bir sağlama toplamı ve bir **build
provenance attestation**'ı (derleme kaynağı belgesi) vardır. Bu belge,
dosyanın bu depodan, sürüm workflow'u tarafından, sürümün etiketinden
derlendiğini kanıtlar. Herhangi bir şeyi açmadan önce ikisini de kontrol
edin:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify Kule_<version>_universal.dmg --repo kulehq/kule \
  --signer-workflow kulehq/kule/.github/workflows/release.yml \
  --source-ref refs/tags/v<version>
```

(`gh`, [GitHub CLI](https://cli.github.com)'dır. `.deb`, `.rpm`,
`.tar.gz`, `-setup.exe` ya da `.zip`'i kontrol etmek için dosya adını
değiştirin. Windows'ta PowerShell'deki `Get-FileHash`, bir dosyanın
`SHA256SUMS` ile karşılaştırılacak SHA-256'sını verir.)

### macOS

1. `.dmg`'yi açın ve **Kule**'yi **Applications** klasörüne sürükleyin.
2. Komutu `PATH`'inize ekleyin:

   ```sh
   ln -sf /Applications/Kule.app/Contents/MacOS/kule ~/.local/bin/kule
   ```

3. Kule'yi açın.

Kule'nin macOS uygulaması Apple tarafından **notarize edilmemiştir, ad hoc
imzalıdır**; bu yüzden macOS onu ilk seferde açmayı reddeder. İndirdiğinizi
doğruladıktan sonra şunlardan birini yapın:

- **System Settings → Privacy & Security**'yi açıp Kule için **Open
  Anyway**'i seçin; ya da
- bir kez şunu çalıştırın:

  ```sh
  xattr -dr com.apple.quarantine /Applications/Kule.app
  ```

`kule-<version>-macos-universal.tar.gz` içindeki `kule` komutunu bir
tarayıcı indirdiyse, onun için de bir kez aynısı gerekir:
`xattr -d com.apple.quarantine kule`.

### Linux

```sh
# Debian, Ubuntu
sudo apt-get install ./Kule_<version>_amd64.deb
# Fedora
sudo dnf install ./Kule-<version>.x86_64.rpm
```

İkisi de `kule-desktop` ve `kule`'yi `/usr/bin`'e koyar. Yalnızca komut
satırı için `.tar.gz`'yi açın ve `kule`'yi `PATH`'inize ekleyin.

:::warning Ubuntu 24.04 ve kullanıcı ad alanları

Kule'nin Linux sandbox'ı ayrıcalıksız kullanıcı ad alanlarına (unprivileged
user namespaces) ihtiyaç duyar; Ubuntu 24.04 bunları varsayılan olarak
kapatır. İlk ajan çalıştırmanızdan önce
[Sorun giderme](../troubleshooting.md) sayfasına (*Linux: agents do not
start*) bakın.

:::

### Windows

1. İndirdiğinizi doğrulayın (yukarıda). Kurucu ve programlar **kod imzası
   taşımaz**; bu yüzden SmartScreen yayımcının bilinmediğini söyler:
   doğruladıktan sonra **More info → Run anyway**'i seçin.
2. `Kule_<version>_x64-setup.exe`'yi (ARM64 bir bilgisayarda `_arm64-`)
   çalıştırın. Yalnızca sizin için, `%LOCALAPPDATA%\Kule` altına kurar ve
   yönetici istemez.
3. Komutu `PATH`'inize ekleyin: `%LOCALAPPDATA%\Kule`'yi kullanıcı
   `PATH`'inize ekleyin (*Settings → System → About → Advanced system
   settings → Environment Variables*) ya da PowerShell'de:

   ```powershell
   $user = [Environment]::GetEnvironmentVariable('Path', 'User')
   [Environment]::SetEnvironmentVariable('Path', "$user;$env:LOCALAPPDATA\Kule", 'User')
   ```

4. Kule'yi Başlat menüsünden açın.

Yalnızca komut satırı için `kule-<version>-windows-<arch>.zip`'i kendine
ait bir klasöre açın ve o klasörü `PATH`'inize ekleyin. İçindeki üç program
birlikte kalır ve klasör Kule'nin ev dizini (`%USERPROFILE%\.kule`)
değildir: bir görevin komutları Kule'nin terminal yardımcısının klasörünü
okuyabilir, Kule'nin ev dizinini ise asla okumaz; bu yüzden ikisi aynı
olduğunda Kule komutları reddeder.

:::warning Windows 11 24H2 ve masaüstü oturumu

Kule'nin Windows sandbox'ı, 26100.9278 ya da sonraki güncellemeyle Windows
11 24H2'ye ve bir masaüstü oturumuna ihtiyaç duyar. İkisinden biri eksikse
sandbox'lı her komut gerekçesiyle reddedilir; kabul edilecek daha zayıf bir
mod yoktur. Bkz. [Platformlar](../platforms.md).

:::

## Ajanları kurma

Kule ajanlarla birlikte gelmez. Her birini **Kule olmadan nasıl yapıyorsanız
öyle** kurar ve oturum açarsınız; Kule'nin çalıştırmaları o oturumu
kullanır.

| Ajan | Neye ihtiyaç duyar | Oturum açma |
| --- | --- | --- |
| **Claude Code** | Node.js (Kule, ACP adaptörünü `npx` ile başlatır) | Bir terminalde Claude Code'a bir kez oturum açın ya da `ANTHROPIC_API_KEY`'i ayarlayın. |
| **Codex** | Node.js (`npx`) | `codex login` (ChatGPT oturumu da olur) ya da `OPENAI_API_KEY`. |
| **Gemini** | Gemini CLI | Google AI Studio'dan alınan, bir dosyada saklanan API anahtarı: `GEMINI_FORCE_FILE_STORAGE=true gemini` çalıştırın, sonra `/auth`. |

Notlar:

- Gemini bir **AI Studio API anahtarı** ister: Google, Code Assist oturum
  açmayı artık Kule gibi istemcilere sunmuyor. Anahtar keychain'de değil,
  bir dosyada saklanmalıdır; çünkü sandbox keychain'i salt okunur tutar.
- Kule, `npx`'in indireceği her ajan adaptörünü **sürüm başına bir kez**,
  sandbox'ın dışında ve paketin kurulum betiklerini çalıştırmadan kurar:
  macOS'ta `~/Library/Application Support/Kule/packages` altına (Linux'ta
  ve Windows'ta profilinizin altında `~/.local/share/kule/packages`).
- Kule'ye verdiğiniz bir API anahtarını (*Settings → Secrets*'ta ya da
  `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` / `GEMINI_API_KEY` olarak) ajan
  asla görmez: ajan bir yer tutucu alır, anahtarı proxy doldurur. Bkz.
  [Gizli değerler](../concepts/secrets.md).
- Agent Client Protocol (ACP) konuşan başka ajanlar da eklenebilir. Bkz.
  [Ajanlar](../using-agents.md).

## Her şeyin çalıştığını doğrulama

```sh
kule --version
```

```text
kule 0.2.0-preview.20
```

(Sayı, derlediğiniz ya da indirdiğiniz sürümdür.)

Hangi ajanların başlayabildiği ve diğerlerinin neden başlayamadığı:

```sh
kule agents list
```

```text
claude-code      built in
gemini           built in
codex            built in

Add your own in ~/.kule/agents.toml (format: kule agents show claude-code).
```

Bu bilgisayarın sandbox'a ihtiyaç duyduğu her şeyi verip vermediği:

```sh
kule isolation
```

```text
this computer gives a sandboxed command all of its isolation
```

Linux'ta `kule isolation` bunun yerine eksik olan her şeyi listeler.
Windows'ta ya aynı satırı söyler ya da sandbox'lı komutların bu
bilgisayarda neden reddedildiğini. Bkz.
[`kule isolation`](../cli/isolation.md).

## Kule verilerini nerede tutar

Her şey **Kule'nin ev klasöründe** durur, varsayılan olarak `~/.kule`
(Windows'ta `%USERPROFILE%\.kule`): görevler ve kopyaları, ayarlar,
politika, gizli değerler. Başka bir klasör
kullanmak için `KULE_HOME`'u ayarlayın (ya da `--home` verin). Hiçbir
sandbox Kule'nin ev klasörünü göremez. Bkz.
[Dosyalar ve ayarlar](../reference/files.md).

## Sırada

- [Terminalde hızlı başlangıç](quick-start.md)
- [Uygulamada ilk göreviniz](first-task-in-the-app.md)

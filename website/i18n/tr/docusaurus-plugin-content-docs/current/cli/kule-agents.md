---
id: agents
title: kule agents
sidebar_label: kule agents
description: Kule'nin çalıştırabildiği ajanları listeleyin, profillerini inceleyin; kendi ajanlarınızı ekleyin, deneyin, onaylayın ya da kaldırın.
---

# kule agents

Kule'nin çalıştırabildiği ajanları listeleyin, her birinin profilinin
sandbox'ta neyi açtığını görün; kendi ajanlarınızı ekleyin, deneyin,
onaylayın ya da kaldırın.

## Kullanım {#synopsis}

```sh
kule agents [OPTIONS] [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-agents-list) | Ajanları ve çalışamayanların neden çalışamadığını listeler (varsayılan). |
| [`show`](#kule-agents-show) | Bir ajanın profilinin sandbox'ta neyi açtığını gösterir. |
| [`registry`](#kule-agents-registry) | ACP kayıt defterindeki ajanları listeler. |
| [`add`](#kule-agents-add) | ACP kayıt defterinden bir ajanı taslak profil olarak ekler. |
| [`try`](#kule-agents-try) | Bir profili boş bir klasörde bir kez çalıştırır ve sandbox'ın neyi reddettiğini gösterir. |
| [`approve`](#kule-agents-approve) | Profillerinizden birinin, şu anki hâliyle çalışmasına izin verir. |
| [`revoke`](#kule-agents-revoke) | Bir onayı geri alır. |
| [`remove`](#kule-agents-remove) | Profillerinizden birini `agents.toml`'dan çıkarır. |

## Açıklama {#description}

Kule, [Agent Client Protocol](https://agentclientprotocol.com) (ACP)
konuşan ajanları çalıştırır. Her ajanın bir **profili** vardır: onu
başlatan komut ve sandbox'ta görevin kopyasının ötesinde neleri
açabileceği (Kule'de kendine ait bir ev dizini, oturum bilgisi, ihtiyaç
duyduğu host'lar). Yazdığı hiçbir şey ev dizininize ulaşmaz.

- **Yerleşik**: `claude-code`, `codex` ve `gemini`.
- **Sizinkiler**: Kule'nin ev dizinindeki `agents.toml` içindeki profiller
  (`~/.kule/agents.toml`).
- **Bir eklentiden**: onaylanmış bir [eklentinin](../extensions.md)
  getirdiği profiller.

Bir profil sandbox'ta delikler açar; bu yüzden **sizin profillerinizden
biri ancak siz onayladıktan sonra çalışır**, ve her değişiklikten sonra
yeniden onay ister. Bir profil Kule'nin gizlediği kimlik bilgilerini,
başka bir ajanın durumunu, Kule'nin ev dizinini ya da `~/Documents` gibi
bütün bir dizini asla açamaz; indirdiği bir paket de tek ve tam bir sürüm
belirtmelidir. Profiller hiçbir zaman bir çalışma klasöründen gelmez.

Her profil alanı ve yerleşik ajanlarda nasıl oturum açılacağı için bkz.
[Ajanlar](../using-agents.md).

Bir ajan eklemenin olağan yolu:

```sh
kule agents registry            # kimliğini bulun
kule agents add qwen-code       # henüz hiçbir şey açmayan bir taslak
kule agents try qwen-code --apply   # bir kez çalıştırın; reddedilenleri ekleyin
kule agents show qwen-code      # neleri açacağını okuyun
kule agents approve qwen-code   # çalışmasına izin verin
```

Tüm alt komutlar `--home` alır (bkz. [genel seçenekler](index.md)).

## kule agents list

```sh
kule agents list [OPTIONS]
```

```console
$ kule agents
claude-code      built in
gemini           built in
codex            built in
qwen-code        yours, held: not approved as it stands; review it with `kule agents show qwen-code`, then `kule agents approve qwen-code`

Add your own in ~/.kule/agents.toml (format: kule agents show claude-code).
```

`held:`, ajanlarınızdan birinin şu anda neden çalışamadığını söyler: şu
anki hâliyle onaylanmamıştır (hiç onaylanmamış ya da o zamandan beri
değişmiştir), profili bir profilin açamayacağı bir şeyi açıyordur ya da
başka bir profil aynı adı taşıyordur.

## kule agents show

```sh
kule agents show [OPTIONS] <NAME>
```

Profili onaylayacağınız hâliyle yazar, ardından varsa uyarıları.

```console
$ kule agents show codex
[[agent]]  codex
runs              npx -y @agentclientprotocol/codex-acp@2.2.2 -c sandbox_mode="danger-full-access"
writes in ~/      nothing
its login         nothing
hidden from it    .codex (only Kule reads there)
copied in         .codex/AGENTS.md, .codex/AGENTS.override.md, .codex/config.toml, .codex/plugins, .codex/prompts, .codex/rules, .codex/skills (at every start, read-only)
sees your env     nothing
also connects to  https://auth.openai.com, https://chatgpt.com
session mode      agent-full-access
home of its own   $CODEX_HOME points at Kule's folder for it: what it writes in Kule stays there
its own home      /Users/you/Library/Application Support/Kule/agents/codex

warning: Kule puts your OPENAI_API_KEY into what codex sends to api.openai.com.
```

| Satır | Anlamı |
| --- | --- |
| `runs` | Ajanı ACP üzerinden başlatan komut. |
| `writes in ~/` | Ajanın durumunu yazdığı, ev dizininizin altındaki klasörler ve dosyalar. Ajan bunları görevin ev dizini kopyasına yazar; hiçbiri sizinkine ulaşmaz. |
| `its login` | Oturum bilgisi; diğer her ajandan gizlenir. |
| `hidden from it` | Ajanı Kule olmadan çalıştırdığınızda durumunu tuttuğu yer. Orayı yalnızca Kule okur. |
| `copied in` | Her başlangıçta Kule'deki ev dizinine kopyalanan ayarlarınız, hook'larınız, skill'leriniz ve talimatlarınız; ajan çalışırken salt okunurdur. |
| `protected` | Eski profillerden; artık gerekmiyor, çünkü yazdığı hiçbir şey ev dizininize ulaşmaz. |
| `reveals (macOS)` | macOS'ta okuyabildiği gizli yollar, örneğin bir anahtar zinciri. |
| `sees your env` | Olduğu gibi gördüğü ortam değişkenleriniz. |
| `sets env` | Kule'nin onu başlatırken verdiği değişkenler. |
| `also connects to` | Çalışırken politikanın ötesinde ulaştığı HTTPS host'ları. |
| `session mode` | Kule'nin ajanı başlattığı, sandbox işini Kule'ye bırakan ajanın kendi modu. |
| `its own home` / `home of its own` | Kule'de ona ait bir klasör (bkz. [Ajanlar](../using-agents.md) sayfasındaki `own_home`). |
| `warning:` | Onaylamadan önce bilmeniz gereken bir şey; örneğin Kule'nin onun için doldurduğu bir anahtar. |

Buradaki sandbox codex'in kendisininki değildir:
`sandbox_mode="danger-full-access"`, codex'in kendi sandbox'ını Kule'ninkinin
*içinde* kapatır; geçerli olan Kule'nin sandbox'ıdır.

## kule agents registry

```sh
kule agents registry [OPTIONS]
```

Herkese açık [ACP kayıt defterindeki](https://agentclientprotocol.com/registry)
ajanları, her birinin sürümü ve bu sistemde nasıl başladığıyla listeler.
Liste Kule'nin ev dizininde (`registry.json`) tutulur; kayıt defterine
ulaşılamadığında son kopya
`(the registry could not be reached; as last read)` notuyla gösterilir.

```console
$ kule agents registry
auggie                 0.36.0     npx -y @augmentcode/auggie@0.36.0 --acp
claude-acp             0.94.0     npx -y @agentclientprotocol/claude-agent-acp@0.94.0
goose                  1.55.0     download, then ./goose
qwen-code              0.25.3     npx -y @qwen-code/qwen-code@0.25.3 --acp --experimental-skills
cursor                 2026.10.18 not for this system
...
```

## kule agents add

```sh
kule agents add [OPTIONS] <ID>
```

Kayıt defterinden `agents.toml`'a bir **taslak** profil ekler. Taslak henüz
hiçbir şey açmaz: durum klasörü yok, oturum bilgisi yok, host yok. Siz
onaylayana kadar bekletilir.

```console
$ kule agents add qwen-code
added qwen-code, opening nothing yet; next: kule agents try qwen-code --apply, then kule agents approve qwen-code
```

Kayıt defterinin indirme olarak sunduğu bir ajan için `add`, hangi arşivi
indireceğinizi, SHA-256 değerini ve `PATH`'inize neyi koymanız gerektiğini
söyler.

## kule agents try

```sh
kule agents try [OPTIONS] <NAME>
```

| Seçenek | Anlamı |
| --- | --- |
| `--apply` | Denemenin önerdiklerini profile ekler. Profil sonra yeniden onay bekler. |

**Sizin** profillerinizden birini boş bir klasörde, profilin söylediğinden
daha geniş açılmadan bir kez çalıştırır (en fazla iki dakika), ardından
sandbox'ın ona neleri reddettiğini listeler: yazmak istediği klasörler ve
ulaşmak istediği host'lar. Profilinin açması gereken şey budur.

Çıktısı şu biçimdedir (neyin reddedilip önerildiği ajana bağlıdır):

```text
trying qwen-code in an empty folder (up to two minutes)…
it answered: …
refused  file-write-create /Users/you/.qwen/settings.json (node)
refused  connect example-model-api.com:443 (no-rule-lets-it-pass)
suggested state: .qwen
suggested hosts: example-model-api.com
added to qwen-code; review it with kule agents show qwen-code, then approve it
```

Ajanın önerilen durumda yazdıkları her görevin ev dizini kopyasına gider,
asla sizinkine. Durumunun yerini bir değişkenden okuyan bir ajana bunun
yerine görevler boyunca tutulan kendine ait bir ev dizini (`own_home`)
verilebilir.

:::note

`try` ajanı gerçekten başlatır; bu yüzden oturum açmanızı isteyebilir ve
sağlayıcısındaki hesabınızı kullanabilir. Linux reddedilen dosyaları
bildirmediği için orada yalnızca host'lar önerilir.

:::

## kule agents approve

```sh
kule agents approve [OPTIONS] <NAME>
```

Profili yazar, sonra onu **şu anki hâliyle** onaylar. Profilde sonradan
yapılan her değişiklik, elle ya da `try --apply` ile, onu yeniden bekletir.

```console
$ kule agents approve qwen-code
[[agent]]  qwen-code
runs              npx -y @qwen-code/qwen-code@0.25.3 --acp --experimental-skills
...

approved qwen-code as it stands; a change to it needs approval again
```

## kule agents revoke

```sh
kule agents revoke [OPTIONS] <NAME>
```

```console
$ kule agents revoke qwen-code
qwen-code is no longer approved
```

## kule agents remove

```sh
kule agents remove [OPTIONS] <NAME>
```

Profillerinizden birini `agents.toml`'dan kaldırır. Yerleşik ajanlar
kaldırılamaz.

```console
$ kule agents remove qwen-code
removed qwen-code
```

## Hatalar ve çözümleri {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: no agent nope` | Adı `kule agents` ile kontrol edin. |
| `kule: no agent nope in the ACP registry` | Kimliği `kule agents registry` ile kontrol edin. |
| `kule: there is a profile named qwen-code already` | Önce var olan profili düzenleyin ya da kaldırın. |
| `kule: qwen-code has nothing to run on this system` | Kayıt defteri bu işletim sistemi ya da işlemci için bir derleme sunmuyor. |
| `kule: no profile qwen-code of yours` | `try` ve `remove` yerleşik ajanlarda değil, yalnızca kendi profillerinizde çalışır. |
| `held: not approved as it stands; …` | `kule agents show NAME` çıktısını okuyun, sonra `kule agents approve NAME`. |

## Uygulamada {#in-the-app}

*Settings → Agents* her ajanı açtıklarıyla birlikte listeler. **Try it** bir
taslağı bir kez çalıştırır ve reddedilenleri onay kutuları olarak sunar;
böylece bir telemetri host'unu dışarıda bırakabilirsiniz. Onaylama da aynı
şekilde çalışır. İlk açılış ekranı hangi ajanların başlayabildiğini ve
diğerleri için neyin kurulması gerektiğini söyler.

## Ayrıca bakın {#see-also}

- [Ajanlar](../using-agents.md): her profil alanı ve oturum açma.
- [`kule run`](run.md): bir ajana bir prompt verin.
- [Dosyalar ve ayarlar](../reference/files.md): `agents.toml`.

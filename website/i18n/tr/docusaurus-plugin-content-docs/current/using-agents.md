---
title: Ajanlar
sidebar_label: Ajanlar
description: Kule'nin hazır çalıştırdığı ajanlar, sandbox içinde nasıl çalıştıkları ve başka herhangi bir ACP ajanının agents.toml'daki bir profille nasıl ekleneceği.
---

# Ajanlar

**Ajan**, bir görevde işi yapan yapay zekâ programıdır: Claude Code, Codex,
Gemini ya da [Agent Client Protocol](https://agentclientprotocol.com)
(ACP) konuşan başka herhangi bir program. Kule ajanı başlatır, ona
prompt'unuzu verir ve ne yaptığını gösterir. Ajan, görevin kopyasında,
görevin [sandbox'ı](concepts/sandbox.md) içinde çalışır; ağ trafiği de
görevin proxy'si üzerinden geçer.

## Hazır ajanlar {#built-in-agents}

| Ajan | Kule'deki adı | Neye ihtiyaç duyar | Nasıl oturum açar |
| --- | --- | --- | --- |
| **Claude Code** | `claude-code` | Node.js (`npx`) | Kule olmadan nasılsa öyle: bir terminalde `claude` ya da `ANTHROPIC_API_KEY`. |
| **Codex** | `codex` | Node.js (`npx`) | Bir ChatGPT hesabıyla `codex login` ya da `OPENAI_API_KEY`. |
| **Gemini** | `gemini` | Gemini CLI (`PATH`'inizde `gemini`) | Google AI Studio'dan bir API anahtarı (aşağıya bakın). |

Varsayılan `claude-code`'dur (`kule run TASK "prompt"` onu kullanır).
Başka bir ajanı `kule run TASK --agent codex "prompt"` ile ya da uygulamada
prompt'un altındaki ajan seçiciyle seçin.

Her ajanda bir kez, Kule olmadan nasıl yapıyorsanız öyle oturum açın.
Kule'nin çalıştırmaları bu oturumu kullanır. Uygulamanın ilk ekranı ve
`kule agents list`, hangi ajanların başlayabildiğini ve diğerlerinde neyin
eksik olduğunu söyler.

### Claude Code {#claude-code}

- Kule, Claude Code'u ACP bağdaştırıcısı (adapter) üzerinden başlatır; bu,
  sürümü sabitlenmiş bir npm paketidir
  (`@agentclientprotocol/claude-agent-acp`).
- macOS'ta Claude Code'un oturumu anahtar zincirinde (keychain) durur ve
  anahtar zinciri sandbox'tan gizli kalır. Kule token'ı oradan kendisi
  okur ve ajana bir yer tutucu verir; görevin proxy'si gerçek token'ı
  yalnızca `api.anthropic.com`'a giden isteklere koyar.
- Ortamınızdaki `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` ve
  `CLAUDE_CODE_OAUTH_TOKEN` de aynı şekilde işlenir: ajan anahtarı değil,
  bir yer tutucuyu görür.

### Codex {#codex}

- Kule, Codex'i ACP bağdaştırıcısı (`@agentclientprotocol/codex-acp`)
  üzerinden başlatır.
- Codex'in kendi komut sandbox'ı Kule'ninkinin içinde kurulamaz; bu yüzden
  Kule onu tam erişim (full-access) modunda başlatır ve onun yerini Kule'nin
  sandbox'ı tutar.
- ChatGPT oturumu (`codex login`) çalışır: token'ını `auth.openai.com`'da
  yeniler ve modele `chatgpt.com` üzerinden ulaşır; Codex çalışırken bu iki
  adres açıktır. `OPENAI_API_KEY` yalnızca `api.openai.com`'a, bir yer
  tutucu olarak gider.
- ChatGPT ile oturum açılmışken Codex token sayısını bildirir ama maliyeti
  bildirmez; bu yüzden bir [bütçe](concepts/budgets-and-limits.md) onu
  sınırlayamaz.

### Gemini {#gemini}

- Gemini, **Google AI Studio'dan bir API anahtarına** ihtiyaç duyar.
  Google, Code Assist oturumunu artık bu tür istemcilere sunmuyor.
- Anahtarı, sandbox'ın salt okunur tuttuğu anahtar zincirinin dışına bir
  kez kaydedin:

  ```sh
  GEMINI_FORCE_FILE_STORAGE=true gemini
  # ardından, Gemini'nin içinde:
  /auth
  ```

- Ya da `GEMINI_API_KEY`'i (veya `GOOGLE_API_KEY`'i) ayarlayın: ajan bir
  yer tutucu görür ve proxy anahtarı yalnızca
  `generativelanguage.googleapis.com`'a giden isteklere koyar.
- Gemini ne token sayısını ne de maliyeti bildirir.

## Bir ajan nasıl çalışır {#how-an-agent-runs}

Bir göreve prompt verdiğinizde (`kule run` ya da uygulamada **Run**):

1. Kule ajanı **profiliyle** başlatır: onu başlatan komut ve sandbox'ta
   ihtiyaç duyduğu dar ek alan (kendi durum (state) dizini ve oturumu,
   host'ları).
2. Ajan görevin kopyasında, sandbox'ın içinde çalışır. Diğer ajanların
   oturumları ve konuşma kayıtları ondan gizli kalır.
3. Yaptığı her araç çağrısı bir **adım** olur (`kule steps TASK` ya da
   [Timeline](app/timeline.md)). Politikanın size sorduğu bağlantılar
   [soru](concepts/network-and-questions.md) olarak bekler.
4. Siz [commit](concepts/review-commit-undo.md) edene kadar klasörünüze
   hiçbir şey ulaşmaz.

### Bir ajanın sonrası için değiştiremeyeceği şeyler {#what-an-agent-may-not-change-for-later}

Bir ajanın kendi dizini (`~/.claude`, `~/.codex`, `~/.gemini`) sandbox'ın
dışındadır; ajanın Kule'de oraya yazdıkları, onu Kule olmadan bir sonraki
başlatışınızda ona ulaşır. Bu yüzden bir ajanın başlarken bir şey
çalıştırmasına ya da talimat izlemesine yol açan şeyler sandbox'ta **salt
okunurdur**: ayarlar, hook'lar, eklentiler (plugin'ler, extension'lar),
skill'ler, komutlar, kurallar, güvenilen klasörler ve `CLAUDE.md`,
`AGENTS.md` ya da `GEMINI.md` dosyası. Kule'nin kendi profilleri bunları
kapsar; kendi profilleriniz için bir deneme (trial) bunları önerir ve
`kule agents show` yazılabilir bırakılmış olanlar için uyarır.

Ya da bir ajana `own_home` ile Kule'de **kendine ait bir ev dizini** verin
(aşağıya bakın): o zaman Kule'de yazdığı hiçbir şey dışarıda ona ulaşmaz;
oturumu orada bir kez açarsınız.

### npx ile getirilen paketler {#packages-fetched-with-npx}

`npx`'in getirdiği bir ajan (Claude Code'un ve Codex'in bağdaştırıcıları)
**her sürüm için bir kez**, sandbox'ın dışında ve paketin kurulum
betikleri çalıştırılmadan, Kule'nin taşıdığı bir lockfile'dan, her
bağımlılık kendi bütünlük (integrity) hash'iyle kurulur. Şuraya kurulur:

- macOS: `~/Library/Application Support/Kule/packages`
- Linux: `~/.local/share/kule/packages`

Hiçbir sandbox oraya yazamaz. Ajan oradan `node` ile başlar; böylece yeni
bir görev onu kendi npm önbelleğine yeniden indirmez.

## Kendi ajanınızı eklemek {#adding-your-own-agent}

ACP konuşan herhangi bir ajan, `~/.kule/agents.toml` dosyasına (ayarladıysanız
`KULE_HOME` içine) bir **profil** olarak eklenebilir. Kule'nin ev dizini
her sandbox'tan gizlidir; bu yüzden hiçbir ajan kendisi için bir profil
yazamaz ve profiller hiçbir zaman bir çalışma klasöründen okunmaz.

### Hızlı yol: ACP kayıt listesinden {#the-quick-way-from-the-acp-registry}

```sh
kule agents registry            # ACP kayıt listesindeki ajanları listele
kule agents add qwen-code       # oradan taslak bir profil al, henüz hiçbir şey açmadan
kule agents try qwen-code       # boş bir klasörde bir kez çalıştır; sandbox'ın reddettiklerini listele
kule agents try qwen-code --apply   # reddedilenleri profile ekle
kule agents show qwen-code      # profilin neleri açtığını ve uyarıları oku
kule agents approve qwen-code   # şimdiki haliyle çalışmasına izin ver
```

`kule agents try`, profili boş bir klasörde, taslağından daha geniş
açılmadan bir kez çalıştırır, ardından sandbox'ın ona reddettiklerini
listeler: yazmak istediği dizinler ve erişmek istediği host'lar. `--apply`
bunları profile ekler; profil de ardından her değişiklik gibi onayınızı
bekler. Linux reddedilen dosyaları bildirmez; bu yüzden orada yalnızca
host'lar önerilir.

Uygulamada *Settings → Agents* aynı şeyi yapar: **Add from the ACP
registry**, ardından **Try it**; bir telemetri host'u dışarıda kalabilsin
diye onay kutularıyla.

### Elle {#by-hand}

Profili, `kule agents show claude-code`'un yazdırdığı biçimde kendiniz
yazın:

```toml
[[agent]]
name = "opencode"
command = ["npx", "-y", "opencode-ai@1.2.3", "acp"]
state = [".local/share/opencode"]          # ~ altında yazabileceği yer
login = [".local/share/opencode/auth.json"] # diğer tüm ajanlardan gizli
keys = { OPENROUTER_API_KEY = ["openrouter.ai"] } # API anahtarınız, ajan onu hiç görmez
hosts = ["openrouter.ai"]                   # çalışırken eriştiği HTTPS host'ları
```

Ardından `kule agents show opencode` ve `kule agents approve opencode`.

### Onay {#approval}

Bir profil sandbox'ta delikler açar; bu yüzden sizin profilleriniz
yalnızca **siz onayladıktan sonra**, onayladığınız haliyle çalışır. Sonradan
herhangi biri tarafından değiştirilirse yeniden onay bekler.
`kule agents revoke NAME` bir onayı geri alır; `kule agents remove NAME`
profili `agents.toml`'dan çıkarır.

## `agents.toml` alan başvurusu {#agentstoml-field-reference}

Her ajan bir `[[agent]]` tablosudur. Bilinmeyen alanlar reddedilir. Yollar
ev dizininize göredir.

| Alan | Tip | Nedir |
| --- | --- | --- |
| `name` | string, zorunlu | Ajanın nasıl istendiği (`kule run --agent NAME`). Küçük harfler, rakamlar ve tireler. Kule'nin kendi ajanlarından birinin adı olamaz. |
| `command` | string listesi, zorunlu | Ajanı stdio üzerinden ACP konuşacak şekilde başlatır. `npx`, `bunx`, `pnpx`, `uvx` ya da `pipx` ile başlatılan bir paket **tek bir kesin sürüm** belirtmelidir (`npx -y name@1.2.3`). |
| `state` | yol listesi | Ajanın kendi durumunu yazdığı yer, `~` altında. Birden fazla ajanınkinden çok daha fazlasını tutan bir dizin olamaz (`.config`, `.local/share`, `.cache`, `Library/Application Support`, `Documents`, …); bir kabuk, oturum ya da araç başlatırken çalışan bir şey de olamaz (`.zshrc`, `.gitconfig`, `.config/fish`, `.local/bin`, `Library/LaunchAgents`, …). |
| `protected` | yol listesi | Durumunun içinde, daha sonra sandbox'ın dışında neyin çalışacağını yapılandıran yollar (ayarlar, hook'lar, MCP sunucuları, plugin'ler, talimatlar): okunabilir, asla yazılamaz. |
| `login` | yol listesi | Durumunun içindeki oturum bilgisi. Diğer tüm ajanlardan ve Kule'nin kendi çalıştırdığı komutlardan gizlidir. |
| `keys` | tablo: değişken → host listesi | Ajanın API anahtarlarını tutan ortam değişkenleriniz; her biri hangi host'lar için olduğuyla. Ajanın değişkeni bir yer tutucu tutar; görevin proxy'si anahtarı yalnızca o host'lara giden isteklere, HTTPS üzerinden koyar. |
| `keychain` | tablo: değişken → `{ service, json, hosts }` | macOS: ajanın anahtar zincirinde tuttuğu bir anahtar, onu okuduğu değişkenle. Kule onu oradan okur (`service` anahtar zinciri öğesidir; `json`, varsa öğenin JSON'ı içindeki noktalı yoldur) ve `keys`'teki gibi ajana bir yer tutucu verir. Ortamınızda aynı adlı bir anahtar varsa o kazanır. |
| `hosts` | host listesi | Oturumu ve modeli için, politikanın geçirdiklerinin ötesinde ihtiyaç duyduğu HTTPS host'ları. Yalnızca bu ajan çalışırken açıktır. |
| `env` | tablo: değişken → değer | Ajanın başlatıldığı değişkenler. |
| `pass_env` | değişken adı listesi | Ajanın olduğu gibi görebileceği ortam değişkenleriniz. Diğerleri dışarıda kalır. Gizli olan her şey için `keys`'i tercih edin. |
| `own_home` | değişken adı | Durumunun nerede olduğunu bir değişkenden okuyan bir ajan için Kule'de kendine ait bir ev dizini (`CODEX_HOME`, `CLAUDE_CONFIG_DIR` ya da `HOME`'un kendisi). Kule değişkeni ajanın kendine ait bir klasörüne yönlendirir; böylece Kule'de yazdığı şey dışarıda ona hiç ulaşmaz; oturumu orada bir kez açarsınız. `PATH`, `TMPDIR`, `TMP`, `TEMP`, `PWD` ya da `SHELL` olamaz. |
| `reveal_on_macos` | yol listesi | macOS'ta okuması gereken, varsayılan olarak gizli yollar. Yalnızca `Library/Keychains` açılabilir. |
| `mode` | string | Ajan sunuyorsa başlanacak ACP oturum modu: kendi sandbox'ı Kule'ninkinin içinde başlayamayan bir ajan için. |
| `devices` | yol listesi | Linux: ajanın ve komutlarının, her komutun aldıklarının ötesinde ihtiyaç duyduğu `/dev` altındaki aygıtlar; örneğin `/dev/snd`, `/dev/video0` ya da bir seri port. Kule size ait olana ulaşanları reddeder: diskler, bellek, klavye ve fare, konsollar ve terminaller. |

Kendi ev dizini klasörleri, sandbox'ın tamamen gizlediği Kule ev dizininin
dışında durur: macOS'ta `~/Library/Application Support/Kule/agents`, Linux'ta
`~/.local/share/kule/agents`.

### Bir profilin asla açamayacakları {#what-a-profile-may-never-open}

Kule her profili çalışmadan önce denetler. Bir profil şunları asla açamaz:

- Kule'nin gizlediği kimlik bilgileri (`~/.ssh`, `~/.aws`, …);
  `reveal_on_macos` üzerinden macOS anahtar zinciri hariç;
- başka bir ajanın durumu ya da oturumu;
- Kule'nin kendi ev dizini;
- `~/Documents`, `~/.config` ya da `~/Library` gibi bütün bir dizin;
- bir kabuk, oturum ya da araç başlatırken çalışan şeyler;
- tek bir kesin sürüm belirtmeyen bir paket.

### Linux'ta aygıtlar {#devices-on-linux}

Bir görevdeki her komut şu aygıtları adıyla alır: `/dev/null`, `zero`,
`full`, `tty`, kendi terminalleri, okumak için `random` ve `urandom`, ve
GPU [sınırı](concepts/budgets-and-limits.md) onu kapatmadıkça GPU'nun
aygıtları. Asla `/dev`'in tamamı değil; orada oturumunuzun bir kamerası, bir
mikrofonu ya da bir USB aygıtı olabilir. Daha fazlasına ihtiyaç duyan bir
ajan bunları `devices` altında belirtir.

## Terminalden {#from-a-terminal}

Her alt komut için bkz. [`kule agents`](cli/kule-agents.md); bir ajana prompt
vermek için [`kule run`](cli/run.md). Uygulamada bkz.
[Settings](app/settings.md) → Agents.

---
title: Ortam değişkenleri
sidebar_label: Ortam değişkenleri
description: Kule'nin okuduğu ortam değişkenleri ve bir görevde çalıştırdığı komutlar için ayarladıkları.
---

# Ortam değişkenleri

Bu sayfa, Kule'nin kendisinin okuduğu değişkenleri ve bir görevin içindeki
bir komutun ortamının neye benzediğini listeler.

## Kule'nin okuduğu değişkenler {#variables-kule-reads}

| Değişken | Okuyan | Anlamı |
| --- | --- | --- |
| `KULE_HOME` | `kule`, daemon, uygulama | Kule'nin ev dizini. Varsayılan `~/.kule` (Windows'ta `%USERPROFILE%\.kule`). Her komuttaki `--home` ile aynıdır. Kısa tutun: içindeki daemon soketinin yolu 100 baytın altında kalmalıdır. Varsayılan dışında bir ev dizini, gizli değerlerin anahtarını sistemin deposu (anahtar zinciri, Secret Service, Credential Manager) yerine bir dosyada (`secrets.key`) tutar. Windows'ta Kule'nin programlarının bulunduğu klasör olmamalıdır. |
| `HOME` (Windows'ta `USERPROFILE`) | her şey | Ev dizininiz: `~/.kule`'nin bulunduğu yer ve sandbox'ın kendi klasörlerinizi altında gizlediği yer. |
| `PATH` | uygulama, sandbox | Komutların ve ajanların (`npx`, `node`, `gemini`, …) bulunduğu yerler. Finder'dan ya da bir başlatıcıdan açıldığında uygulama `PATH`'i giriş kabuğunuzdan okur; böylece bir terminalle aynı araçları bulur. |
| `SHELL` | `kule shell`, uygulamanın terminalleri | Bir görevin kopyasında açılacak kabuk. |
| `NO_COLOR` | `kule` | Renkleri kapatmak için (boş olmayan bir değerle) ayarlayın. Çıktı bir terminal değilse ya da `TERM=dumb` ise de renk kapalıdır. |
| `KULE_TERMINAL` | uygulama | *Open in terminal app*'in kullandığı terminal uygulaması: macOS'ta bir uygulama adı (örneğin `iTerm`), Linux'ta bir komut. Ayarlı değilse macOS, uygulamanın başlatıldığı terminali, o da yoksa Terminal'i kullanır; Linux `PATH` üzerindeki bilinen terminalleri dener. |
| `KULE_NO_EXTENSIONS` | uygulama | Uygulamayı `--no-extensions` gibi, tüm eklentiler kapalı başlatmak için ayarlayın. |
| `CARGO_HOME` | sandbox | Bir görevin kendi cargo önbelleğini başlatmak için sizin cargo önbelleklerinizin bulunduğu yer. |
| `XDG_CONFIG_HOME` | sandbox | Gerçek `~/.config` dizininizi gösterdiğinde, bir komut onun yerine ev katmanının `.config` dizinini alır. |
| `XDG_RUNTIME_DIR` | sandbox (Linux) | Oturum soketlerinin durduğu çalışma zamanı dizininiz; bir komutun erişiminin dışında tutulur. |
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, … | yerleşik ajanlar | Yalnızca ajanın kendi API host'una giden **isteklere doldurulmak** için okunur; ajan bir yer tutucu görür. Bkz. [Ajanlar](../using-agents.md) sayfasındaki `keys`. |

## Bir görevdeki komutun gördükleri {#what-a-command-in-a-task-sees}

`kule exec`'in, `kule run`'ın ajanının ve uygulamanın ajanlarının bir
görevde başlattığı komutlar **kırpılmış bir ortam** alır. Yalnızca
araçların kendilerini bulması için gerekenler geçer; ortamınızdaki
token'lar, anahtarlar ve ajan soketleri dışarıda kalır.

### Olduğu gibi geçenler {#passed-through-as-they-are}

`PATH`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LANGUAGE`, her `LC_*`, `TZ`,
`TERM`, `COLORTERM`, `NO_COLOR`, `FORCE_COLOR`, `EDITOR`, `PAGER`,
`__CF_USER_TEXT_ENCODING`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `RUSTUP_HOME`,
`RUSTUP_TOOLCHAIN`, `GOPATH`, `GOROOT`, `JAVA_HOME`, `NVM_DIR`, `NVM_BIN`,
`VOLTA_HOME`, `PYENV_ROOT`, `VIRTUAL_ENV`, `CONDA_PREFIX`, `DOTNET_ROOT`,
`ANDROID_HOME`, `SDKROOT`, `DEVELOPER_DIR`, `HOMEBREW_PREFIX`,
`HOMEBREW_CELLAR`, `HOMEBREW_REPOSITORY`.

Tek bir komut için bir tane daha geçirmek için: `kule exec --pass-env NAME`.
Bir ajan profili kendininkileri `pass_env` altında adlandırır.

### Kule'nin ayarladıkları {#set-by-kule}

| Değişken | Değer | Neden |
| --- | --- | --- |
| `KULE_TASK` | görevin kimliği | `kule exec`, `kule shell` ve uygulamanın terminalleri tarafından ayarlanır; böylece bir betik hangi görevde çalıştığını anlayabilir. |
| `HOME` | `<task>/home` | Görevin [ev katmanı](../concepts/home-layer.md): oraya yazılanlar inceleme için yakalanır. |
| `XDG_CONFIG_HOME` | `<task>/home/.config` | Yalnızca sizinki gerçek `~/.config` dizininizi gösteriyorduysa. |
| `TMPDIR` | `<task>/tmp` | Görevin kendi geçici dizini. |
| `TMPPREFIX` | `<task>/tmp/zsh` | zsh'nin here-document'ların ve `=(…)`'nin dosyalarını koyduğu yer; varsayılan olarak `/tmp`. |
| `CLAUDE_CODE_TMPDIR` | `<task>/tmp` | Claude Code'un kendi geçici dosyalarını tuttuğu yer; varsayılan olarak `/tmp`. |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` (ve küçük harfli hâlleri) | `http://127.0.0.1:<port>` | Her bağlantıyı değerlendirip kaydeden görevin proxy'si. `NO_PROXY` kaldırılır. |
| `NODE_USE_ENV_PROXY` | `1` | Node'un yerleşik `fetch`'inin proxy'yi kullanmasını sağlar. |
| `SSL_CERT_FILE`, `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, `PIP_CERT`, `GIT_SSL_CAINFO`, `CARGO_HTTP_CAINFO`, `AWS_CA_BUNDLE` | `<task>/tmp/kule-ca-bundle.pem` | Sistemin kök sertifikaları artı görevin kendi sertifika yetkilisi; proxy bunu [gizli değerleri](../concepts/secrets.md) host'ları için doldurmakta kullanır. |
| `NODE_EXTRA_CA_CERTS` | `<task>/tmp/kule-ca.pem` | Aynı yetkili, Node için. |
| `GODEBUG` | `x509sslcertoverrideplatform=1` ekler | macOS: Go'nun sertifikaları `SSL_CERT_FILE`'a göre denetlemesini sağlar. Kendi `GODEBUG` ayarlarınız korunur. |
| `CARGO_HOME` | `<task>/tmp/caches/cargo` | Görevin kendi cargo önbelleği (cargo ayarlarınızla, ama asla `credentials.toml` olmadan). |
| `npm_config_cache` | `<task>/tmp/caches/npm` | Görevin kendi npm önbelleği. |
| `PNPM_HOME`, `pnpm_config_store_dir` | `<task>/tmp/caches/pnpm` altında | pnpm. |
| `YARN_CACHE_FOLDER`, `PIP_CACHE_DIR`, `UV_CACHE_DIR`, `GOCACHE`, `GOMODCACHE`, `NUGET_PACKAGES`, `XDG_CACHE_HOME` | `<task>/tmp/caches/` altında | Her aracın önbelleği; görevin kendisine ait. |
| `NODE_OPTIONS`, `PYTHONPATH`, `RUBYOPT` | `<task>/tmp/.kule-loopback/` içinden küçük bir dosya yükler | Her adresi isteyen Node, Python ve Ruby sunucularını yalnızca bu bilgisayara yönlendirir. (Linux'ta Kule ayrıca her programın sunucularını kendisi loopback'te tutar.) Kendi değerleriniz korunur. |
| saklanan bir gizli değerin adı, örn. `STRIPE_KEY` | `kule_secret_…` | Bir yer tutucu; proxy değeri yalnızca o gizli değerin host'ları için koyar. |

`kule exec --no-network` ile hiçbir proxy değişkeni ayarlanmaz ve TCP
kapatılır. `--open-network` ile komutlar proxy ve politika olmadan
doğrudan bağlanır. `--no-sandbox` ile yukarıdakilerin hiçbiri geçerli
değildir.

macOS'ta cargo ve npm önbellekleri sizinkilerin copy-on-write klonları
olarak başlar; böylece görev onlara bir şey ekleyene kadar yer kaplamazlar.
Linux'ta boş başlarlar.

### Kendiniz bakın {#look-for-yourself}

```sh
kule exec TASK -- env | sort
```

## Ayrıca bakın {#see-also}

- [Dosyalar ve ayarlar](files.md)
- [Sandbox](../concepts/sandbox.md), [Ağ ve sorular](../concepts/network-and-questions.md)
- [`kule exec`](../cli/exec.md)

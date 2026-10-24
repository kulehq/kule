---
title: Politika
sidebar_label: Politika
description: Kule'nin Cedar politikası her değişikliği ve bağlantıyı nasıl kendiliğinden geçer, onay bekler ya da reddedilir diye karara bağlar; balanced ve careful politikaları, kendi politikanız ve hatırlanan onaylar.
---

# Politika

Bir görevin yaptığı her değişiklik ve kurduğu her ağ bağlantısı, klasörünüze
ya da ağa ulaşmadan önce bir **politika** tarafından değerlendirilir.
Politika, izin kuralları için küçük bir dil olan
[Cedar](https://www.cedarpolicy.com) ile yazılır.

## Üç karar

| Karar | Ne olur | Nasıl görünür |
| --- | --- | --- |
| **pass** (kendiliğinden geçer) | Değişiklik commit'te içeri girer; bağlantı kurulur. | `policy: passes on its own` |
| **ask** (onay bekler) | Değişiklik onayınızı bekler; bağlantı yanıtınızı bekler. | `policy: waits for approval` |
| **deny** (reddedilir) | Siz onaylasanız bile asla içeri girmez. | `policy: denied` |

Kendiliğinden geçmeyenler nedenlerine göre bir araya toplanır: sade
sözcüklerle bir neden, onu durduran **kural** ve altında yollar:

```text
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    package.json
```

Karar veremeyen bir kural ya da hatalı bir politika reddeder.

## Yerleşik politikalar

Birini ilk açılış ekranında, *Settings → Policy* bölümünde ya da
[`kule policy`](../cli/policy.md) ile seçersiniz:

### Balanced (varsayılan)

**Kendiliğinden geçenler:**

- kaynak kod, belge ve veri dosyalarındaki (kaynak kod, notlar, belgeler,
  veri dosyaları, resimler ve fontlar) ve projenizin git'te zaten izlediği
  dosya türlerindeki değişiklikler;
- ileri taşınan (fast-forward) bir git dalı ya da yeni bir dal;
- HTTPS üzerinden paket kayıt depolarına (npm, Yarn, crates.io, NuGet,
  PyPI, RubyGems, Maven Central) bağlantılar ve her ajanın, o ajan
  çalışırken kendi model API'sine bağlantıları.

**Sizi bekleyenler (ask):**

- diğer dosya türleri, betikler (`.sh`, `.ps1`, `.bat`, …) de dahil; siz
  türü bir kez kalıcı olarak yanıtlayana ya da proje onu izleyene kadar;
- görevin gizli bir dosyaya ya da klasöre (noktayla başlayan bir yol
  parçası) yazdığı her şey, türü ne olursa olsun;
- manifest ve kilit dosyaları: `package.json`, `package-lock.json`,
  `yarn.lock`, `Cargo.toml`, `Cargo.lock`, `go.mod`, `pyproject.toml`,
  `requirements*.txt`, `*.csproj`, `pom.xml`, `build.gradle` ve
  diğerleri;
- kendi başına kod çalıştıran ya da bir ajanı yönlendirenler: CI
  (`.github`, `.gitlab-ci*`, …), editör ayarları (`.vscode`, `.idea`), dev
  container'lar, hook yöneticileri (`.husky`, `lefthook.yml`), araç zinciri
  ve paket yöneticisi ayarları (`.npmrc`, `rust-toolchain.toml`,
  `build.rs`), bir ajanın ayarları, talimatları ve MCP sunucuları
  (`.claude`, `CLAUDE.md`, `AGENTS.md`, `.mcp.json`, …) ve Kule'nin kendi
  `.kule/` klasörü;
- klasörün içindeki bir depo (repository) ve sembolik bağlantılar;
- git config değişiklikleri, geçmişin yeniden yazılması, dal silme,
  `HEAD`'i değiştirme, etiketler ve diğer ref'ler;
- **toplu silmeler**: 200'den fazla silme ya da git deposu olmayan bir
  klasörde 20'den fazla silme (yeniden adlandırılan bir dosya silme değil,
  taşıma sayılır);
- git sunucuları dahil diğer tüm bağlantılar.

**Reddedilenler:** git hook'ları, iç içe bir deponun hook'ları da: bir
sonraki git komutunuzda sizin kimliğinizle çalışırlar.

Adlar her klasörde ve büyük/küçük harf nasıl yazılırsa yazılsın eşleşir.

### Careful

Bir dosyadaki ya da git'teki her değişiklik sizi bekler. Yalnızca paket
kayıt depolarına ve her ajanın, çalışırken kendi API'sine sormadan
ulaşılır.

```sh
kule policy careful     # her değişiklik bekler
kule policy balanced    # varsayılana dön
kule policy             # şu an geçerli kuralları yazdır
```

Her kuralın kimliğiyle birlikte tam kural listesi
[Yerleşik politika kuralları](../reference/policy-rules.md) sayfasındadır.

## Kendi politikanız

Yerleşik politikanın yerine geçmesi için kendi Cedar politikanızı
`~/.kule/policy.cedar` olarak kaydedin. Kule onun üzerine asla yazmaz.
`kule policy`'nin yazdırdığından başlayın.

## Bir deponun kendi kuralları

Bir depo `.kule/policy.cedar` içinde **ek retler** (Cedar `forbid`
kuralları) tanımlayabilir. Bunlar ajanın düzenlediği kopyadan değil, her
zaman gerçek çalışma klasöründen okunur ve politikayı yalnızca daha sıkı
yapabilir.

## Bir grubun politikası

Bir [grup](groups.md), görevlerini kendi politikasıyla değerlendirebilir:
tüm görevler için geçerli olan (varsayılan), diğerleri ne kullanırsa
kullansın balanced ya da careful, veya grup için sizin yazdığınız bir
politika. Bkz. [`kule group policy`](../cli/group.md).

## Onaylamak ve hatırlamak

Bir şey sizi beklediğinde:

- `kule commit TASK --approve` ya da uygulamadaki **Approve and commit**
  onu içeri alır. Reddedilen değişiklikler dışarıda kalır.
- `--remember` (ya da *Don't ask again in this folder*), şimdi onaylanan
  dosyalar için bu klasörde bir daha sormaz: manifestler, proje dosyaları
  ve gizli dosyalar yollarıyla, projede hiç olmayan dosya türleri
  türleriyle hatırlanır.
- Bir bağlantı sorusundaki **Approve and remember**, o host'u ve portu bu
  klasörde geçirir.

Bir izin kalıcı olarak saklanmadan önce Kule, geçmiş görevlerinizde neyi
geçireceğini gösterebilir:

```sh
kule rules allow api.example.com:443        # neyi geçireceğini gösterir
kule rules allow api.example.com:443 --yes  # saklar
kule rules                                  # Kule'ye söylediklerinizi listeler
kule rules forget ID                        # yeniden sorar
```

Hatırlanan bir bağlantı bir Cedar `permit` kuralına dönüşür; hatırlanan bir
dosya ya da tür ise onu soran kurala dar bir istisnaya. **Asla
hatırlanmayanlar:** reddedilenler, dosyanın adı, yeri ya da türü dışındaki
nedenlerle sorulanlar (toplu silme, geçmişin yeniden yazılması),
kendiliğinden kod çalıştıranlar (CI, editör ve ajan ayarları; her seferinde
sorulur) ve `.kule/` altındaki her şey.

## İlgili sayfalar

- [`kule policy`](../cli/policy.md), [`kule rules`](../cli/rules.md), [`kule commit`](../cli/commit.md)
- [Yerleşik politika kuralları](../reference/policy-rules.md)
- [Ağ ve sorular](network-and-questions.md)
- [Settings](../app/settings.md)

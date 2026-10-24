---
title: Dosyalar ve ayarlar
sidebar_label: Dosyalar ve ayarlar
description: Kule'nin sakladığı her dosya ve klasör; nerede durduğu, ne tuttuğu ve elle düzenleyip düzenleyemeyeceğiniz.
---

# Dosyalar ve ayarlar

Kule'nin sakladığı her şey, varsayılan olarak `~/.kule` olan **Kule'nin ev
dizininde** durur. Başka bir dizin kullanmak için `KULE_HOME` ayarlayın (ya
da `--home` verin); bkz. [Ortam değişkenleri](environment.md). Hiçbir
sandbox Kule'nin ev dizinini göremez: bir görevin komutları ve ajanları, kendi
kopyaları ve geçici dosyaları dışında bu sayfadaki hiçbir şeyi okuyamaz ya da
yazamaz.

Birkaç şey bunun dışında durur: Kule'nin ajanlar için kurduğu paketler,
ajanların kendi ev dizinleri ve bir deponun kendi kuralları. Bunlar en sonda
listelenir.

:::tip Elle düzenleme

**edit: yes** (düzenlenebilir) olarak işaretlenen dosyalar sizin yazmanız
için vardır: `policy.cedar`, `agents.toml` ve bir deponun
`.kule/policy.cedar` dosyası. Geri kalan her şey Kule'nin kendi durumudur.
Onları uygulama ya da `kule` komutları üzerinden değiştirin; daemon
çalışırken elle düzenlenen bir dosyanın üzerine yazılabilir, bozuk bir dosya
da düzeltilene ya da kaldırılana kadar Kule'nin çalışmayı reddetmesine yol
açabilir.

:::

## Kule'nin ev dizini {#kules-home}

| Yol | Ne tutar | Düzenleme |
| --- | --- | --- |
| `tasks/<id>/` | Her görev için bir klasör. Bkz. [Bir görevin klasörü](#a-tasks-folder). | hayır |
| `trash/` | Silinen, kaldırılmak üzere olan görevler. Kule onu arka planda hemen ve daemon her başladığında boşaltır. | hayır |
| `policy.cedar` | Her görev için Kule'ninkinin yerine geçen **kendi politikanız**. Kule ona asla yazmaz, üzerine de yazmaz. Önceki bir sürümün yazdığı (0.2.0-preview.1 ile preview.11 arası) balanced ya da careful metnini tutan bir dosya o seçim sayılır ve bugünün kurallarıyla değerlendirilir. Bkz. [Politika kuralları](policy-rules.md). | **evet** |
| `policy-choice` | Kule'nin careful politikasını seçtiyseniz (`kule policy careful`) `careful`; balanced altında yoktur. Metin değil seçimdir; bu yüzden her zaman bu sürümün kurallarını ifade eder. | hayır: `kule policy` ile seçin |
| `group-policies/<id>.cedar` | Bir grubun kendi politikası; denetlendikten sonra `kule group policy ID own --file PATH` tarafından saklanır. | hayır: komutla yeniden ayarlayın |
| `groups.json` | Gruplar: kimlik, ad, renk, politika seçimi, bütçeler ve sınırlar. | hayır |
| `order.json` | Görevlerin uygulamadaki ve `kule list`'teki sırası. | hayır |
| `limits.json` | Her görev için sınırlar (`kule limits`). | hayır |
| `budget.json` | Her görev için günlük bütçe (`kule daily-budget`). | hayır |
| `budget.lock` | Kule bir çalıştırmanın payını ayırırken ya da bir çalıştırmanın harcadığını kaydederken tutulur; böylece süreçleri bunu birer birer yapar. | hayır |
| `remembered.json` | Kule'ye bir daha sormamasını söyledikleriniz (`kule rules`). | hayır |
| `beside.json` | Bir klasörün yanındaki, görevlerinin okuyabildiği projeler (`kule reveal`). | hayır |
| `isolation.json` | Daha zayıf yalıtımı kabul edip etmediğiniz (`kule isolation`). | hayır |
| `secrets.json` | Saklanan gizli değerler: adlar, host'lar, klasörler ve **mühürlenmiş** (şifrelenmiş) değerler. Yalnızca siz okuyabilirsiniz. | hayır |
| `secrets.key` | Gizli değerleri mühürleyen anahtar; **yalnızca** sistem onu saklamadığında: `~/.kule` dışında bir Kule ev dizini ya da anahtar zinciri veya Secret Service olmayan bir bilgisayar. `~/.kule` için anahtar macOS anahtar zincirinde (`Kule secrets` öğesi) ya da Linux'ta Secret Service'tedir. | hayır |
| `kule.key` | Her görevin geçmişini imzalayan anahtar (`kule history`). | hayır |
| `agents.toml` | Ajan profilleriniz. Her biri ancak onaylandıktan sonra, ve her değişiklikten sonra yeniden onaylanınca çalışır. Biçim: `kule agents show claude-code`; alanlar: [Ajanlar](../using-agents.md). | **evet** |
| `agents-approved.json` | Hangi profili, tam olarak hangi hâliyle onayladığınız. | hayır |
| `registry.json` | ACP kayıt defterinin son kopyası (`kule agents registry`). | hayır: bir önbellek |
| `extensions/` | Eklentiler. Bkz. [aşağısı](#extensions). | hayır |
| `shortcuts.json` | Klavye kısayollarınız (Settings → Shortcuts). | hayır |
| `themes.json` | Kullanılan tema ve sizin yaptığınız temalar (Settings → Appearance). | hayır |
| `layout.json` | Pencere panellerinin nerede durduğu. *Reset the layout* Kule'nin kendi düzenine döner. | hayır |
| `editors.json` | Bir kopyayı en son açtığınız editör ve ilk kullanım notunu gördüğünüz editörler. | hayır |
| `kept/` | Linux: Kule'nin bir çalıştırmadan sonra bir ajanın ayarlarını geri koyarken kenara aldıkları (`kept/replaced/…`). Kule bunları asla silmez. | eski girdileri silebilirsiniz |
| `kuled.sock` | Daemon'ın soketi. Yalnızca sahibi bağlanabilir. | hayır |
| `kuled.lock` | Çalışan daemon tarafından tutulur; böylece her ev dizini için yalnızca bir tane çalışır. | hayır |
| `kuled.token` | Daemon'a giden her isteğin taşıdığı token; her başlangıçta yeniden üretilir. | hayır |
| `kuled.log` | Uygulamanın başlattığı bir daemon'ın çıktısı. | yalnızca okuma |

### Eklentiler {#extensions}

`extensions/` altında:

| Yol | Ne tutar |
| --- | --- |
| `installed.json` | Kurulu her eklenti: sürüm, özet (digest), açık ya da kapalı ve geliştirilmekte olan birinin klasörü. |
| `approved.json` | Her biri için onayladığınız özet, izinler ve host'lar. |
| `mcp-approved.json` | Ajanların kullanmasına izin verdiğiniz MCP sunucuları, özetleriyle. |
| `packages/<id>-<digest>.kulext` | Paketler; olduğu gibi saklanır, asla açılmaz. |
| `data/<id>/` | Eklentinin sakladıkları (`storage.json`) ve ayarları (`config.json`). |

Bkz. [Eklentiler](../extensions.md).

## Bir görevin klasörü {#a-tasks-folder}

Her görev `tasks/<id>/` içinde durur; `<id>`, `kule list`'in yazdığı 8
karakterlik kimliktir.

| Yol | Ne tutar |
| --- | --- |
| `task.json` | Görevin kimliği, çalışma klasörü (sizin klasörünüz), ne zaman oluşturulduğu ve kopyanın klasör adı. |
| `settings.json` | Görevin adı ve rengi, grubu, bütçesi, kendi sınırları, soruların ne kadar beklediği ve kopyanın oluşturulduğunda kapladığı yer (disk sınırları oradan itibaren sayılır). |
| `w/<folder name>/` | **Kopya**: ajanın çalıştığı yer. `kule path TASK` bunu yazar. Yanında, `kule reveal` ile açtığınız projelere giden bağlantılar vardır. |
| `base/` | Klasörün son sync'teki hâli; üç yönlü birleştirmeler (three-way merge) için. |
| `baseline.json` | Çalışma klasörü ile kopyanın en son, dosya dosya uyuştuğu yer: Kule her birinin neyi değiştirdiğini bulmak için ikisini de bununla karşılaştırır. |
| `tmp/` | Görevin `TMPDIR`'ı. İçinde: `caches/` (görevin kendi paket önbellekleri: npm, cargo, pip, uv, Go, NuGet, Yarn, pnpm, …), `kule-ca.pem` ve `kule-ca-bundle.pem` (proxy'nin bu görev için sertifika yetkilisi), `.kule-loopback/` (sunucuları loopback'te tutan şey). |
| `home/` | **Ev katmanı**: görevin komutlarının ev dizininiz olarak gördüğü yer. Bkz. [Ev katmanı](../concepts/home-layer.md). |
| `home.json` | Görevin neyi değiştirdiğini anlamak için ev katmanının neyden oluşturulduğu. |
| `home-kept/` | `kule home TASK apply`'ın gerçek ev dizininizde yerine geçtikleri; her uygulama için bir klasör, `kule home TASK undo` için. |
| `steps.jsonl`, `steps-mark.json`, `steps-store/` | Commit edilmemiş **adımlar** (her araç çağrısının değişiklikleri) ve bir adıma geri dönmek için gerekenler. |
| `checkpoints/` | Her commit için numaralı bir klasör (`000001/checkpoint.json`, …) ve `store.git`: her commit'in yerine geçtiklerini tutan Lonca deposu ve imzalı geçmiş (`refs/kule/log`). |
| `network.jsonl` | Görevin komutlarının proxy üzerinden kurduğu her bağlantı, reddedilenler dahil: ne zaman, host, port, karar, kurallar, `outcome` (`tunneled`, `refused`, `cut`, `failed`, `unresolved`, `private`, `unreachable`), sorulduysa yanıtınız, içine giden gizli değerler, ve `cut` ya da `failed` içinde nedeni. |
| `sandbox.jsonl` | Sandbox'ın reddettikleri (macOS bildirir; Linux bildirmez). |
| `usage.jsonl` | Her ajan çalıştırmasının, ajanın bildirdiği şekliyle kullandıkları: token'lar, bağlam, maliyet; ve kapattığı pay (`reservation`). `released: true` olan bir satır bir çalıştırma değil, `kule budget TASK --release` ile bıraktığınız bir paydır. |
| `reserved.jsonl` | Her çalıştırmanın, ajanı başlamadan önce bütçelerden ayırdığı. Bir pay, `usage.jsonl` dosyasındaki bir satır onu kapatana kadar harcanmış sayılır. |
| `secrets.json` | Kule'nin kopyadaki `.env` dosyalarında maskelediği değerler; saklanan gizli değerler gibi mühürlenmiş. |
| `synced.json` | Son `kule sync`'in getirdikleri; ajanın bir sonraki turunda duyması için. |
| `refused-hints.json` | "Bir araç neden başarısız oldu" notlarından hangilerinin ajana zaten söylendiği. |

Bunların hiçbiri elle düzenlemek için değildir. Bir görevi klasörünü
kaldırarak değil, her zaman `kule rm TASK` ile silin.

### `~/.kule` neden büyük görünüyor {#why-kule-looks-large}

macOS'ta (APFS) ve Linux'ta Btrfs ya da XFS ile kopya, `base/` ve
önbelleğe alınmış paketler, dosyalarınızla disk bloklarını paylaşan
**klonlardır** (clone). Finder ve `du` yine de onları tam boyutlarıyla
sayar; gerçekte ne kadar yer kullanıldığını görmek için boş alana bakın
(`df -h`).

## Kule'nin ev dizini dışında {#outside-kules-home}

| Yol | Ne tutar |
| --- | --- |
| `~/Library/Application Support/Kule/packages` (macOS), `~/.local/share/kule/packages` (Linux) | Kule'nin npm ile kurduğu ajanlar (Claude Code'un ve Codex'in ACP bağdaştırıcıları); her tam sürüm için bir klasör, kurulum betikleri çalıştırılmadan kurulur. Hiçbir sandbox buraya yazamaz. |
| `~/Library/Application Support/Kule/agents/<name>` (macOS), `~/.local/share/kule/agents/<name>` (Linux) | Kule'nin kendi ajanları gibi profili `own_home` ayarlayan bir ajan için **ona ait bir ev dizini**: Kule'deki ayarları, geçmişi ve konuşma kayıtları, görevler boyunca. Kule ayarlarınızı ve talimatlarınızı her başlangıçta buraya kopyalar (`carry`). |
| `<workspace>/.kule/policy.cedar` | Bir deponun kendi **forbid** kuralları. Politikayı yalnızca sıkılaştırabilir. Kule onu bir görevin kopyasından değil, her zaman gerçek klasörünüzden okur; `.kule/` altındaki herhangi bir şeye yapılan her değişiklik bir insanın onayını bekler. **Düzenleme: evet.** |

## Bilgisayarlar arasında taşıdığınız dosyalar {#files-you-move-between-computers}

| Dosya | Ne olduğu |
| --- | --- |
| `*.kule-profile.json` | Seçimleriniz tek bir dosyada (Settings → Profile): temalar, kısayollar, balanced ya da careful seçimi, günlük bütçe, sınırlar ve ajan profilleriniz. Asla gizli değerler, hatırlanan onaylar ya da sizin yazdığınız bir politika değil. İçe aktarırken Kule dosyada ne olduğunu gösterir, siz parçaları seçersiniz; dosyadaki ajanlar onayınızı bekler. |
| `*.kule-theme.json` | Tek bir tema: pencerenin adlandırılmış her parçası için bir `#rrggbb` rengi, başka hiçbir şey. İçeri alınırken denetlenir. |
| `*.kulext` | Bir eklenti paketi. Bkz. [Eklentiler](../extensions.md). |

## Ayrıca bakın {#see-also}

- [Ortam değişkenleri](environment.md)
- [Politika kuralları](policy-rules.md)
- [Görevler ve kopyalar](../concepts/tasks-and-copies.md)

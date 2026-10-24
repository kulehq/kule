---
title: Ağ ve sorular
sidebar_label: Ağ ve sorular
description: Bir görevin kurduğu her bağlantı görevin proxy'sinden nasıl geçer, hangi host'lar kendiliğinden geçer, sorular yanıtınızı nasıl bekler ve ağ nasıl kapatılır ya da açılır.
---

# Ağ ve sorular

## Her bağlantı görevin proxy'sinden geçer

Her görevin kendi **proxy**'si vardır. Görevdeki komutlar yalnızca ona
bağlanabilir: alışılmış proxy değişkenleri (`HTTPS_PROXY` ve benzerleri)
onu gösterir, sandbox da geri kalan her şeyi erişim dışında tutar.

Proxy her bağlantıyı dosyalarla aynı [politika](policy.md) ile
değerlendirir (bir host ve port taşıyan `net.connect` etki türü):

| Bağlantı | Karar (balanced ve careful) |
| --- | --- |
| HTTPS üzerinden paket kayıt depoları: npm, Yarn, crates.io, NuGet, PyPI, RubyGems, Maven Central | **pass** (kendiliğinden geçer) |
| Ajanın kendi model API'si, o ajan çalışırken | **pass** (kendiliğinden geçer) |
| Git sunucuları dahil diğer her şey | **ask** (onay bekler) |

Git sunucuları onay bekler, çünkü TLS içinde bir push bir fetch'ten ayırt
edilemez.

Her bağlantı görevin `network.jsonl` dosyasına yazılır ve bir komuttan sonra
ekrana basılır:

```text
net: registry.npmjs.org:443 · tunneled (package-registries)
net: example.com:443 · refused, would wait for approval; the command line asks no one (no-rule-lets-it-pass)
net: cdn.example:443 · cut by Kule: its TLS hello asks for other.example, and this tunnel is to cdn.example (agent-api)
```

İstendiği gibi geçmeyen bir bağlantı, satırında nedenini söyler: Kule
kesmiştir, adının götürdüğü adres yüzünden Kule reddetmiştir, kendi
kendine başarısız olmuştur ya da adı çözülmemiştir.

Her bağlantının sonucu (`outcome`) ona ne olduğunu söyler:

| Sonuç | Ne oldu |
| --- | --- |
| `tunneled` | Geçti. Kule'nin içine baktığı yerde, içindeki her istek de geçti. |
| `refused` | Politika onu geçirmedi, kimse onaylamadı ya da Kule onu adının götürdüğü adres yüzünden reddetti (hangisi olduğunu kaydın `refused` alanı söyler). |
| `cut` | Kule onu geçirdi, sonra kesti ve nedenini günlüğün `cut` alanında söyler: aşağıdaki *HTTPS ve gizli değerler* bölümüne bakın. |
| `failed` | Kendi hatasıyla bitti, Kule'nin yaptığı değil: host gitti, bağlantı sıfırlandı. Nedenini günlüğün `failed` alanı söyler. Bu, düz HTTP için de Kule'nin içine baktığı bağlantılar için de geçerlidir. |
| `unresolved`, `private`, `unreachable` | Adı çözümlenmedi, çıktığı her adres özel ya da bu bilgisayarın adresiydi, ya da host bağlantıyı kabul etmedi. |

Proxy bağlandığı adresi de denetler ve özel (private) adresleri erişim
dışında tutar. IPv6'da yalnızca global unicast aralığı genel (public)
sayılır; bir IPv6 adresinin içinde taşınan bir IPv4 adresi (mapped, NAT64,
6to4) kendisi olarak değerlendirilir. Bir ad küçük harfle ve sondaki nokta
olmadan değerlendirilir; yani `Example.com.`, `example.com`'dur.

## Sorular

Kararı **ask** olan bir bağlantı bir **soruya** dönüşür: sizi bekler.

**Uygulamada**, görevin Live panelinde bir kart çıkar ve görev **Needs
you** (sizi bekleyenler) altına taşınır. Kart host'u ve portu, bekletilmesine
yol açan kuralı ve bir geri sayımı gösterir. Pencere önde değilken bir
sistem bildirimi size haber verir.

Kart, onaylamanın neyi geçireceğini de söyler:

- **Tünellenir, içine bakılmaz.** Olağan durum budur. Kule host'u ve portu
  görür, bağlantının üzerinden geçeni görmez; TLS hello'sunun bu host'u
  istediğini denetler. Onay hello'dan önce gelir, bu yüzden kart yalnızca
  kuralı söyleyebilir: hello başka bir host isterse ya da adını gizlerse,
  Kule bağlantıyı onayınızdan sonra keser.
- **İçine bakılır.** Host, Kule'nin bir gizli değer doldurduğu bir host'tur
  (kart gizli değerlerin adını verir: "fills STRIPE_KEY into its requests")
  ya da Kule'nin yalnızca okumaya izin verdiği bir paket kayıt deposudur.
  Bir onay, Kule'nin içine baktığı host'lara asla yeni bir host eklemez.

Yanıtlandıktan sonra kart, sorulduğu yerde çalıştırmanın bir satırına
dönüşür: "You approved api.example:443". Kule ardından bağlantıyı keserse ya
da adı bu bilgisayardaki veya ağındaki bir adrese gittiği için reddederse,
aynı satır bunu nedeniyle birlikte, çalıştırma sürerken söyler ve bir
bildirim size haber verir: "You approved api.example:443; then cut by Kule:
…".

| Yanıt | Kısayol | Etkisi |
| --- | --- | --- |
| **Approve** | Mod+Shift+Y | Bu bağlantı kurulur. |
| **Approve and remember** | – | Bu host ve port bundan sonra bu klasörde geçer. |
| **Deny** | Mod+Shift+N | Reddedilir; ajana onsuz devam etmesi söylenir. |

**Yanıtlanmazsa**, görevin bekleme süresi dolunca bağlantı reddedilir:
varsayılan 15 dakikadır, 1 dakika ile 8 saat arasında ayarlanabilir
(istemin altındaki **waits** seçicisi). Kapatılamaz.

Kule bir soruyu sizin yanıtınız olmadan üç durumda reddeder ve bunu sizin
değil kendi reddi olarak söyler: kimse zamanında yanıtlamadı; çalıştırma,
kimse yanıtlamadan bitti; ya da çalıştırmanın 16 sorusu zaten bekliyordu,
bu yüzden sıradaki sorulmadan reddedildi (her host ve port için bir kez
söylenir). Her biri çalıştırmada bir satır ("Kule refused api.example:443:
nobody answered in time"), bir bildirim ve Timeline'da bir kayıt olarak
görünür.

**Komut satırında** soracak kimse yoktur, bu yüzden `kule exec` ve
`kule run` böyle bağlantıları hemen reddeder (`would wait for approval; the
command line asks no one`).

Hatırlanan host'lar [`kule rules`](../cli/rules.md) ile ve *Settings →
Rules* bölümünde listelenir; orada birini unutturabilirsiniz.

## Ağı kapatmak ya da açmak

Tek bir komut için:

```sh
kule exec TASK --no-network -- npm test     # hiç TCP yok, localhost dahil
kule exec TASK --open-network -- ./fetch.sh # her yere bağlan: proxy yok, politika yok
```

`--open-network` politikayı tamamen devre dışı bırakır. Bilerek kullanın.

## HTTPS ve gizli değerler

Proxy normalde HTTPS'i yalnızca tünelden geçirir: içeriği değil, host'u ve
portu görür. Bir tünel devam etmeden önce istemcinin TLS hello'sunu okur:
istediği ad, tünelin açıldığı host olmalıdır (bir adres için hiçbir ad
olmamalıdır); yoksa tünel, host'a tek bir bayt ulaşmadan kesilir, çünkü
birçok host tek bir adresi paylaşır ve bir hello hangi adı isterse onu
sunar. Adını gizleyen bir hello (Encrypted Client Hello) da kesilir: her
tarayıcıda ve araçta bunu neyin kapattığı
[Sorun giderme](../troubleshooting.md#a-connection-is-cut-for-encrypted-client-hello)
sayfasında. Port 443 TLS'ten
başka bir şey taşımaz. Düz `http://` istekleri tek tek okunur ve her biri,
geçirildiği host için olmalıdır. Adı bu bilgisayardaki ya da ağındaki bir
adrese çıkan bir bağlantı da kesilir. Kesilen bir bağlantı, nedeniyle
birlikte `cut` olarak günlüğe yazılır.

Bir [gizli değerin (secret)](secrets.md) ait olduğu host'lar
için ise, gizli değeri isteğe yerleştirmek üzere TLS'in içine bakar. Bunun
için her çalıştırmada yalnızca o host'lar için geçerli kendi sertifika
otoritesini üretir ve `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`,
`REQUESTS_CA_BUNDLE` ve benzerlerini ona yönlendirir. Kule onu asla
anahtar zincirinize (keychain) eklemez.

Paket kayıt depoları, aracı görevin otoritesine güvenen npm, yarn, pnpm ve
bun, cargo, pip, uv ve poetry ile gem için komutlara **salt okunurdur**
(publish reddedilir).

Kule'nin bir bağlantının içine bakıp bir isteği kendisi yanıtladığı yerde
bağlantı, `Kule refused a request in it: <why>` ile `cut` olarak günlüğe
yazılır. Neden bunlardan biridir; isteğin değil, Kule'nin sözleriyle:

- `a write to a host commands may only read from`
- `a credential Kule does not hold for this host`
- `a body Kule checks whole that was chunked or over 1 MiB`
- `a request Kule could not read one way only, or for another host`

Kule'nin reddettiği düz bir `http://` isteği de nedeniyle birlikte `cut`
olarak yazılır.

## Bilinen sınırlar

Bunlar [Platformlar](../platforms.md) sayfasında da listelenir:

- HTTP/2 konuşulmaz: istemciler HTTP/1.1'e geri döner. WebSocket
  yükseltmeleri henüz desteklenmiyor.
- macOS'ta sertifikaları yalnızca sistemin kendi hizmetiyle denetleyen
  araçlar bir görevde HTTPS kullanamaz. pip 24.2 ve sonrası
  `--use-deprecated=legacy-certs` ile çalışır. Go'ya `SSL_CERT_FILE`'ın
  gösterdiği kök sertifikaları kullanması söylenir.
- macOS'ta NuGet'nin ve Maven'ın araçları yalnızca sistemin kök
  sertifikalarına güvenir, bu yüzden kayıt depoları Go'nun modül
  proxy'si gibi görülmeden geçer.
- Linux'ta ağ kuralları Linux 6.7 veya sonrasını gerektirir.

## İlgili sayfalar

- [`kule exec`](../cli/exec.md), [`kule run`](../cli/run.md), [`kule rules`](../cli/rules.md)
- [Politika](policy.md), [Gizli değerler](secrets.md)
- [Live](../app/live.md), [Timeline](../app/timeline.md)

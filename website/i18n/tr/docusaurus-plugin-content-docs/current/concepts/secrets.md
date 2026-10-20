---
title: Gizli değerler
sidebar_label: Gizli değerler
description: Kule bir görevin komutlarının API anahtarlarını hiç görmeden kullanmasını nasıl sağlar - yer tutucular, proxy ve maskelenmiş .env dosyaları.
---

# Gizli değerler (secrets)

## Fikir

Bir ajanın sık sık bir API anahtarına ihtiyacı olur: Stripe'ı çağırmak, bir
kayıt deposuna push etmek, kendi modeline ulaşmak için. Anahtar ortam
değişkenlerindeyse ajan (ya da bir prompt injection) onu ekrana basabilir,
bir dosyaya yazabilir ya da herhangi bir yere gönderebilir.

Kule ile ajan **değeri asla görmez**. Bir **yer tutucu** (placeholder)
görür; görevin proxy'si de yer tutucuyu yalnızca sizin belirttiğiniz
host'lara giden isteklerde gerçek değerle değiştirir.

## Bir gizli değer saklamak

```sh
kule secrets add STRIPE_KEY --host api.stripe.com
```

- Değer terminalden **ekrana yansıtılmadan** ya da standart girdiden
  (`… < key.txt`) okunur. Kabuk geçmişinize düşeceği komut satırından asla
  okunmaz.
- `--host` nereye gidebileceğini belirtir (birden çok host için tekrarlayın).
- Varsayılan olarak bir gizli değer, komutu çalıştırdığınız klasör içindir;
  başka bir klasör için `--folder PATH`, her klasör için `--everywhere`
  kullanın.

```sh
kule secrets              # değerleri olmadan listele
kule secrets remove STRIPE_KEY
```

Uygulamada: *Settings → Secrets*.

## Bir komut ne görür

```sh
kule exec TASK -- sh -c 'echo $STRIPE_KEY'
# anahtarınızı değil, bir yer tutucuyu basar
```

`api.stripe.com`'a giderken proxy, değeri bir **kimlik bilgisi başlığında**
(credential header) yer tutucunun yerine koyar: `Authorization` (`Basic`
dahil), `x-api-key`, `x-goog-api-key` ve benzerleri. Diğer her yerde yer
tutucu olarak kalır:

- URL'de, diğer başlıklarda ve gövdede; böylece değer, onu geri
  yansıtabilecek bir yere asla ulaşmaz;
- başka bir host'a gönderildiğinde hiçbir şeyi açmayan bir metindir.

O host'a, Kule'nin onun için tutmadığı kimlik bilgileri taşıyan bir istek
reddedilir (403). Değeri geri yansıtan bir yanıtta değer, proxy'nin
okuyabildiği ölçüde `*` ile karartılır.

Bağlantı kaydı, giden gizli değerlerin adını verir:

```text
net: api.stripe.com:443 · tunneled · with STRIPE_KEY
```

Bunu yapmak için proxy yalnızca gizli değerin host'larında, başka hiçbir
yerde değil, TLS'in içine bakar. Her çalıştırmada, ad kısıtlamaları
(name constraints) gereği yalnızca o host'lar için geçerli, anahtarı
belleğinden hiç çıkmayan kendi sertifika otoritesini üretir ve
`SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`, `REQUESTS_CA_BUNDLE` ve benzerlerini
ona yönlendirir.

## Ajanların API anahtarları

Yerleşik ajanlar `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` ve `GEMINI_API_KEY`'i
aynı şekilde alır: ajanın değişkeni çalıştırmaya özgü bir yer tutucu tutar,
proxy de anahtarınızı ajanın kendi API host'u için doldurur. Yazdığınız bir
profil de aynısını `keys` altında yapabilir (bkz. [Ajanlar](../using-agents.md)).

## `.env` dosyaları da maskelenir

Kopyanın `.env` dosyalarındaki (`.env`, `.env.local`, `*.env`, `.envrc`;
`.env.example` değil) adıyla ya da biçimiyle gizli değer gibi görünen
değerler, yer tutucularla değiştirilir:

- görev açıldığında, her commit, rollback ve undo'dan sonra ve her
  çalıştırmadan önce;
- **gerçek değerleri klasör tutar**: bir commit onları ajanın değiştirdiği
  bir ayar dosyasına geri koyar ve gerçek değerlerle birleştirir;
- görev, maskelediği değerleri kendi mühürlü `secrets.json` dosyasında
  tutar;
- aynı zamanda saklı bir gizli değer olan maskelenmiş bir değer, o gizli
  değerin yer tutucusunu alır ve böylece onun host'larına ulaşır. Diğerleri
  yer tutucu olarak kalır.

Aynı maskeleme, [ev katmanının](home-layer.md) `.zshrc`, `.profile`,
`~/.env` ve benzerlerinin klonlarına da uygulanır.

## Gizli değerler nerede saklanır

Gizli değerler Kule'nin ev dizininde durur ve her sandbox'tan gizlenir.
Değerleri, sisteminizin anahtar zincirinin (macOS keychain) ya da Secret
Service'in (Linux) tuttuğu bir anahtarla **mühürlenir**. Bunlardan biri
olmayan bir bilgisayar ya da `~/.kule` dışında bir Kule ev dizini, anahtarı
yanındaki bir dosyada tutar; *Settings → Secrets* bunu söyler.

## Henüz yapmadıkları

- HTTP/2 (istemciler HTTP/1.1'e geri döner) ve WebSocket yükseltmeleri.
- Bir parça (chunk) sınırıyla bölünmüş ya da 1 MiB'tan büyük bir gövdedeki
  yer tutucu.
- macOS'ta sertifikaları yalnızca sistemin kendi hizmetleriyle denetleyen
  araçlar (`--use-deprecated=legacy-certs` olmadan pip 24.2 ve sonrası).
  Kule kendi otoritesini asla anahtar zincirine eklemez.

## İlgili sayfalar

- [`kule secrets`](../cli/secrets.md)
- [Ağ ve sorular](network-and-questions.md)
- [Settings](../app/settings.md)
- [Güvenlik modeli](../security.md)

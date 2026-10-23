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
reddedilir (403); böylece sizin olmayan bir hesap adına hiçbir şey dışarı
çıkmaz:

- bir kimlik bilgisi başlığı, bir çerez (cookie) ya da kimlik bilgisi gibi
  adlandırılmış bir başlık veya URL parametresi (`token`, `key`, `secret`,
  `password`, `auth`, `credential` ve benzerleri, hangi yazımla olursa olsun:
  `private_token`, `JOB-TOKEN`, `apiKey`) yalnızca Kule'nin yer tutucusunu
  taşır;
- `Basic` ile yer tutucu, herhangi bir kullanıcı adı altında paroladır ya da
  parolasız kullanıcı adıdır;
- alanları olan bir gövde (bir form, JSON, çok parçalı (multipart) bir form,
  metin) 1 MB'a kadar bütünüyle okunur ve aynısı onun alanları için de,
  JSON'un en üst düzeyinde ve nesnelerin içindeki nesnelerde geçerlidir;
  okunamayacak kadar büyük ya da parçalı (chunked) olan bir gövde
  reddedilir.

Reddedilen bir isteği Kule kendisi yanıtlar, asla iletmez; bağlantı da
Kule'nin sözleriyle nedeniyle birlikte `cut` olarak günlüğe yazılır:
`Kule refused a request in it: a credential Kule does not hold for this host`
ya da `… a body Kule checks whole that was chunked or over 1 MiB`. `net:`
satırı `cut` der (bkz. [Ağ ve sorular](network-and-questions.md)).

Proxy o host'tan yalnızca bütün yanıtlar ister (`Range` yok). Değeri geri
yansıtan bir yanıtta değer, proxy'nin okuyabildiği ölçüde `*` ile
karartılır: olduğu gibi, JSON'un ya da bir URL'nin yazdığı gibi kaçışlı
(escaped) hâliyle ya da base64'te, parçalar arasında bölünmüş olsa bile.

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

Kule'nin maskeleyemediği bir ayar dosyası, yani UTF-8 olmayan (bir
Latin-1 baytı, Windows'taki bir editörden UTF-16 bir dosya) ya da 1 MB'tan
büyük bir dosya, değerlerini olduğu gibi gösterirdi. Bu yüzden kopyada
böyle bir dosya varken görevde hiçbir ajan çalıştırması, `kule exec`,
`kule shell` ya da terminal başlamaz; Kule dosyayı ve nedenini söyler.
Dosyayı 1 MB'ın altında, UTF-8 olarak kaydedin ya da klasörden çıkarın,
sonra yeniden deneyin.

Aynı maskeleme, [ev katmanının](home-layer.md) `.zshrc`, `.profile`,
`~/.env` ve benzerlerinin klonlarına da uygulanır. Orada Kule'nin
maskeleyemediği bir dosya ise saklanır: komutlar onu ne katmanda ne de
gerçek yolunda görür; Kule de bunu komutların çalıştığı yerde söyler
(uygulamada çalıştırmanın notları, `kule run`, `exec` ve `shell` çıktısı ve
terminal).

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

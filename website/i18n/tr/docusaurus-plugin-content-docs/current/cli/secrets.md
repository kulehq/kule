---
title: kule secrets
sidebar_label: kule secrets
description: Bir görevin komutlarının hiç görmeden kullandığı API anahtarlarını saklayın.
---

# kule secrets

**Gizli değerleri** (secret) saklayın: bir görevin komutlarının hiç görmeden
kullanabildiği API anahtarları ve token'lar.

## Kullanım {#synopsis}

```sh
kule secrets [OPTIONS] [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-secrets-list) | Saklanan gizli değerleri, değerleri olmadan listeler (varsayılan). |
| [`add`](#kule-secrets-add) | Bir ya da daha çok host için bir gizli değer saklar. |
| [`remove`](#kule-secrets-remove) | Bir gizli değeri unutur. |

## Açıklama {#description}

Saklanan bir gizli değerin bir **adı** (`STRIPE_KEY` gibi bir ortam
değişkeni) ve gidebileceği **host'ları** (`api.stripe.com` gibi) vardır.
Bir görevde:

1. Komutlar değişkende değeri değil, bir **yer tutucu** bulur; örneğin
   `kule_secret_4f7567ba…`.
2. Görevin [proxy'si](../concepts/network-and-questions.md) yalnızca bu
   host'lara giden HTTPS isteklerinin içine bakar ve bir kimlik bilgisi
   başlığındaki (`Authorization`, `x-api-key`, `x-goog-api-key` ve benzerleri)
   yer tutucunun yerine gerçek değeri koyar.
3. Başka her yerde (URL, diğer başlıklar, gövde, başka herhangi bir host)
   yer tutucu yer tutucu olarak kalır ve hiçbir kapıyı açmaz.

Bağlantı kaydı, dışarı çıkan gizli değerlerin adını yazar:
`net: api.stripe.com:443 · tunneled · with STRIPE_KEY`.

Değerin durduğu yer: Kule'nin ev dizininde (`secrets.json`), her sandbox'tan
gizli, sisteminizin sakladığı bir anahtarla (macOS anahtar zinciri ya da
Linux'ta Secret Service) mühürlenmiş olarak. Anahtar halkası olmayan bir
bilgisayarda ya da `~/.kule` dışında bir Kule ev dizininde, anahtar onun
yanında bir dosyada (`secrets.key`, yalnızca sizin okuyabileceğiniz)
saklanır ve *Settings → Secrets* bunu belirtir.

Kopyadaki `.env` dosyalarında gizli değere benzeyen değerler, sizin
eklemenize gerek kalmadan aynı şekilde maskelenir. Tüm resim ve henüz
kapsanmayanlar için bkz. [Gizli değerler](../concepts/secrets.md).

Bir gizli değer **tek bir klasör** (varsayılan: içinde bulunduğunuz dizin)
ya da **her klasör** (`--everywhere`) için geçerlidir.

Tüm alt komutlar `--home` alır (bkz. [genel seçenekler](index.md)).

## kule secrets list

```sh
kule secrets list [OPTIONS]
```

```console
$ kule secrets
STRIPE_KEY  → api.stripe.com  · ~/code/app
GH_TOKEN  → api.github.com, uploads.github.com  · every folder
```

Hiç yoksa: `no secrets kept`.

## kule secrets add

```sh
kule secrets add [OPTIONS] --host <HOSTS> <NAME>
```

| Argüman / seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `NAME` | değişken adı | | Değerin konacağı ortam değişkeni, `STRIPE_KEY` gibi: büyük harfler, rakamlar ve `_`. |
| `--host` | host adı | | Gidebileceği bir host. Daha fazlası için tekrarlayın. En az bir tane. |
| `--folder` | yol | içinde bulunduğunuz dizin | Değerin ait olduğu klasör. |
| `--everywhere` | | | Her klasör. |

Değer **hiçbir zaman** bir argüman olarak verilmez; böylece kabuk
geçmişinize ve süreç listesine girmez. Bir terminalde Kule değeri ekrana
yansıtmadan sorar; aksi hâlde standart girdinin ilk satırını okur.

```console
$ cd ~/code/app
$ kule secrets add STRIPE_KEY --host api.stripe.com
value of STRIPE_KEY (not shown):
kept STRIPE_KEY: commands see a placeholder; the value goes only to api.stripe.com
```

Bir parola yöneticisinden, bir boru (pipe) üzerinden:

```console
$ op read "op://dev/github/token" | kule secrets add GH_TOKEN \
    --host api.github.com --host uploads.github.com --everywhere
kept GH_TOKEN: commands see a placeholder; the value goes only to api.github.com, uploads.github.com
```

Görevdeki bir komutun gördüğü:

```console
$ kule exec 1046fb30 -- sh -c 'echo $STRIPE_KEY'
kule_secret_4f7567ba95b5beb31fccc18345b34260
```

## kule secrets remove

```sh
kule secrets remove [OPTIONS] <NAME>
```

| Seçenek | Anlamı |
| --- | --- |
| `--folder FOLDER` | Değerin saklandığı klasör (varsayılan: içinde bulunduğunuz dizin). |
| `--everywhere` | Her klasör için saklanan değer. |

```console
$ kule secrets remove STRIPE_KEY
forgot STRIPE_KEY
$ kule secrets remove GH_TOKEN --everywhere
forgot GH_TOKEN
```

Yanlış yerden kaldırmaya çalışırsanız `no secret STRIPE_KEY kept there`
yazar.

## Hatalar ve çözümleri {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: lower: a secret's name is an environment variable's, like STRIPE_KEY` | Büyük harfli bir değişken adı kullanın. |
| `kule: *.stripe.com: a lowercase host name, without a wildcard or a port` | Her host'u tam adıyla, kendi `--host`'uyla verin. |
| `kule: no value given` | Değeri yazın ya da boruyla verin; boş bir satır değer sayılmaz. |
| `error: the following required arguments were not provided` … `--host <HOSTS>` | En az bir `--host` ekleyin. |

## Uygulamada {#in-the-app}

*Settings → Secrets* gizli değerleri ekler, listeler ve kaldırır; onları
mühürleyen anahtarın nerede saklandığını da gösterir.

## Ayrıca bakın {#see-also}

- [Gizli değerler](../concepts/secrets.md)
- [Ağ ve sorular](../concepts/network-and-questions.md)
- [Dosyalar ve ayarlar](../reference/files.md)

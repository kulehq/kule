---
title: kule ext
sidebar_label: kule ext
description: Kule eklentilerini ve sundukları MCP sunucularını oluşturun, paketleyin, kurun, onaylayın ve yönetin.
---

# kule ext

**Eklentileri** (extension) ve ajanlara sundukları MCP sunucularını
oluşturun, paketleyin, kurun, onaylayın ve yönetin.

## Kullanım {#synopsis}

```sh
kule ext [OPTIONS] [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-ext-list) | Eklentileri ve çalışamayanların neden çalışamadığını listeler (varsayılan). |
| [`new`](#kule-ext-new) | Bir eklentiye başlar: olduğu gibi kurulabilen bir klasör. |
| [`info`](#kule-ext-info) | Bir `.kulext` dosyasının ya da bir klasörün neler istediğini, kurmadan gösterir. |
| [`pack`](#kule-ext-pack) | Bir klasörü `.kulext` dosyası olarak paketler. |
| [`install`](#kule-ext-install) | Bir `.kulext` kurar ya da günceller. |
| [`dev`](#kule-ext-dev) | Üzerinde çalıştığınız bir klasörü, canlı okunacak şekilde kurar. |
| [`approve`](#kule-ext-approve) | Bir eklentinin, şu anki hâliyle çalışmasına izin verir. |
| [`enable`](#kule-ext-enable) / [`disable`](#kule-ext-disable) | Onaylanmış bir eklentiyi açar ya da kapatır. |
| [`remove`](#kule-ext-remove) | Bir eklentiyi ve sakladıklarını kaldırır. |
| [`mcp`](#kule-ext-mcp) | Eklentilerin ajanlara sunduğu MCP sunucuları. |

## Açıklama {#description}

Bir **eklenti**, Kule'ye komutlar, paneller, temalar, ayarlar, bir
incelemeye notlar, ajan profilleri ya da MCP sunucuları ekler. Bir klasördeki
JavaScript'tir ve bir `.kulext` dosyası olarak paylaşılır. Kule ona bir
ajandan fazla güvenmez:

- kapalı bir listeden **izinler** ister ve ancak siz **tam olarak gördüğünüz
  sürümü** (özetiyle, digest) onayladıktan sonra çalışır;
- üzerindeki her değişiklik yeniden onay bekler;
- kodu, Kule'den başka hiçbir yere ulaşamayan sandbox'lı çerçevelerde
  (frame) çalışır.

Hiçbir izin gizli değerlere, sorulara verilen yanıtlara, commit'e,
rollback'e, undo'ya ya da politikalara ulaşmaz. Getirdiği ajanlar ve MCP
sunucularının her biri kendi onayını bekler.
[Eklentiler](../extensions.md) sayfası manifest'i, izinleri, SDK'yı ve paket
biçimini anlatır.

Uygulama açıkken `kule ext` değişikliklerini daemon üzerinden yapar;
böylece uygulama onları hemen görür. Aksi hâlde doğrudan Kule'nin ev
dizinini değiştirir.

Tüm alt komutlar `--home` alır (bkz. [genel seçenekler](index.md)).

## kule ext list

```sh
kule ext list [OPTIONS]
```

```console
$ kule ext
me.hello                         0.1.0      Hello — runs
```

Durum `runs` (çalışıyor), `turned off` (kapalı) ya da gerekçesiyle birlikte
`held: …` (bekletiliyor) olur; örneğin `not approved yet` ya da
`it changed since you approved it`. `(dev)` bir klasörden kurulmuş olanı
işaretler. Hiç yoksa:
``no extensions; `kule ext install FILE.kulext` adds one``.

## kule ext new

```sh
kule ext new [OPTIONS] <ID> [DIR]
```

| Argüman | Anlamı |
| --- | --- |
| `ID` | `publisher.name`; her parça `a-z`, `0-9` ve `-` karakterlerinden 1 ile 20 arası. |
| `DIR` | Nereye yazılacağı. Varsayılan: kimliğin ad kısmı (`me.hello` için `hello`). |

Çalışan bir eklenti yazar: `kule-extension.json` (manifest), `main.js`, bir
panel (`panel.html`, `panel.js`, `panel.css`), `kule-ext.d.ts` (SDK'nın
tipleri) ve bir `README.md`.

```console
$ kule ext new me.hello
wrote me.hello into hello
try it: kule ext dev hello, then kule ext approve me.hello
```

## kule ext info

```sh
kule ext info [OPTIONS] <PATH>
```

Bir `.kulext` dosyasını ya da bir eklentinin klasörünü Kule'nin okuyacağı
gibi okur ve neler istediğini yazar. Hiçbir şey kurulmaz.

```console
$ kule ext info me.hello-0.1.0.kulext
me.hello 0.1.0 — Hello
publisher:   me (not verified by anyone)
about:       What it does, in a sentence.
digest:      96e2ff19826845575cd1ea734e7b657615c53eb06107c3f12dd8230086f11af5
may:         tasks.read — See your tasks: their names, folders and state
command:     Hello: say hello (ext.me.hello.hello)
panel:       Hello
```

| Satır | Anlamı |
| --- | --- |
| `publisher` | Manifest'teki ad. Kimse bunu doğrulamaz: herkes istediği adı yazabilir. |
| `digest` | Paketin SHA-256 özeti. Onay buna bağlıdır. Bir klasör `dev-` ile başlayan bir özet gösterir. |
| `may` | İstediği her izin, sözcüklerle. `(new)`, siz onayladığınızda olmayan bir izni işaretler. |
| `reaches` | Bağlanabileceği bir host. |
| `warning: it can read your work and send data to the hosts above` | Hem bir ağ izni hem de çalışmanızı okuyan bir izni var. |
| `command`, `panel`, `theme` | Pencereye ekledikleri. |

## kule ext pack

```sh
kule ext pack [OPTIONS] <DIR>
```

| Seçenek | Anlamı |
| --- | --- |
| `-o`, `--out OUT` | Nereye yazılacağı. Varsayılan: içinde bulunduğunuz dizinde `<id>-<version>.kulext`. |

```console
$ kule ext pack hello
packed me.hello 0.1.0 into me.hello-0.1.0.kulext
digest 96e2ff19826845575cd1ea734e7b657615c53eb06107c3f12dd8230086f11af5
```

Aynı klasör her zaman aynı baytlara paketlenir; bu yüzden özet sabittir.
Özeti dosyanın yanında yayımlayın ki insanlar kurmadan önce
`kule ext info` ile kontrol edebilsin.

## kule ext install

```sh
kule ext install [OPTIONS] <FILE>
```

Bir `.kulext` kurar ya da aynı kimlikle kurulu olanı günceller. Ancak
onaylandıktan sonra çalışır.

```console
$ kule ext install me.hello-0.1.0.kulext
me.hello 0.1.0 — Hello
publisher:   me (not verified by anyone)
...
held:        not approved yet

installed; `kule ext approve me.hello` lets it run
```

Bir güncelleme, siz yeni sürümü onaylayana kadar eklentiyi yeniden
bekletir:

```console
$ kule ext install me.hello-0.2.0.kulext
me.hello 0.2.0 — Hello
...
may:         review.read — Read a task's changes and their diffs  (new)
held:        it changed since you approved it
```

## kule ext dev

```sh
kule ext dev [OPTIONS] <DIR>
```

Üzerinde çalıştığınız bir klasörü **canlı okunacak** şekilde kurar: kod
değişiklikleri olduğu gibi çalışır, değişen bir manifest ise yeniden onay
bekler. Buradan onaylanmamış kod çalıştığı için klasör hiçbir ajanın
yazamayacağı bir yerde olmalıdır: `/tmp`, `/var/tmp`, `/dev/shm`, Kule'nin
ev dizini (görevlerin kopyaları) ya da bir ajanın kendi klasörlerinin
altında olmamalıdır.

```console
$ kule ext dev hello
...
installed from hello; `kule ext approve me.hello` lets it run
```

## kule ext approve

```sh
kule ext approve [OPTIONS] <ID>
```

| Seçenek | Anlamı |
| --- | --- |
| `--yes` | Gösterileni sormadan onaylar. Terminal olmadığında (bir betikte) gereklidir. |

Eklentinin istediklerini gösterir, sonra `Let me.hello run as shown?
[y/N]` diye sorar. Onaylanan, gösterilendir: arada değiştiyse onay
reddedilir.

```console
$ kule ext approve me.hello --yes
me.hello 0.1.0 — Hello
...
approved me.hello as it stands; a change to it needs approval again
```

## kule ext enable

```sh
kule ext enable [OPTIONS] <ID>
```

```console
$ kule ext enable me.hello
me.hello runs
```

Hâlâ bekletiliyorsa (örneğin onaylanmamışsa) `me.hello is on, but
held: …` yazar.

## kule ext disable

```sh
kule ext disable [OPTIONS] <ID>
```

Eklentiyi kaldırmadan durdurur.

```console
$ kule ext disable me.hello
me.hello is turned off
```

## kule ext remove

```sh
kule ext remove [OPTIONS] <ID>
```

Eklentiyi, paketini, ayarlarını ve sakladıklarını kaldırır.

```console
$ kule ext remove me.hello
removed me.hello and what it kept
```

## kule ext mcp

```sh
kule ext mcp [OPTIONS] [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| `list` | Çalışan eklentilerin sunduğu MCP sunucularını listeler (varsayılan). |
| `approve SERVER [--yes]` | Ajanların `EXTENSION/NAME` sunucusunu, şu anki hâliyle kullanmasına izin verir. Komutu gösterir, sonra sorar. |
| `revoke SERVER` | `EXTENSION/NAME` sunucusunu ajanlara sunmayı bırakır. |

Bir eklenti ajanlara bir **MCP sunucusu** sunabilir. Her biri kendi onayını
bekler. Bir ajan onaylanmış bir sunucuyu **kendi sandbox'ının içinde**
başlatır; böylece sunucu, ajanın oturum bilgisi dahil ajanın okuyabildiği
her şeyi okur.

```console
$ kule ext mcp
An agent starts each inside its sandbox: it reads what the agent can, its sign-in included.

acme.db/db — waits for you
  starts: npx -y @acme/db-mcp@1.2.3
$ kule ext mcp approve acme.db/db --yes
acme.db/db starts: npx -y @acme/db-mcp@1.2.3
An agent starts it inside its sandbox: it reads what the agent can, its sign-in included.
agents get acme.db/db from their next run: npx -y @acme/db-mcp@1.2.3
$ kule ext mcp revoke acme.db/db
agents no longer get acme.db/db
```

## Hatalar ve çözümleri {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: "Bad_Id" must be publisher.name` | Küçük harfli `publisher.name` kullanın. |
| `kule: no extension me.hello` | Kimliği `kule ext` ile kontrol edin. |
| `kule: no terminal to ask on; add --yes to approve what is shown above` | Bir betikte önce çıktıyı okuyun, sonra `--yes` verin. |
| `… is under /private/tmp, where agents can write; an extension under development must sit where only you can change it` | Klasörü `~/src/hello` gibi size ait bir yere taşıyın ya da onun yerine paketleyip kurun. |
| `kule: name the server as EXTENSION/NAME` | Sunucuyu `acme.db/db` biçiminde yazın. |
| `kule: no extension that runs offers acme.db/db` | Eklenti kurulu değil, onaylanmamış ya da kapalı. |

## Uygulamada {#in-the-app}

*Settings → Extensions* eklentileri kurar, onaylar, açar ya da kapatır ve
kaldırır; sundukları MCP sunucularını da onaylar. Uygulama tüm eklentiler
kapalı olarak da başlatılabilir: `--no-extensions` ya da
`KULE_NO_EXTENSIONS=1`.

## Ayrıca bakın {#see-also}

- [Eklentiler](../extensions.md): manifest, izinler, SDK, paket biçimi.
- [`kule agents`](kule-agents.md): bir eklentinin getirdiği ajan profilleri orada bekler.
- [Dosyalar ve ayarlar](../reference/files.md): `extensions/`.

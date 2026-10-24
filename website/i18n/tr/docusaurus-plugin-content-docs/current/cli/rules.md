---
title: kule rules
sidebar_label: kule rules
description: Kule'ye bir daha sormamasını söylediklerinizi listeleyin, ekleyin ve unutturun.
---

# kule rules

**Hatırlanan kuralları**, yani Kule'ye bir daha sormamasını
söylediklerinizi listeler, ekler ve unutturur.

## Kullanım {#synopsis}

```sh
kule rules [OPTIONS] [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-rules-list) | Kuralları listeler (varsayılan). |
| [`allow`](#kule-rules-allow) | Bir bağlantıyı ya da tek bir dosyadaki değişiklikleri sormadan geçirir. |
| [`forget`](#kule-rules-forget) | Bir kuralın geçirdiği şey için yeniden sorulmasını sağlar. |

## Açıklama {#description}

[Politika](../concepts/policy.md) bir şeyi size sorduğunda, bir kez ve
kalıcı olarak yanıtlayabilirsiniz. Hatırlanan bir kural iki türden biridir:

- **Bir bağlantı**: `HOST:PORT`, örneğin `api.example.com:443`. Bir Cedar
  `permit` kuralına dönüşür; böylece oraya yapılan bağlantılar kendiliğinden
  geçer.
- **Bir dosya**: klasöre göre bir yol, örneğin `package.json`, ve o dosya
  hakkında soran kural. O tek dosya için o kurala dar bir istisna olur.

Her biri **tek bir klasör** (varsayılan: geçerli dizin) ya da **tüm
klasörler** (`--everywhere`) için geçerlidir.

Bazı şeyler asla hatırlanmaz:

- politikanın **reddettikleri** (örneğin yeni bir git hook'u);
- bir dosyanın adından başka bir nedenle bekleyenler: toplu silme, geçmişi
  yeniden yazma, bir bağlantı (link), git config, çalıştırılabilir yapılan
  bir dosya, değişen genişletilmiş öznitelikler. Yalnızca
  `manifests-and-lockfiles`, `project-files` ve `automation` kuralları kalıcı
  olarak yanıtlanabilir;
- bir deponun kendi kurallarını tuttuğu `.kule/` altındaki her şey;
- deponun kendi bir kuralı (`repo:<id>`), `@id`'si ne olursa olsun:
  hatırlanan bir yanıt, deponun sorduğunu asla iptal etmez.

careful politikada her dosya değişikliği ayrıca hatırlanamayan `careful`
kuralını da bekler; bu yüzden orada hatırlanan bir dosya geçmez.

Hatırlanan kurallar Kule'nin evinde (`remembered.json`) durur; bunu hiçbir
sandbox göremez. Politikası ne olursa olsun her grupta geçerlidirler.

Tüm alt komutlar `--home` alır (bkz. [genel seçenekler](index.md)).

## kule rules list {#kule-rules-list}

```sh
kule rules list [OPTIONS]
```

```console
$ kule rules
  1  connections to example.com:443  · ~/code/app
  3  connections to api.github.com:443  · every folder
```

Hiçbir şey saklanmamışsa: `nothing remembered: every question is asked`.

## kule rules allow {#kule-rules-allow}

```sh
kule rules allow [OPTIONS] <WHAT>
```

| Argüman / seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `WHAT` | `HOST:PORT` ya da `PATH` | | Bir bağlantı (küçük harfli bir host adı ve bir port, joker karakter yok) ya da klasöre göre bir dosya yolu (`..` yok). |
| `--rule` | kural kimliği | `manifests-and-lockfiles` | Bir dosya için: o dosya hakkında soran kural. `manifests-and-lockfiles`, `project-files`, `automation`'dan biri. |
| `--folder` | yol | geçerli dizin | Kuralın geçerli olduğu klasör. |
| `--everywhere` | | | Tüm klasörler için geçerli olur. |
| `--yes` | | | Kuralı saklar. `--yes` olmadan hiçbir şey saklanmaz. |

`allow` önce kuralın geçmiş görevlerinizde **şimdiye kadar neleri
geçireceğini** gösterir: bekleyen ya da reddedilen bağlantılar ve dosyaya
dokunan commit'ler ile commit edilmemiş değişiklikler. Kuralı yalnızca
`--yes` saklar. Böylece karar vermeden önce bakabilirsiniz:

```console
$ cd ~/code/app
$ kule rules allow example.com:443
connections to example.com:443 · ~/code/app
would have let through so far: 1 connections, 0 changes, in 1 tasks
not kept: rerun with --yes to keep it
$ kule rules allow example.com:443 --yes
connections to example.com:443 · ~/code/app
would have let through so far: 1 connections, 0 changes, in 1 tasks
kept as 1
```

Bu klasörde `package.json` değişikliklerini geçirin:

```console
$ kule rules allow package.json --yes
changes to package.json (manifests-and-lockfiles) · ~/code/app
would have let through so far: 0 connections, 0 changes, in 0 tasks
kept as 2
```

`automation` kuralının sorduğu bir dosya, örneğin bir CI workflow'u:

```console
$ kule rules allow .github/workflows/ci.yml --rule automation --yes
```

Tüm klasörler için bir host:

```console
$ kule rules allow api.github.com:443 --everywhere --yes
connections to api.github.com:443 · every folder
would have let through so far: 0 connections, 0 changes, in 0 tasks
kept as 3
```

:::warning

Bir bağlantı kuralı, o görevlerdeki yalnızca ajanın değil, *her* komutun o
host'a ulaşmasına izin verir. Git host'ları sık görülen bir örnektir: TLS
içinde bir push, bir fetch ile aynı görünür.

:::

## kule rules forget {#kule-rules-forget}

```sh
kule rules forget [OPTIONS] <ID>
```

```console
$ kule rules forget 2
forgot 2: asked again from now on
$ kule rules forget 9
nothing remembered as 9
```

## Hatalar ve çözümler {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `HOST:PORT: a lowercase host name and a port, without a wildcard` | `api.example.com:443` gibi tek ve tam bir host yazın. |
| `PATH: a path relative to the folder, without ..` | Yolu klasörün içindeki haliyle verin. |
| `PATH: the repository's own rules are always put to a person` | `.kule/` altındaki dosyalar hatırlanamaz. |
| `RULE: only a rule that asks about a file by its name can be answered for good` | `--rule`, `manifests-and-lockfiles`, `project-files` ya da `automation` alır. |

## Uygulamada {#in-the-app}

Bekleyen bir bağlantıda **Approve and remember** (onayla ve hatırla), görevin
klasörü için bir bağlantı kuralı saklar. Review'da bekletilen bir dosyadaki
**Don't ask again in this folder** (bu klasörde bir daha sorma) önce neleri
geçireceğini gösterir, sonra bir dosya kuralı saklar. *Settings → Rules*
hepsini, her birinin yanında **Forget** ile listeler. Komut satırından
`kule commit --approve --remember`, onaylanan dosyalar için dosya kuralları
saklar.

## Ayrıca bakın {#see-also}

- [`kule policy`](policy.md), [Politika kuralları](../reference/policy-rules.md)
- [`kule commit`](commit.md): `--approve --remember`.
- [Ağ ve sorular](../concepts/network-and-questions.md)

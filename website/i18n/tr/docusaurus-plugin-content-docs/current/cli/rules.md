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
| [`allow`](#kule-rules-allow) | Bir bağlantıyı, tek bir dosyadaki değişiklikleri ya da bir türdeki dosyaları sormadan geçirir. |
| [`forget`](#kule-rules-forget) | Bir kuralın geçirdiği şey için yeniden sorulmasını sağlar. |

## Açıklama {#description}

[Politika](../concepts/policy.md) bir şeyi size sorduğunda, bir kez ve
kalıcı olarak yanıtlayabilirsiniz. Hatırlanan bir kural üç türden biridir:

- **Bir bağlantı**: `HOST:PORT`, örneğin `api.example.com:443`. Bir Cedar
  `permit` kuralına dönüşür; böylece oraya yapılan bağlantılar kendiliğinden
  geçer.
- **Bir dosya**: klasöre göre bir yol, örneğin `package.json`, ve o dosya
  hakkında soran kural. O tek dosya için o kurala dar bir istisna olur.
  Gizli bir dosya (`dot-paths`) da böyle, yoluyla yanıtlanır.
- **Bir dosya türü**: noktası olmadan küçük harfle bir uzantı (`vue`) ya da
  uzantısız bir ad (`makefile`); `unfamiliar-kinds` kuralının sorduğu türden.
  Bundan sonra o tür projede bulunan bir tür sayılır: o türdeki dosyalar
  artık türleri yüzünden beklemez.

Her biri **tek bir klasör** (varsayılan: geçerli dizin) ya da **tüm
klasörler** (`--everywhere`) için geçerlidir.

Bazı şeyler asla hatırlanmaz:

- politikanın **reddettikleri** (örneğin yeni bir git hook'u);
- bir dosyanın adı, yeri ya da türü dışında bir nedenle bekleyenler: toplu
  silme, geçmişi yeniden yazma, bir bağlantı (link), git config,
  çalıştırılabilir yapılan bir dosya, değişen genişletilmiş öznitelikler.
  Yalnızca `manifests-and-lockfiles`, `project-files`, `unfamiliar-kinds` ve
  `dot-paths` kuralları kalıcı olarak yanıtlanabilir;
- kendiliğinden kod çalıştıran şeyler (`automation`: CI, editör ve ajan
  ayarları) ve başka bir sürüm kontrol sisteminin dosyaları (`other-vcs`):
  bunlar her seferinde sorulur. Bunlardan biri ve ayrıca başka bir kural
  tarafından tutulan bir dosya da hatırlanmaz;
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

Önceki bir sürümün `automation` kuralı için sakladığı bir yanıt hâlâ
listelenir, ama artık hiçbir şeyi geçirmez. Onu
[`kule rules forget`](#kule-rules-forget) ile unutturun:

```console
$ kule rules
  2  changes to .github/workflows/ci.yml (automation)  · ~/code/app · no longer applies: what runs code on its own is always asked
  4  files of kind vue (unfamiliar-kinds)  · ~/code/app
```

## kule rules allow {#kule-rules-allow}

```sh
kule rules allow [OPTIONS] <WHAT>
kule rules allow [OPTIONS] --kind <KIND>
```

| Argüman / seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `WHAT` | `HOST:PORT` ya da `PATH` | | Bir bağlantı (küçük harfli bir host adı ve bir port, joker karakter yok) ya da klasöre göre bir dosya yolu (`..` yok). |
| `--rule` | kural kimliği | `manifests-and-lockfiles` | Bir dosya için: o dosya hakkında soran kural. `manifests-and-lockfiles`, `project-files`, `unfamiliar-kinds`, `dot-paths`'ten biri. |
| `--kind` | tür | | `WHAT` yerine, bu türdeki dosyalar: noktası olmadan bir uzantı (`vue`) ya da uzantısız bir ad (`makefile`), küçük harfle. |
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

Projede hiç olmayan bir türdeki dosyalar, bundan sonra:

```console
$ kule rules allow --kind vue --yes
files of kind vue (unfamiliar-kinds) · ~/code/app
would have let through so far: 0 connections, 3 changes, in 1 tasks
kept as 4
```

Gizli bir dosya, yoluyla:

```console
$ kule rules allow .prettierrc.json --rule dot-paths --yes
```

`automation` kuralının sorduğu bir dosya, örneğin bir CI workflow'u,
hatırlanamaz: kendiliğinden kod çalıştıran şey her seferinde sorulur.

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
| `RULE: only a rule that asks about a file by its name, place or kind can be answered for good; what runs code on its own is always asked` | `--rule`, `manifests-and-lockfiles`, `project-files`, `unfamiliar-kinds` ya da `dot-paths` alır. |
| `KIND: a kind of file in lower case, as an extension without its dot (vue) or a name without one (makefile)` | Türü Kule'nin adlandırdığı gibi verin, örneğin `vue` ya da `makefile`. Gizli bir dosya yoluyla hatırlanır. |

## Uygulamada {#in-the-app}

Bekleyen bir bağlantıda **Approve and remember** (onayla ve hatırla), görevin
klasörü için bir bağlantı kuralı saklar. Review'da bekletilen bir dosyadaki
**Don't ask again in this folder** (bu klasörde bir daha sorma) önce neleri
geçireceğini gösterir, sonra bir dosya kuralı saklar. Yalnızca türü
yüzünden bekleyen dosyalar tek bir grup olur; grubun **Don't ask again
about files of kind X in this folder** (bu klasörde X türündeki dosyaları
bir daha sorma) seçeneği türü saklar. *Settings → Rules* hepsini, her
birinin yanında **Forget** ile listeler; önceki bir sürümün `automation`
için sakladığı bir yanıtta **No longer applies** (artık geçerli değil)
yazar. Komut satırından `kule commit --approve --remember`, onaylanan
değişikliklerden hatırlanabilenleri saklar: türü yüzünden bekleyen dosyalar
için türü, diğerleri için yolu.

## Ayrıca bakın {#see-also}

- [`kule policy`](policy.md), [Politika kuralları](../reference/policy-rules.md)
- [`kule commit`](commit.md): `--approve --remember`.
- [Ağ ve sorular](../concepts/network-and-questions.md)

---
title: kule group
sidebar_label: kule group
description: Her birinin kendi rengi, politikası, bütçeleri ve sınırları olan görev grupları oluşturun ve yönetin.
---

# kule group

Görev **grupları** oluşturur ve yönetir. Bir grubun bir adı, bir rengi,
görevlerinin tabi olduğu bir politikası ve kendi bütçeleri ile sınırları
vardır.

## Kullanım {#synopsis}

```sh
kule group [OPTIONS] [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-group-list) | Grupları, politikalarını ve görevlerini listeler (varsayılan). |
| [`new`](#kule-group-new) | Bir grup oluşturur. |
| [`rename`](#kule-group-rename) | Bir grubu yeniden adlandırır. |
| [`color`](#kule-group-color) | Bir grubun rengini ayarlar ya da kaldırır. |
| [`policy`](#kule-group-policy) | Bir grubun görevlerinin tabi olduğu politikayı seçer ya da yazdırır. |
| [`budget`](#kule-group-budget) | Bir grubun görevlerinin birlikte ne kadar tutabileceğini gösterir ya da ayarlar. |
| [`put`](#kule-group-put) | Bir görevi bir gruba koyar ya da gruptan çıkarır. |
| [`delete`](#kule-group-delete) | Bir grubu siler; görevleri kalır. |

## Açıklama {#description}

Gruplar bir dizi göreve aynı şekilde davranmanızı sağlar: bir müşterinin
tüm görevleri ya da careful (dikkatli) politikaya tabi tutmak istediğiniz
tüm denemeler gibi. Bir görev en fazla bir grupta olabilir. Gruplar
numaralıdır (`1`, `2`, …); `ID` alan her alt komut, `kule group`'un yazdığı
bu numarayı ister.

Bir grubun görevleri için belirledikleri:

- **Politika**: görevlerinin her değişikliğini ve bağlantısını değerlendiren
  kurallar (bkz. [Politika](../concepts/policy.md)). Varsayılan olarak bir grup
  tüm görevler için geçerli olan politikayı kullanır.
- **Bütçeler**: görevlerinin tüm çalıştırmaları üzerinde bir toplam ve
  herhangi bir 24 saat üzerinde bir tane. İkisi de her görevin kendi
  bütçesinin ve tüm görevler için günlük bütçenin yanında geçerlidir.
- **Sınırlar**: [`kule limits --group ID`](limits.md) ile ayarlanır.
  Gruptaki her görevi, görevin kendi değeri olmadığı yerde sınırlar.

Bir deponun kendi `forbid` kuralları (`.kule/policy.cedar`) ve Kule'ye bir
daha sormamasını söyledikleriniz ([`kule rules`](rules.md)) her grupta
geçerlidir. Bütün resim için [Gruplar](../concepts/groups.md) sayfasına bakın.

Tüm alt komutlar `--home` alır (bkz. [genel seçenekler](index.md)).

## kule group list {#kule-group-list}

```sh
kule group list [OPTIONS]
```

Her grubu kimliği, adı, politikası, bütçeleri, kendi sınırları olup olmadığı,
rengi ve görevleriyle birlikte listeler. Alt komut olmadan `kule group` aynı
şeyi yapar.

```console
$ kule group
1  clients · the policy for every task · budget 20.00 USD, 5.00 USD a day · limits of its own (kule limits --group 1) (teal)
    1046fb30  ~/code/app
2  side projects · the policy for every task (red)
```

Henüz hiç grup yokken:

```console
$ kule group
no groups: every task is judged by the policy for every task
```

## kule group new {#kule-group-new}

```sh
kule group new [OPTIONS] <NAME>
```

| Argüman / seçenek | Anlamı |
| --- | --- |
| `NAME` | Grubun adı, en fazla 60 karakter. |
| `--color COLOR` | `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`, `pink` renklerinden biri. Verilmezse Kule başka hiçbir grubun kullanmadığı bir renk seçer. |

Yeni bir grup, siz başka bir politika seçene kadar tüm görevler için geçerli
olan politikayı kullanır. En fazla 100 grubunuz olabilir.

```console
$ kule group new clients --color teal
group 1 made: clients
$ kule group new "side projects"
group 2 made: side projects
```

## kule group rename {#kule-group-rename}

```sh
kule group rename [OPTIONS] <ID> <NAME>
```

```console
$ kule group rename 2 "Side projects"
```

Başarılı olduğunda hiçbir şey yazmaz.

## kule group color {#kule-group-color}

```sh
kule group color [OPTIONS] <ID> [COLOR]
```

Grubun rengini ayarlar; `COLOR` verilmezse rengi kaldırır. Grubun görevleri,
kendi renkleri yoksa bu rengi gösterir (bkz. [`kule color`](color.md)).

```console
$ kule group color 2 purple
$ kule group color 2
```

## kule group policy {#kule-group-policy}

```sh
kule group policy [OPTIONS] <ID> [CHOICE]
```

| Argüman / seçenek | Anlamı |
| --- | --- |
| `ID` | Grup. |
| `CHOICE` | `default` (tüm görevler için geçerli politika), `balanced`, `careful` ya da `own`. Grup için geçerli kuralları yazdırmak için boş bırakın. |
| `--file PATH` | `own` ile: grubun politikasını içeren bir Cedar dosyası. Saklanmadan önce kontrol edilir (en fazla 256 KiB). |

Grubun görevleri, bir sonraki commit'lerinden ve çalıştırmalarından
itibaren yeni seçime göre değerlendirilir.

```console
$ kule group policy 1 careful
group 1's tasks are judged by it from their next commit and run
$ kule group policy 1 | tail -3
@id("careful")
forbid (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" || resource.kind like "git.*" };
```

Grubun kendine ait bir politikası:

```console
$ kule group policy 1 own --file clients.cedar
group 1's tasks are judged by it from their next commit and run
```

Dosya saklandıktan sonra `kule group policy 1 own` (`--file` olmadan) yeniden
ona geçer. Nasıl yazılacağı için [Politika kuralları](../reference/policy-rules.md)
sayfasına bakın; en kolay başlangıç, `kule policy` çıktısının bir kopyasını
alıp değiştirmektir.

:::warning

Kendinize ait bir politika, grup için Kule'ninkinin *yerine geçer*. Dışarıda
bıraktığınız kurallar, bir deponun kendi forbid kuralları dışında, o görevler
için artık geçerli olmaz. Kule'nin balanced politikasının metninden başlayın
(balanced geçerliyken `kule policy` onu yazdırır) ve yalnızca değiştirmek
istediğinizi değiştirin.

:::

## kule group budget {#kule-group-budget}

```sh
kule group budget [OPTIONS] <ID> [AMOUNT] [CURRENCY]
```

| Argüman / seçenek | Anlamı |
| --- | --- |
| `ID` | Grup. |
| `AMOUNT` | `20` ya da `7.50` gibi pozitif bir tutar, ya da `off`. İki bütçeyi ve harcananı göstermek için boş bırakın. |
| `CURRENCY` | Büyük harfle ISO 4217. Varsayılan `USD`. |
| `--daily` | Tüm çalıştırmalar üzerindeki bütçe yerine herhangi bir 24 saat üzerindeki bütçeyi ayarlar. |

Bir grubun isteğe bağlı iki bütçesi vardır: biri **görevlerinin tüm
çalıştırmaları** üzerinde, biri **herhangi bir 24 saat** üzerinde. Bir
çalıştırma, görevi çalıştığı sırada hangi gruptaysa o grup için sayılır;
yani bir görevi gruptan çıkarmak, harcadığını grubun hesabında bırakır.

```console
$ kule group budget 1 20
budgets for GROUP 1 "clients": its 1 task together, beside each task's own and the daily one for every task:
  all their runs   20.00 USD, 0.00 USD spent, 20.00 USD left
  any 24 hours     no budget (nothing reported spent)
$ kule group budget 1 5 --daily
budgets for GROUP 1 "clients": its 1 task together, beside each task's own and the daily one for every task:
  all their runs   20.00 USD, 0.00 USD spent, 20.00 USD left
  any 24 hours     5.00 USD, 0.00 USD spent, 5.00 USD left
$ kule group budget 1 off --daily
```

Bir grup bütçesi tükendiğinde grubun görevleri hiçbir çalıştırma başlatmaz:
`group "clients" has spent its budget: its tasks together; raise it or turn it off to run again`.
Görevlerinden birinin çalıştırmalarının harcadığına dair bir kayıt
okunamıyorken de başlatmaz: kayıt, görevin şu an bulunduğu grup için, iki
bütçede de sayılır ve ret
`Kule cannot tell what group "clients"'s budget has left: …` (ya da
`… daily budget …`) der.

## kule group put {#kule-group-put}

```sh
kule group put [OPTIONS] <TASK> [GROUP]
```

`TASK`'ı `GROUP` grubuna koyar; `GROUP` verilmezse görevi grubundan çıkarır.
Ardından görevin çalıştırmalarını sınırlayan sınırlarda ve grup
bütçelerinde neyin değiştiğini söyler; böylece hiçbir şey siz görmeden
gevşemez.

```console
$ kule group put 1046fb30 1
task 1046fb30 is in group 1 "clients"
  its limits: memory 4 GB → 2 GB, one run no limit → 30 min
$ kule group put 1046fb30
task 1046fb30 is in no group
  its limits: memory 2 GB → 4 GB, one run 30 min → no limit
```

Görevin bir sonraki commit'ini ve çalıştırmasını yeni grubun politikası
değerlendirir.

## kule group delete {#kule-group-delete}

```sh
kule group delete [OPTIONS] <ID>
```

Grubu siler. Görevleri hiçbir grupta olmadan kalır ve yeniden tüm görevler
için geçerli politikaya göre değerlendirilir.

```console
$ kule group delete 2
group 2 deleted; its tasks are in no group
```

## Hatalar ve çözümler {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: no group 9` | Kimliği `kule group` ile kontrol edin. |
| `kule: a task's colour is one of red, orange, yellow, green, teal, blue, purple, pink, not "magenta"` | Bu renklerden birini seçin. |
| `kule: strict: a group's policy is default, balanced, careful or own` | Dört seçenekten birini kullanın. |
| `kule: write the group's policy first` | `own`, ilk seferde `--file PATH` ister. |
| `kule: a policy's text goes with a policy of the group's own` | `--file` yalnızca `own` ile kullanılır. |
| `kule: policy: unexpected end of input` (ya da başka bir Cedar hatası) | Dosya geçerli Cedar değil; hiçbir şey değişmedi. Dosyayı düzeltip yeniden deneyin. |
| `kule: at most 100 groups; delete one first` | Artık kullanmadığınız bir grubu silin. |

## Uygulamada {#in-the-app}

*Tasks*'ın yanındaki **+** bir grup oluşturur. Bir grubun ⋮ menüsü onu
yeniden adlandırır, rengini değiştirir, onun için *Budget…* ve *Limits…*
açar ve siler. Bir görevi oraya koymak için bir grubun üstüne sürükleyin
(ya da görevin menüsündeki *Group…* öğesini kullanın); çıkarmak için
*Not in a group* üstüne sürükleyin. Bir taşıma, görevin bekleyen
değişikliklerinden herhangi birinin kararını ya da sınırlarını ve
bütçelerini değiştirecekse uygulama önce sorar. Bir grubun politikası
*Settings → Policy*'de seçilir.

## Ayrıca bakın {#see-also}

- [`kule limits`](limits.md): bir grubun sınırları (`--group ID`).
- [`kule policy`](policy.md): tüm görevler için geçerli politika.
- [Gruplar](../concepts/groups.md), [Politika](../concepts/policy.md), [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md)

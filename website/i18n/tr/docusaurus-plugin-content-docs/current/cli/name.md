---
title: kule name
sidebar_label: kule name
description: Bir göreve, klasörünün adından sonra gösterilen bir ad verir ya da adı kaldırır.
---

# kule name

Bir göreve, klasörünün adından sonra gösterilen bir ad verir; ad verilmezse
mevcut adı kaldırır.

## Kullanım

```sh
kule name [OPTIONS] <TASK> [NAME]
```

## Açıklama

Bir görev klasörünün adıyla gösterilir. Aynı klasörde birkaç göreviniz
olduğunda, bir ad onları birbirinden ayırır: uygulama `app – greeting fix`
gösterir, `kule list` ise klasörden sonra `· greeting fix` yazar.

Ad yalnızca bir etikettir. Görevin kimliğini (komutlar yine kimlik bekler),
kopyayı ya da klasörü değiştirmez.

Baştaki ve sondaki boşluklar kırpılır. Kırpıldıktan sonra boş kalan bir ad,
mevcut adı kaldırır.

## Argümanlar

| Ad | Zorunlu | Anlamı | Kabul edilen değerler |
| --- | --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. | Bkz. [Bir göreve atıf yapmak](index.md). |
| `NAME` | hayır | Yeni ad. Adı kaldırmak için vermeyin. | En fazla 60 karakter; kontrol karakteri ya da yazı yönü karakteri olmadan. Boşluk içeriyorsa tırnak içine alın. |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Bir göreve ad vermek:**

```text
$ kule name 3efe1ca8 "greeting fix"
$ kule list
3efe1ca8  /Users/me/code/app · greeting fix
```

**Adını değiştirmek:** `kule name`'i yeni adla tekrar çalıştırın.

**Adı kaldırmak:**

```sh
kule name 3efe1ca8
```

## Çıktı

Başarılı olursa hiçbir şey yazdırmaz.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: no task <id>` | `kule list`'teki tam kimliği kullanın. |
| `kule: a task's name is at most 60 characters` | Adı kısaltın. |
| `kule: a task's name cannot hold control or text-direction characters` | Sekme, satır sonu ve sağdan sola işaretlerini çıkarın. Kule bunları, bir ad kendini başka bir şeymiş gibi gösteremesin diye reddeder. |

## Uygulamada

Görevin ⋮ menüsündeki *Rename…* ya da F2.

## Ayrıca bakın

- [kule color](color.md), [kule list](list.md)

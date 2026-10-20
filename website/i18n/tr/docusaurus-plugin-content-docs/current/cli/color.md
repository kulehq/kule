---
title: kule color
sidebar_label: kule color
description: Bir görevi sekiz renkten biriyle işaretler ya da rengini kaldırır.
---

# kule color

Bir görevi bir renkle işaretler; renk verilmezse mevcut rengi kaldırır.

## Kullanım

```sh
kule color [OPTIONS] <TASK> [COLOR]
```

## Açıklama

Renk, bir görevi uygulamanın görev listesinde kolayca bulmanızı sağlar. Bir
[gruptaki](../concepts/groups.md) görev, kendi rengi yoksa grubun rengiyle
görünür.

Renk yalnızca bir işarettir. Görevin nasıl çalıştığını ya da nasıl
değerlendirildiğini hiç değiştirmez.

## Argümanlar

| Ad | Zorunlu | Anlamı | Kabul edilen değerler |
| --- | --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. | Bkz. [Bir göreve atıf yapmak](index.md). |
| `COLOR` | hayır | Renk. Rengi kaldırmak için vermeyin. | `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`, `pink` |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Bir görevi teal ile işaretlemek:**

```text
$ kule color 3efe1ca8 teal
$ kule list
3efe1ca8  /Users/me/code/app · greeting fix (teal)
```

**Rengi kaldırmak:**

```sh
kule color 3efe1ca8
```

**Bilinmeyen bir renk reddedilir:**

```text
$ kule color 3efe1ca8 mauve
kule: a task's colour is one of red, orange, yellow, green, teal, blue, purple, pink, not "mauve"
```

## Çıktı

Başarılı olursa hiçbir şey yazdırmaz.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: a task's colour is one of red, orange, … not "<word>"` | Sekiz addan birini kullanın. |
| `kule: no task <id>` | `kule list`'teki tam kimliği kullanın. |

## Uygulamada

Görevin ⋮ menüsündeki renk örnekleri (Mod+. geçerli görev için menüyü açar;
göreve sağ tıklamak da olur).

## Ayrıca bakın

- [kule name](name.md), [kule group](group.md)

---
title: kule list
sidebar_label: kule list
description: Tüm görevleri kimliği, klasörü, adı, rengi ve grubuyla, uygulamanın gösterdiği sırayla listeler.
---

# kule list

Tüm görevleri, uygulamanın Tasks panelinde gösterdiği sırayla listeler.

## Kullanım

```sh
kule list [OPTIONS]
```

## Açıklama

`kule list`, Kule'nin ev klasöründeki her görev için bir satır yazdırır.
Diğer tüm komutların ihtiyaç duyduğu görev kimliğini bulmanın en hızlı
yoludur.

Sıra uygulamanınkiyle aynıdır: sürükleyerek (ya da Alt+Shift+↑/↓ ile)
yerleştirdiğiniz görevler o sırayı korur; hiç taşımadığınız görevler en
başta, en yenisi en üstte gelir.

Yalnızca okur; hiçbir şeyi değiştirmez.

## Argümanlar

Yok.

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Görevleri listelemek:**

```text
$ kule list
3422f830  /Users/me/code/app
40ac70b8  /Users/me/notes
3efe1ca8  /Users/me/code/app · greeting fix (teal)
```

**Adlar, renkler ve gruplarla:**

```text
$ kule list
5e6f7a8b  /Users/me/Downloads · march invoices (orange) [clients]
3422f830  /Users/me/code/app
```

**Tek bir klasördeki görevleri bulmak:**

```sh
kule list | grep ' /Users/me/code/app'
```

**Henüz görev yoksa:** `kule list` hiçbir şey yazdırmaz ve `0` ile çıkar.

## Çıktı

Her satır şöyledir:

```text
<id>  <workspace> · <name> (<colour>) [<group>]
```

| Parça | Ne zaman görünür | Nasıl ayarlanır |
| --- | --- | --- |
| `<id>` | her zaman | [kule new](new.md) oluşturur |
| `<workspace>` | her zaman: görevin kopyaladığı gerçek klasör | |
| `· <name>` | görevin bir adı varsa | [kule name](name.md) |
| `(<colour>)` | görevin bir rengi varsa | [kule color](color.md) |
| `[<group>]` | görev bir gruptaysa | [kule group put](group.md) |

## Uygulamada

**Tasks** paneli (Mod+0) aynı listeyi gösterir. Sizi bekleyen görevler
*Needs you* ("Sizi bekleyenler") altında toplanır.

## Ayrıca bakın

- [kule new](new.md), [kule name](name.md), [kule color](color.md)
- [kule group](group.md)

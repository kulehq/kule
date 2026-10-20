---
title: kule path
sidebar_label: kule path
description: cd, betikler ve editörler için bir görevin kopyasının ya da içindeki bir dosyanın diskte nerede olduğunu yazdırır.
---

# kule path

Bir görevin kopyasının ya da içindeki bir dosyanın diskte nerede olduğunu
yazdırır.

## Kullanım

```sh
kule path [OPTIONS] <TASK> [FILE]
```

## Açıklama

Kopya, Kule'nin ev klasörünün içinde, `<home>/tasks/<id>/w/<klasör adı>`
yolunda durur. `kule path` bu yolu ya da kopyadaki bir dosyanın veya
klasörün yolunu yazdırır; böylece oraya `cd` yapabilir, bir editörde
açabilir ya da bir betikte kullanabilirsiniz.

`FILE`, kopyanın içinde, kopyanın en üstüne göre bir yoldur. Kule oraya
ulaşmak için hiçbir zaman bir bağlantıyı (link) izlemez ve `..`'yı
reddeder; bu yüzden yazdırılan yol her zaman kopyanın içindedir.

[kule shell](shell.md) gibi `kule path` de, görev kopyanın git config'ini ya
da hook'larını değiştirmişse stderr'e bir uyarı yazar: o klasördeki git,
onları sandbox dışında çalıştırırdı. Yolu yine de yazdırır (tek başına bir
yol hiçbir şey çalıştırmaz), ama orada git çalıştırmadan ya da bir editör
açmadan önce uyarıyı ciddiye alın.

Yolu yalnızca stdout tutar; bu yüzden komut ikamesi (command substitution)
uyarı olsa bile çalışır.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |
| `FILE` | hayır | Kopyanın içindeki bir dosya ya da klasör, örneğin `src/main.rs`. Verilmezse kopyanın kendisi. |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Kopya:**

```text
$ kule path 3422f830
/Users/me/.kule/tasks/3422f830/w/app
```

**Oraya gitmek:**

```sh
cd "$(kule path 3422f830)"
```

**Kopyayı editörünüzde açmak:**

```sh
code "$(kule path 3422f830)"
```

**Tek bir dosya:**

```text
$ kule path 3422f830 src/main.rs
/Users/me/.kule/tasks/3422f830/w/app/src/main.rs
```

**Bir dosyayı çalışma klasöründekiyle karşılaştırmak:**

```sh
diff ~/code/app/src/main.rs "$(kule path 3422f830 src/main.rs)"
```

**Bir git config değişikliğinden sonraki uyarı:**

```text
$ kule path 3422f830
kule: careful in there: the copy's git config changed, and git there would run it outside the sandbox; review it, then commit or roll it back before opening a terminal
/Users/me/.kule/tasks/3422f830/w/app
```

## Çıktı

stdout'ta tek bir mutlak yol. Uyarı varsa, stderr'de `kule: careful in
there:` ile başlar.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: <file>: No such file or directory (os error 2)` | Dosya kopyada yok. Yollar kopyanın en üstüne göredir. |
| `kule: <file>: a path inside the copy, without ..` | `..` ya da mutlak bir yol kullanmayın. |

:::tip

Bir editör bir klasörden kendiliğinden bazı şeyler çalıştırır: görevler
(tasks), ayarlar, önerilen eklentiler, git hook'ları. Kopyada bir ajan
çalıştıysa onu VS Code'un Restricted Mode'unda açın (yazarlara güvenip
güvenmediğinizi sorduğunda *no* deyin). Uygulamadaki *Open the copy in your
editor* bunu ilk seferde açıklar.

:::

## Uygulamada

**Files**'taki bir dosyada *Copy path* (macOS'ta ⌥⌘C, Linux'ta Ctrl+Alt+C)
ve görevin ⋮ menüsünde *Open the copy in your editor*. Bkz.
[Files ve Preview](../app/files-and-preview.md).

## Ayrıca bakın

- [kule shell](shell.md), [kule open](open.md)

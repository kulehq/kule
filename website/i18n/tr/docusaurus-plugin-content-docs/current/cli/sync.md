---
title: kule sync
sidebar_label: kule sync
description: Gerçek klasörün, görevin son sync'inden beri değiştirdiklerini kopyaya alır; iki tarafın da değiştirdiği dosyaları birleştirir.
---

# kule sync

Çalışma klasörünün, görevin son sync'inden beri değiştirdiklerini kopyaya
alır (sync).

## Kullanım

```sh
kule sync [OPTIONS] <TASK>
```

## Açıklama

Bir görevin kopyası çalışma klasörünü kendiliğinden izlemez. Ajan, klasörün
görev oluşturulduğu ya da son sync yapıldığı andaki **hâli** üzerinde
çalışır. Bu arada gerçek klasörde bir dosyayı düzenlerseniz ya da başka bir
görev oraya commit ederse, kopya bunu görmez. [kule status](status.md) bu
dosyaları *the workspace changed since the task's copy* (çalışma klasörü,
görevin kopyasından beri değişti) altında listeler.

`kule sync` bunları kopyaya alır:

- **Yalnızca çalışma klasörünün değiştirdiği bir dosya** olduğu gibi gelir.
- **Görevin de değiştirdiği bir dosya** fonksiyon, metot, bölüm ya da anahtar
  düzeyinde birleştirilir ve görevin değişikliği olarak kalır (yani
  `kule status`'ta görünür).
- **Herhangi bir şey birleşmezse** hiçbir şey gelmez ve Kule iki tarafın da
  neyi değiştirdiğini adlandırır. Kopyanız olduğu gibi kalır.
- Klasörün git deposundaki **commit'ler, dallar ve stage'lenmiş dosyalar**
  da gelir. İki tarafın da ilerlettiği bir dal gelmez.
- `.env` gibi **ayar dosyaları**, kopyanın geri kalanı gibi maskelenmiş
  olarak gelir (bkz. [Gizli değerler](../concepts/secrets.md)).

Çalışma klasörü yalnızca okunur. Bir sync'ten sonra adımlar, bir commit'ten
sonraki gibi baştan başlar ve ajan bir sonraki turunun başında neyin
geldiğini öğrenir.

Bir ajan çalışırken ve görevde bir terminal (uygulamanınki ya da
[kule shell](shell.md)) açıkken sync reddedilir; görevin sandbox'ında hâlâ
çalışan programlar önce durdurulur ve Kule onların adını verir.

Birleştirmenin nasıl çalıştığı için bkz.
[Sync ve birleştirme](../concepts/sync-and-merging.md).

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve ad vermek](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (ya da `KULE_HOME`). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Alınacak bir şey yok:**

```text
$ kule sync 3efe1ca8
the copy has everything the workspace changed
```

**Yalnızca klasörün değiştirdiği bir dosya ve ikisinin de değiştirdiği, birleştirilen bir dosya:**

```text
$ kule status 3efe1ca8
…
the workspace changed since the task's copy (kule sync 3efe1ca8 brings it in):
  README.md
  src/main.rs (the task changed it too)
$ kule sync 3efe1ca8
  < README.md
  ~ src/main.rs (merged with the task's change)
```

**Klasörün yeni git commit'leri:**

```text
$ kule sync 3efe1ca8
  < the repository (commits, branches, the index)
```

**İkisi de aynı fonksiyonu değiştirdi:**

```text
$ kule sync 3efe1ca8
kule: the folder and the task both changed: src/main.rs (both changed function greet); nothing was brought in
hint: commit or roll back the task's changes there first, or leave the copy as it is
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `< path` | Çalışma klasöründeki hâliyle alındı. |
| `~ path (merged with the task's change)` | İkisi de değiştirdi, farklı yerlerde; birleştirildi. |
| `< the repository (commits, branches, the index)` | Klasörün git durumu geldi. |
| `= path (the task's own kept: both changed it)` | Görevdeki hâliyle bırakıldı. |
| `the copy has everything the workspace changed` | Yapılacak bir şey yok. |
| `kule: Kule ended <name>, still running in the task's sandbox, to sync` | stderr'de: görevin sandbox'ında hâlâ çalışan ve Kule'nin kopyaya yazmadan önce sonlandırdığı şey. Birden fazlaysa: `kule: Kule ended <N> processes still running in the task's sandbox, to sync: <names>`. Bkz. [Süreçler ve sunucular](../concepts/sandbox.md#processes-and-servers). |

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: the folder and the task both changed: <paths>; nothing was brought in` | İki taraf da aynı fonksiyonu, bölümü ya da anahtarı değiştirdi. Önce görevin oradaki değişikliğini commit edin ya da atın; ya da kopyayı olduğu gibi bırakıp çakışmayı commit sırasında çözün. |
| `kule: an agent is at work in task <id> …` | Çalıştırmanın bitmesini bekleyin. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then sync` | Görevin terminalini kapatın (uygulamadaki sekmeyi ya da `kule shell`'den `exit` ile çıkın), sonra sync yapın. |

## Uygulamada

Review'daki *Changed in the folder* (klasörde değişenler) altında *Bring into
the copy* (kopyaya al), görevin ⋮ menüsünde ya da Mod+Shift+G. Bkz.
[Review](../app/review.md).

## Ayrıca bakın

- [Sync ve birleştirme](../concepts/sync-and-merging.md)
- [kule status](status.md), [kule commit](commit.md)

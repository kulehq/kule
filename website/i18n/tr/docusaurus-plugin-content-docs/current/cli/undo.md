---
title: kule undo
sidebar_label: kule undo
description: Bir görevin en yeni commit'ini, sakladığı kontrol noktasından, gerçek klasörde ve kopyada geri alır.
---

# kule undo

Görevin en yeni commit'ini çalışma klasöründe ve kopyada geri alır (undo).

## Kullanım

```sh
kule undo [OPTIONS] <TASK>
```

## Açıklama

Her [kule commit](commit.md) bir **kontrol noktası** saklar: commit'in
çalışma klasöründe yerine koyduğu şeyleri. `kule undo` bunları geri koyar;
böylece çalışma klasörü commit'ten önceki hâline döner. Commit'in
oluşturduğu dosyalar kaldırılır, değiştirdiği ya da sildiği dosyalar geri
gelir ve kopya da aynı şekilde eski hâline getirilir.

Daha geriye gitmek için tekrar çalıştırın; geri alınacak bir şey kalmayana
kadar her seferinde bir commit geri gider.

Kontrol noktaları görevin kendi deposunda (`checkpoints/store.git` altındaki
git nesneleri, yanlarında kipleri ve zamanlarıyla), hiçbir sandbox'ın
ulaşamadığı Kule'nin ev klasöründe durur. Her undo, görevin imzalı
[geçmişine](history.md) kaydedilir.

**Çalışma klasörü commit'ten sonra değiştiyse undo reddedilir** (geri
koyacağı yollarda): undo, birinin daha yeni işinin üzerine yazmış olurdu. O
dosyaları commit'in bıraktığı hâle getirin ya da elle halledin.

Bir git deposunda, commit'in eklediği git nesneleri `.git` içinde kalır (git,
referanssız nesneleri `git gc` ile kendisi temizler); dal hareketleri ve
index geri konur.

Undo görev başına çalışır: yalnızca o görevin commit'lerini geri alır. Bir
görev [kule rm](rm.md) ile silindiğinde kontrol noktaları da gider.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve ad vermek](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (ya da `KULE_HOME`). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Son commit'i geri alın:**

```text
$ kule undo 3efe1ca8
undid checkpoint 1 (3 changes)
```

**Geri alınacak bir şey kalmadı:**

```text
$ kule undo 3efe1ca8
nothing to undo
```

**Birkaç commit geriye gidin:**

```sh
kule undo 3422f830 && kule undo 3422f830
```

**Klasör o zamandan beri değiştiği için reddedildi:**

```text
$ kule undo 3422f830
kule: the workspace changed since Kule last saw it, at: README.md
hint: roll back, or rerun the task on the current workspace
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `undid checkpoint C (N changes)` | `C` kontrol noktalı commit geri alındı; `N` yol geri kondu. |
| `nothing to undo` | Görevin geri alınacak commit'i kalmadı. |
| `kule: Kule ended <name>, still running in the task's sandbox, to undo` | stderr'de: görevin sandbox'ında hâlâ çalışan ve Kule'nin kopyaya yazmadan önce sonlandırdığı şey. Birden fazlaysa: `kule: Kule ended <N> processes still running in the task's sandbox, to undo: <names>`. Bkz. [Süreçler ve sunucular](../concepts/sandbox.md#processes-and-servers). |

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: the workspace changed since Kule last saw it, at: <paths>` | Bu dosyalar commit'ten sonra gerçek klasörde değişti. Kule bunların üzerine yazmaz. Elle eski hâline getirin ya da commit'i yerinde bırakın. |
| `kule: workspace <path> is gone …` | Gerçek klasör taşındı ya da silindi. |
| `kule: an agent is at work in task <id> …` | Çalıştırmanın bitmesini bekleyin. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then undo` | Görevin terminalini kapatın (uygulamadaki sekmeyi ya da `kule shell`'den `exit` ile çıkın), sonra geri alın. |

## Uygulamada

Timeline'daki en yeni commit üzerinde **Undo** (Mod+Shift+U, iki kez basın).
Bkz. [Timeline](../app/timeline.md).

## Ayrıca bakın

- [kule commit](commit.md), [kule history](history.md), [kule rollback](rollback.md)
- [İnceleme, commit ve geri alma](../concepts/review-commit-undo.md)

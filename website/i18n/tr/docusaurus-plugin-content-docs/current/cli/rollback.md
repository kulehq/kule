---
title: kule rollback
sidebar_label: kule rollback
description: Bir görevin commit edilmemiş değişikliklerini atar; kopya yeniden klasörle aynı olur.
---

# kule rollback

Görevin commit edilmemiş değişikliklerini atar (roll back).

## Kullanım

```sh
kule rollback [OPTIONS] <TASK>
```

## Açıklama

`kule rollback`, görevin kopyasını son commit ya da sync anındaki çalışma
klasörüyle yeniden aynı hâle getirir. O zamandan beri ajanın, `kule exec`'in
ya da sizin kopyada değiştirdiğiniz her şey gider: oluşturulan dosyalar
kaldırılır, değiştirilen ve silinenler geri gelir, taşımalar geri alınır ve
kopyanın `.git`'indeki git değişiklikleri eski hâline döner.

Çalışma klasörüne dokunulmaz; oradan çıkarılacak bir şey yoktur, çünkü
commit edilmemiş değişiklikler oraya hiç ulaşmamıştır.

Adımlar baştan başlar. Görevin kendisi kalır, bir sonraki çalıştırmaya
hazırdır.

İşi beğenmediğinizde ya da [kule exec](exec.md) içinde güvenmediğiniz bir
şeyi denedikten sonra kullanın. Zaten **commit edilmiş** değişiklikleri geri
almak için bunun yerine [kule undo](undo.md) kullanın.

:::warning

Bir rollback geri alınamaz. İşin bir kısmı saklamaya değerse önce onu commit
edin ya da kopyadan dışarı kopyalayın (`kule path TASK`).

:::

Görevin ev katmanında (home layer) yakalanan yazmalar rollback'e dahil
değildir; bunlar için [kule home](home.md) `discard` kullanın.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve ad vermek](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (ya da `KULE_HOME`). Bkz. [genel seçenekler](index.md). |

## Örnekler

**İşi atın:**

```text
$ kule status 3efe1ca8
2 file changes: +1 ~0 -1
  - README.md
      - section App
  + scratch.txt
policy: passes on its own
$ kule rollback 3efe1ca8
rolled back 2 changes
$ kule status 3efe1ca8
no changes
```

**Bir kurulum betiğini deneyin, sonra unutun:**

```sh
kule exec 3422f830 -- sh ./install.sh
kule status 3422f830
kule rollback 3422f830
```

**Atılacak bir şey yok:**

```text
$ kule rollback 3efe1ca8
rolled back 0 changes
```

## Çıktı

`rolled back N changes`; burada `N`, taşınan klasörlerin ve `.git`'in
içindekiler dahil, geri konan her yolu sayar.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: an agent is at work in task <id> …` | Çalıştırmanın bitmesini bekleyin ya da onu durdurun. |
| `kule: task <id> is busy: another kule command holds its lock` | Diğer komutun bitmesini bekleyin. |

## Uygulamada

Review altında **Roll back** (Mod+Shift+Backspace, iki kez basın). Bkz.
[Review](../app/review.md).

## Ayrıca bakın

- [kule commit](commit.md), [kule undo](undo.md)
- [İnceleme, commit ve geri alma](../concepts/review-commit-undo.md)

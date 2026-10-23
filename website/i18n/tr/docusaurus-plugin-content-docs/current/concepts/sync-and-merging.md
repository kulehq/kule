---
title: Sync ve birleştirme
sidebar_label: Sync ve birleştirme
description: Bir görev, klasörünün bu arada değişenlerini nasıl kopyaya alır ve Kule iki görevin işini fonksiyon, bölüm ya da anahtar düzeyinde nasıl birleştirir.
---

# Sync ve birleştirme

## Kopya klasörü kendiliğinden izlemez

Bir görevin kopyası, klasörün **son sync'teki hâlidir** (görev açıldığında
ya da en son sync yaptığınızda). Bu arada klasörü editörünüzde
düzenlerseniz ya da başka bir görev ona commit ederse, ajan bunu görmez.

`kule status` ve uygulamadaki Review klasörün değiştiğini söyler:

```text
the workspace changed since the task's copy (kule sync 1a2b3c4d brings it in):
  hello.txt
```

## Sync: değişiklikleri kopyaya almak

```sh
kule sync TASK
```

```text
  < hello.txt
```

Uygulamada: Review → **Bring into the copy** (Mod+Shift+G), ya da görevin ⋮
menüsü.

Sync'in yaptıkları:

- yalnızca klasörün değiştirdiği bir dosya olduğu gibi gelir;
- görevin de değiştirdiği bir dosya **birleştirilir** (aşağıya bakın) ve
  görevin değişikliği olarak kalır;
- herhangi bir şey birleşmezse **hiçbir şey** gelmez ve Kule ikisinin de
  neyi değiştirdiğini söyler:

  ```text
  kule: the folder and the task both changed: hello.txt (both changed lines 1–3); nothing was brought in
  hint: commit or roll back the task's changes there first, or leave the copy as it is
  ```

- klasörün git deposundaki commit'ler, dallar ve stage'lenmiş dosyalar da
  gelir; ikisinin de ilerlettiği bir dal gelmez;
- `.env` gibi ayar dosyaları, kopyanın geri kalanı gibi maskelenmiş olarak
  gelir (bkz. [Gizli değerler](secrets.md));
- adımlar, bir commit'ten sonraki gibi sıfırlanır;
- ajan, bir sonraki turunun başında nelerin geldiğini öğrenir.

Klasörünüz yalnızca okunur. Sync, bir ajan çalışırken reddedilir; önce
görevin sandbox'ında hâlâ çalışan programları durdurur ve Kule durdurduğu
programların adını verir.

## Birleştirme: iki görev, bir klasör

Aynı klasördeki görevler birbirlerinin işini asla görmez. Kule, ikisi de
hâlâ sürerken **nerede kesiştiklerini** söyler: aynı metot, bölüm ya da
anahtar, ya da içine bakamadığında aynı dosya.

```text
task 5e6f7a8b changes them too: task.rs · method Task::commit
```

Uygulama görevi işaretler, Review altında listeler ve iki görev kesişmeye
başladığında bir bildirim gönderir.

İkisinden **ikincisi** commit ettiğinde, ikisinin de değiştirdiği bir dosya,
ikisinin başladığı hâlden yola çıkarak üç yönlü birleştirilir:

- **farklı** metotlara, bölümlere ya da anahtarlara yapılan değişikliklerin
  ikisi de girer;
- aynı yere eklenen iki metodun ikisi de korunur;
- commit neyi birleştirdiğini söyler.

Bir **çakışma**, ikisinin de neyi değiştirdiğiyle adlandırılır:

```text
task.rs (both changed method Task::commit)
```

Aynı metoda, bölüme ya da anahtara yapılan iki değişiklik, satırları
birbirine değmese bile çakışmadır. Parçaları iki tarafın da koyduğu yerde
kalmayacak bir birleştirme de öyle.

Ajan çalışırken elle **sizin** değiştirdiğiniz bir dosya için de aynısı
geçerlidir. Her görev, birleştirme için klasörün son sync'te tuttuğunu
(`base/`) saklar: macOS'ta tek bir klon; başka yerlerde 1 MiB'a kadarki
dosyalar, `.git`, `node_modules`, `target` ve benzerlerinin dışında.

## İlgili sayfalar

- [`kule sync`](../cli/sync.md), [`kule status`](../cli/status.md), [`kule commit`](../cli/commit.md)
- [Review](../app/review.md)
- [İnceleme, commit ve geri alma](review-commit-undo.md)

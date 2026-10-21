---
title: kule closed
sidebar_label: kule closed
description: Uygulamanın bir görevin kopyasını açtığı terminal uygulamalarını ve editörleri kapattığınızı Kule'ye söyler; böylece ajanlar görevde yeniden çalışabilir.
---

# kule closed

Uygulamanın görevin kopyasını sandbox dışında açtığı terminal
uygulamalarını ve editörleri kapattığınızı Kule'ye söyler: ajanlar görevde
yeniden çalışabilir.

## Kullanım

```sh
kule closed [OPTIONS] <TASK>
```

## Açıklama

*Open in terminal app* ve *Open the copy in your editor*, görevin kopyasını
sandbox'ın dışında, sizin kimliğinizle açar. Kule kopyayı açtığı her
uygulamayı hatırlar, ama uygulamanın kapandığını göremez. Bu yüzden
kapandığını siz söyleyene kadar görevde hiçbir ajan işe başlamaz: uygulama
bir sonraki çalıştırmadan önce size sorar, `kule run` ve `kule exec` ise
uygulamanın adını veren bir mesajla reddeder.

Terminal uygulamasını ya da editörü kapatın, sonra `kule closed TASK`
çalıştırın. Kule kopyanın açıldığı tüm uygulamaları unutur ve çalıştırmalar
yeniden başlayabilir. Açık bir şey yokken çalıştırırsanız hiçbir şey
yapmaz.

Uygulamada aynı soru bir sonraki çalıştırmadan önce gelir: uygulamayı
kapatın, sonra *I closed it, start the run*'ı seçin.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Editörü kapattıktan sonra yeniden çalıştırmak:**

```text
$ kule exec 3422f830 -- cargo test
kule: Kule opened task 3422f830's copy in VS Code outside the sandbox; while that is open it can run what an agent writes there. Close it, then say so: in the app, or with `kule closed 3422f830`
$ kule closed 3422f830
$ kule exec 3422f830 -- cargo test
```

## Çıktı

Başarılı olursa hiçbir şey yazdırmaz.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: no task <id>` | `kule list`'teki tam kimliği kullanın. |

## Uygulamada

Kopyasını Kule'nin sandbox dışında açtığı bir görevde bir çalıştırmadan
önce uygulama *Close … first* diye sorar. Uygulamayı kapatın, sonra *I
closed it, start the run*'ı seçin. Bkz.
[Terminaller ve sunucular](../app/terminals-and-servers.md#outside-the-sandbox).

## Ayrıca bakın

- [kule shell](shell.md), [kule path](path.md)
- [Dosyalar ve önizleme](../app/files-and-preview.md)

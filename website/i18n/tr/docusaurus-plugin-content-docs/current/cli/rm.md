---
title: kule rm
sidebar_label: kule rm
description: Bir görevi, kopyasını ve Kule'nin onun için sakladığı her şeyi, klasöre dokunmadan siler.
---

# kule rm

Bir görevi ve kopyasını siler. Çalışma klasörüne dokunulmaz.

## Kullanım

```sh
kule rm [OPTIONS] <TASK>
```

## Açıklama

`kule rm`, Kule'nin görev için `<home>/tasks/<id>/` altında sakladığı her
şeyi siler: kopyayı, commit edilmemiş değişikliklerini, adımları, kontrol
noktalarını, günlükleri, ev katmanını (home layer) ve görevin kendi paket
önbelleklerini.

Gerçek klasörünüze (çalışma klasörüne) asla dokunulmaz ve daha önce commit
ettiğiniz değişiklikler orada kalır. **Kopyadaki commit edilmemiş
değişiklikler kaybolur**, kontrol noktaları da; bu yüzden `kule rm`'den
sonra görevin commit'lerini Kule ile artık [geri alamazsınız](undo.md).
Onlara ihtiyacınız olabilecekse önce `kule status` çalıştırın ve saklamak
istediklerinizi commit edin.

Kule görevi önce `<home>/trash` klasörüne taşır, sonra çöpü hemen boşaltır.
Daemon her başladığında çöpü yeniden boşaltır; böylece yarıda kalan bir
silme işlemi sonradan tamamlanır. Disk alanı, çöp boşaldığında geri gelir.

Bir ajanın içinde çalıştığı bir görev silinemez; önce çalıştırmayı
durdurun.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Bir görevi silmek:**

```text
$ kule rm 40ac70b8
removed task 40ac70b8
```

**Önce commit edilmemiş iş var mı diye bakmak:**

```sh
kule status 40ac70b8 && kule rm 40ac70b8
```

**Tek bir klasördeki tüm görevleri silmek:**

```sh
kule list | awk '$2 == "/Users/me/code/app" {print $1}' | xargs -n1 kule rm
```

## Çıktı

Görev silindiğinde `removed task <id>`.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: no task <id>` | `kule list`'teki tam kimliği kullanın. |
| `kule: an agent is at work in task <id> (process <pid>); wait until it is done` | Çalıştırmanın bitmesini bekleyin ya da onu durdurun (uygulamada Stop). |
| `kule: task <id> is busy: another kule command holds its lock` | Başka bir `kule` komutu (örneğin bir commit) görevi kullanıyor. O bitince tekrar deneyin. |

## Uygulamada

Görevin ⋮ menüsündeki *Delete…* ya da Mod+Backspace (iki kez basın).

## Ayrıca bakın

- [kule new](new.md)
- [Görevler ve kopyalar](../concepts/tasks-and-copies.md): bir görevin diskte ne kadar yer kapladığı.

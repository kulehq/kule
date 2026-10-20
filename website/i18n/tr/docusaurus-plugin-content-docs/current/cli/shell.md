---
title: kule shell
sidebar_label: kule shell
description: Etrafa bakmak ya da elle değişiklik yapmak için görevin kopyasında, sandbox dışında kendi kabuğunuzu açar.
---

# kule shell

Görevin kopyasında, sandbox dışında kabuğunuzu açar. Orada
değiştirdikleriniz, ajanın işi gibi incelenir ve commit edilir.

## Kullanım

```sh
kule shell [OPTIONS] <TASK>
```

## Açıklama

`kule shell`, oturum kabuğunuzu (`$SHELL` ya da `/bin/sh`) çalışma dizini
görevin kopyası olacak şekilde başlatır. Ajanın işine bakmak, ajanın
derlediği uygulamayı çalıştırmak ya da commit etmeden önce bir şeyi elle
düzeltmek için kullanın.

Bu kabuk **sizindir**: sandbox dışında, dosyalara, ağa ve kimlik
bilgilerine kendi erişiminizle çalışır. Bir kolaylıktır; güvenmediğiniz bir
şeyi çalıştırmanın yeri değildir. Bunun için [kule exec](exec.md) kullanın.

Kopyada değiştirdikleriniz, ajanınkiler gibi görevin değişikliklerinin
parçasıdır: `kule status` onları listeler, `kule commit` uygular.

Kabuğun içinde `KULE_TASK` görevin kimliğini tutar. `exit` yaptığınızda Kule
kopyadaki değişiklikleri yazdırır ve `kule shell` kabuğun çıkış koduyla
çıkar.

**Görev kopyanın git config'ini ya da hook'larını değiştirmişse Kule kabuğu
açmayı reddeder.** Prompt'unuzdaki `git status` ya da yazdığınız herhangi
bir git komutu onları sandbox dışında çalıştırırdı. Önce bu değişiklikleri
[kule status](status.md) ile inceleyin, sonra commit edin ya da atın.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Kopyada etrafa bakmak:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, outside the sandbox; exit to come back
$ ls
Cargo.toml  README.md  src
$ exit
no changes
```

**Bir şeyi elle düzeltip commit etmek:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, outside the sandbox; exit to come back
$ sed -i '' 's/hi /hello /' src/main.rs
$ exit
1 file changes: +0 ~1 -0
  ~ src/main.rs
      ~ function greet
$ kule commit 3422f830
committed 1 changes · checkpoint 2
```

**Başka bir kabuk kullanmak:**

```sh
SHELL=/bin/bash kule shell 3422f830
```

**Bir git config değişikliğinden sonra reddedilmesi:**

```text
$ kule shell 3422f830
kule: the copy's git config changed, and git there would run it outside the sandbox; review it, then commit or roll it back before opening a terminal
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `kule: in task <id>'s copy, outside the sandbox; exit to come back` | Kabuk başlamadan önce stderr'e yazılır. |
| `exit`'ten sonraki değişiklik listesi | Görevin şimdiye kadarki değişiklikleri, [kule status](status.md)'un gösterdiği gibi. |

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: the copy's git config changed, …` / `… hooks …` | `kule status TASK` çalıştırın, sonra `kule commit` (değişikliğe güveniyorsanız; yeni bir hook her zaman reddedilir) ya da `kule rollback`. |
| `kule: could not start <shell>` | `$SHELL` var olmayan bir programı gösteriyor. |

## Uygulamada

Görevin ⋮ menüsündeki *Open terminal* (Mod+T) ya da **Terminal** paneli
(Ctrl+\`). Bkz. [Terminaller ve sunucular](../app/terminals-and-servers.md).

## Ayrıca bakın

- [kule path](path.md): `cd` ya da editörünüz için yalnızca yol.
- [kule exec](exec.md): sandbox içinde bir komut.

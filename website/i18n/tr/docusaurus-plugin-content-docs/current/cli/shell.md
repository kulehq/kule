---
title: kule shell
sidebar_label: kule shell
description: Etrafa bakmak ya da elle değişiklik yapmak için görevin kopyasında, sandbox'ında kendi kabuğunuzu açar.
---

# kule shell

Görevin kopyasında, görevin sandbox'ında kabuğunuzu açar. Orada
değiştirdikleriniz, ajanın işi gibi incelenir ve commit edilir.

## Kullanım

```sh
kule shell [OPTIONS] <TASK>
```

## Açıklama

`kule shell`, kabuğunuzu (`$SHELL` ya da `/bin/sh`) çalışma dizini görevin
kopyası olacak şekilde başlatır. Ajanın işine bakmak, ajanın derlediği
uygulamayı çalıştırmak ya da commit etmeden önce bir şeyi elle düzeltmek
için kullanın.

Kabuk, [kule exec](exec.md) bir komutu nasıl çalıştırıyorsa öyle,
**görevin sandbox'ının içinde** çalışır:

- yalnızca kopyaya ve görevin tmp klasörüne yazar;
- ev klasörü, görevin ev katmanıdır: `~` içine yazdıkları
  [kule home](home.md) ile incelemeniz için yakalanır;
- ağı, görevin proxy'sinden geçer ve politikaya göre değerlendirilir;
  politikanın soracağı bir sunucu reddedilir, çünkü komut satırının
  soracağı kimse yoktur;
- ajanların kendi durumu ve kimlik bilgileri ondan gizlenir.

Kabuk kendine ait bir terminalde çalışır; Kule tuşlarınızı ve kabuğun
çıktısını aradan geçirir. Kendi terminaliniz hiçbir zaman sandbox'a
verilmez.

Kopyada değiştirdikleriniz, ajanınkiler gibi görevin değişikliklerinin
parçasıdır: `kule status` onları listeler, `kule commit` uygular.

Kabuğun içinde `KULE_TASK` görevin kimliğini tutar. `exit` yaptığınızda Kule,
`kule exec`'in yaptığı gibi kopyadaki değişiklikleri, kabuğun kurduğu
bağlantıları, ev klasörüne yazılanları ve sandbox'ın reddettiklerini
yazdırır; `kule shell` de kabuğun çıkış koduyla çıkar. macOS'ta Kule ayrıca
kabuğun sandbox'ında çalışır bıraktığı şeyi sonlandırır ve adını verir.
Linux'ta, kabuk bir hatayla çıktıysa Kule, Linux'un sandbox'ın
reddettiklerini kendisine söylemediğini belirtir. Kule sandbox'ın
reddettiklerini okuyamadıysa ya da kaydedemediyse bunu söyler ve kabuk 0
ile çıksa bile `kule shell` başarısız olur.

**Kabuk açıkken** görevde hiçbir ajan işe başlamaz (`kule run` ve
`kule exec` de reddedilir); commit, sync ve ev değişikliklerini uygulamak
kabuk kapanana kadar bekler. Görevde bir ajan çalışırken Kule kabuğu açmayı
reddeder.

### Sandbox'ın dışında {#outside-the-sandbox}

`--outside` ile kabuk sandbox'ın dışında, sizin kimliğinizle, dosyalara,
ağa ve kimlik bilgilerine kendi erişiminizle çalışır. Bir kolaylıktır;
güvenmediğiniz bir şeyi çalıştırmanın yeri değildir.

- Görevde bir ajan çalışırken ve görev kopyada kendiliğinden çalışacak bir
  şeyi değiştirmişken reddedilir: git config ya da hook'lar
  (prompt'unuzdaki `git status` ya da yazdığınız herhangi bir git komutu
  onları çalıştırırdı) ya da prompt'unuzun klasöre girerken okuduğu bir
  sürüm dosyası, bir `package.json`'ın adını verdiği paket yöneticisi ya da
  `.envrc` gibi bir dosya. Bkz.
  [kendiliğinden çalışanlar](../concepts/git.md#what-runs-by-itself-protects-your-terminal-too).
  Önce bu değişiklikleri [kule status](status.md) ile inceleyin, sonra
  commit edin ya da atın.
- Kopyada commit edilmemiş değişiklikler varken Kule bunları stderr'e
  listeler (en fazla 20 tane) ve kabuğu açmadan önce sorar. Soracağı bir
  terminal yoksa, `--yes` vermediğiniz sürece reddeder; değişiklikler yine
  de listelenir.

Sandbox'taki kabuk gibi, bu kabuk da kapanana kadar görevde açık bir
terminal sayılır.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--outside` | | | Kabuğu sandbox'ın dışında, sizin kimliğinizle açar. |
| `--yes` | | | `--outside` ile: sormadan açar. Commit edilmemiş değişiklikler yine de listelenir. |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Kopyada etrafa bakmak:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, in its sandbox; exit to come back
$ ls
Cargo.toml  README.md  src
$ exit
no changes
```

**Bir şeyi elle düzeltip commit etmek:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, in its sandbox; exit to come back
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

**Kopyada değişiklikler varken sandbox'ın dışında:**

```text
$ kule shell 3422f830 --outside
kule: the copy holds changes not committed, which your shell, its prompt and the tools you run there may run as you, outside the sandbox:
  src/main.rs
  NOTES.md
kule: open your shell there outside the sandbox? [y/N] y
kule: in task 3422f830's copy, outside the sandbox; exit to come back
```

**Bir git config değişikliğinden sonra sandbox dışında reddedilmesi:**

```text
$ kule shell 3422f830 --outside
kule: the copy's git config changed, and git there would run it outside the sandbox; review it, then commit or roll it back before opening a terminal
```

**Kabuk açıkken commit'in beklemesi:**

```text
$ kule commit 3422f830
kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then commit
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `kule: in task <id>'s copy, in its sandbox; exit to come back` | Kabuk başlamadan önce stderr'e yazılır. |
| `kule: in task <id>'s copy, outside the sandbox; exit to come back` | Aynısı, `--outside` ile. |
| `exit`'ten sonraki değişiklik listesi | Görevin şimdiye kadarki değişiklikleri, [kule status](status.md)'un gösterdiği gibi. |
| `exit`'ten sonraki `net:`, ev ve `sandbox:` satırları | Kabuğun kurduğu bağlantılar, ev klasörüne yazdıkları ve sandbox'ın reddettikleri, [kule exec](exec.md)'in yazdırdığı gibi. `--outside` ile yazılmaz. |
| `kule: Kule ended <names>, which the shell left running in its sandbox` | macOS: kabuğun çalışır bıraktığı ve Kule'nin kabuk çıkarken sonlandırdığı şey. |
| `kule: Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error` | Linux, kabuk bir hatayla çıktığında: hiç `sandbox:` satırı olmaması hiçbir şeyin reddedilmediği anlamına gelmez. |

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: an agent is at work in task <id> …` | Ajanın çalıştırmasının bitmesini bekleyin. |
| `kule: the copy's git config changed, …` / `… hooks …` | Yalnızca `--outside` ile. `kule status TASK` çalıştırın, sonra `kule commit` (değişikliğe güveniyorsanız; yeni bir hook her zaman reddedilir) ya da `kule rollback`. Ya da kabuğu `--outside` olmadan, sandbox'ta açın. |
| `kule: review them first, or pass --yes to open your shell there anyway` | `--outside` ile, kopyada commit edilmemiş değişiklikler var ve soracak bir terminal yok. Onlara [kule status](status.md) ile bakın ya da `--yes` verin. |
| `kule: not opened` | *Hayır* dediniz. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then commit` | Kabuk açıkken `kule commit`, `kule sync`, `kule rollback`, `kule undo` ya da `kule home apply`/`undo`'dan gelir. Önce kabuktan `exit` ile çıkın. |
| `kule: could not start <shell>` | `$SHELL` var olmayan bir programı gösteriyor. |

## Uygulamada

**Terminal** paneli (Ctrl+\`), `kule shell` gibi kabuğunuzu sandbox'ta
çalıştırır. Görevin ⋮ menüsündeki *Open in terminal app* (Mod+T), `--outside`
gibi kopyayı terminal uygulamanızda, sandbox dışında açar. Bkz.
[Terminaller ve sunucular](../app/terminals-and-servers.md).

## Ayrıca bakın

- [kule closed](closed.md): uygulamanın kopyayı açtığı şeyi kapattıktan sonra.
- [kule path](path.md): `cd` ya da editörünüz için yalnızca yol.
- [kule exec](exec.md): sandbox içinde tek bir komut.

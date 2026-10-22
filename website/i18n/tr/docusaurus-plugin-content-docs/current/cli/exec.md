---
title: kule exec
sidebar_label: kule exec
description: Görevin kopyasında, sandbox içinde ve görevin proxy'sinin arkasında tek bir komut çalıştırır, sonra neyi değiştirdiğini listeler.
---

# kule exec

Görevin kopyasında, sandbox içinde bir komut çalıştırır, sonra neyi
değiştirdiğini gösterir.

## Kullanım

```sh
kule exec [OPTIONS] <TASK> -- <COMMAND>...
```

## Açıklama

`kule exec`, ajansız bir `kule run`'dır: komutu siz belirlersiniz. Bir
görevde derleme, test ya da deneme yapmanın yolu budur; henüz güvenmediğiniz
bir betiği ya da kurulum programını, neyi değiştireceğini görmek için
çalıştırmanın yolu da budur.

Komut görevin kopyasında (çalışma dizini kopyadır), işletim sisteminin
sandbox'ı (yalıtılmış ortamı) içinde çalışır: macOS'ta Seatbelt, Linux'ta
Landlock ve namespace'ler. Varsayılan olarak:

- **Yalnızca şunlara yazabilir:** kopya, görevin kendi `TMPDIR`'ı, paket
  önbellekleri ve Linux'ta görevin kendine ait olan `/tmp` (macOS'ta `/tmp`
  sisteme aittir, bu yüzden kapalı kalır).
- **Bağlantıları yalnızca bu bilgisayarda dinler.** Sandbox'ın bir sunucunun
  adresini ayırt edemediği macOS'ta, ağ trafiği proxy'den geçerken, görevin
  `servers` [sınırı](limits.md) izin vermedikçe hiç dinlemez.
- **Şunları okuyamaz:** kimlik bilgileriniz (`~/.ssh`, `~/.aws`,
  `~/.npmrc`, cargo'nun registry token'ı ve daha birçoğu), kendi
  klasörleriniz (Documents, Desktop, Downloads, Pictures, bulut depolama),
  Kule'nin ev klasörü ve çalışma klasörünün yanındaki projeler.
- **Ev dizini** görevin ev katmanıdır (home layer): `~/.zshrc` ya da
  `~/.config`'e yazdıkları, [kule home](home.md) ile incelenmek üzere
  yakalanır.
- **Ağ trafiği** görevin proxy'sinden geçer. Paket depoları geçer;
  politikanın geçirmediği her şey reddedilir, çünkü CLI'da soracak kimse
  yoktur. Her bağlantı günlüğe yazılır.
- **Ortamı** yalnızca araçların kendilerini bulmak için ihtiyaç duyduklarını
  içerir. Ortamınızdaki token'lar ve ajan soketleri dışarıda bırakılır.
- `KULE_TASK` görevin kimliğini tutar.

Sandbox kurulamazsa komut çalışmaz.

Komut bittiğinde Kule kopyadaki değişiklikleri (yalnızca bu komutunkileri
değil, hepsini), yaptığı bağlantıları, ev dizinine yazmaları ve macOS'ta
sandbox'ın reddettiklerini yazdırır. `kule exec` komutun kendi çıkış koduyla
çıkar.

`exec` adım oluşturmaz; adımlar bir ajanın araç çağrılarıdır. Görevde bir
`exec` çalışırken commit ve rollback reddedilir.

Neyin gizlendiğinin ve nedeninin tam listesi için bkz.
[Sandbox](../concepts/sandbox.md).

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |
| `COMMAND...` | evet | `--`'den sonra program ve argümanları. Bir kabuk üzerinden değil, doğrudan çalıştırılır; borular (pipe), `&&` ya da glob'lar için `sh -c '…'` kullanın. |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--allow-write` | yol | | Komutun bu yolun altına da yazmasına izin verir. Tekrarlanabilir. |
| `--hide` | yol | | Bu yolu da komuttan gizler. Tekrarlanabilir. |
| `--reveal` | yol | | Komutun varsayılan olarak gizli bir yolu okumasına izin verir; örneğin bir ajanın kendi oturumu için `~/Library/Keychains` ya da çalışma klasörünün yanındaki bir proje (salt okunur). Tekrarlanabilir. Kule'nin ev klasörü ve görevin kendi klasörü asla açılmaz. |
| `--pass-env` | ad | | Bu ortam değişkenini komuta aktarır. Tekrarlanabilir. |
| `--no-network` | | | Komut için TCP ağını, localhost dahil, kapatır. |
| `--open-network` | | | Komutun proxy'yi ve politikayı atlayarak her yere bağlanmasına izin verir. |
| `--no-sandbox` | | | Sandbox olmadan çalıştırır. Komut sizin yazabildiğiniz her yere yazabilir ve her host'a ulaşabilir; yukarıdaki seçeneklerin o zaman bir anlamı kalmaz. |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

:::warning

`--reveal`, `--allow-write`, `--open-network` ve `--no-sandbox`, Kule'nin
bilerek kapattığı delikleri açar. Bunları güvendiğiniz bir komut için ve
olabildiğince dar kullanın: üst klasörü değil, tek bir klasörü `--reveal`
edin.

:::

## Örnekler

**Testleri çalıştırmak:**

```sh
kule exec 3422f830 -- cargo test
```

**Bir komutun neyi değiştirdiğini görmek:**

```text
$ kule exec 3efe1ca8 -- sh -c 'echo notes > NOTES.md; ./update-main.sh'
2 file changes: +1 ~1 -0
  + NOTES.md
  ~ src/main.rs
      ~ function main
      + function wave
```

**Sandbox'ın reddettikleri (macOS):**

```text
$ kule exec 3efe1ca8 -- sh -c 'ls ~/.ssh; echo hi > ~/notes.txt'
ls: /Users/me/.ssh: Operation not permitted
sh: /Users/me/notes.txt: Operation not permitted
sandbox: refused file-read-metadata /Users/me/.ssh (ls)
sandbox: refused file-write-create /Users/me/notes.txt (bash)
```

**Ağ trafiği proxy'den geçer:**

```text
$ kule exec 3efe1ca8 -- sh -c 'curl -sI https://registry.npmjs.org/ >/dev/null; curl -sI https://example.com/'
net: registry.npmjs.org:443 · tunneled (package-registries)
net: example.com:443 · refused, would wait for approval; the command line asks no one (no-rule-lets-it-pass)
```

Bir host'u her seferinde geçirmek için bkz. [kule rules allow](rules.md).

**Hiç ağ olmadan:**

```sh
kule exec 3422f830 --no-network -- ./build.sh
```

**Henüz güvenmediğiniz bir kurulum programını denemek:**

```sh
kule exec 3422f830 -- sh ./install.sh
kule status 3422f830     # neyi değiştireceği
kule home 3422f830       # ev dizininize ne yazmaya çalıştığı
kule rollback 3422f830   # hepsini atın
```

**Klasörün yanındaki bir projeye karşı derlemek:**

```sh
kule exec 3422f830 --reveal ../shared -- npm run build
```

Klasördeki her görev için açmak üzere [kule reveal add](reveal.md)
kullanın.

**Başsız (headless) bir ajan CLI'ını kendiniz çalıştırmak** (`kule run` yerine):

```sh
kule exec "$TASK" --reveal ~/Library/Keychains \
  --allow-write ~/.claude --allow-write ~/.claude.json \
  -- claude -p "Fix the failing test, run cargo test." \
     --allowedTools "Bash,Read,Edit,Write"
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| komutun kendi çıktısı | Çalışırken yazdırılır. |
| `N file changes: +a ~m -d` ve liste | Kopyada olup çalışma klasöründe olmayan her şey, [kule status](status.md)'un gösterdiği gibi. |
| `N ignored by the workspace, staying in the copy …` | Çalışma klasörünün `.gitignore`'unun dışarıda bıraktığı yollar (derleme çıktısı, `.env`). `--include-ignored` vermezseniz bir commit bunları geride bırakır. |
| `git:` | Depo değişiklikleri. Bkz. [Git](../concepts/git.md). |
| `net: <host>:<port> · tunneled (<rule>)` | Politikanın geçirdiği bir bağlantı ve onu geçiren kural. |
| `net: <host>:<port> · refused, …` | Politikanın geçirmediği bir bağlantı. |
| `sandbox: refused <operation> <path> (<program>)` | macOS'ta sandbox'ın reddettikleri. Bunlar görevin `sandbox.jsonl` dosyasında da saklanır. |
| `kule: running without the sandbox` | `--no-sandbox` verdiniz. |
| `kule: network open, bypassing the proxy and the policy` | `--open-network` verdiniz. |

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| Komuttan `Operation not permitted` | Sandbox reddetti. `sandbox:` satırlarına bakın; komutun o yola gerçekten ihtiyacı varsa onun için `--reveal` ya da `--allow-write` kullanın. |
| bir `net: … refused` satırı ve başarısız bir indirme | Host'u [kule rules allow](rules.md) ile geçirin ya da bağlantının cevabınızı beklediği uygulamayı kullanın. |
| `kule: could not run <program>` | Program, komutun gördüğü `PATH` üzerinde değil ya da çalıştırılabilir değil. |
| `kule: an agent is at work in task <id> …` | Ajanın çalıştırmasının bitmesini bekleyin. |
| `kule: a terminal is open on task <id>'s copy …; close it first, then start work` | Görevde bir terminal (uygulamanınki ya da `kule shell`) açık. Önce onu kapatın. |
| `kule: Kule opened task <id>'s copy in <app> outside the sandbox; …` | O terminal uygulamasını ya da editörü kapatın, sonra [kule closed](closed.md) `TASK` çalıştırın. |

Unprivileged user namespace'leri olmayan Linux sistemleri için bkz.
[kule isolation](isolation.md).

## Uygulamada

Uygulamada tek bir komut için bir alan yoktur. Komutları kendiniz
çalıştırmak için görevin **Terminal**'ini açın (Ctrl+\`): `kule exec` ve
[kule shell](shell.md) gibi görevin sandbox'ında çalışır.
`kule shell --outside` ve uygulamanın *Open in terminal app*'i kopyayı
sandbox'ın dışında açar.

## Ayrıca bakın

- [Sandbox](../concepts/sandbox.md), [Ağ ve sorular](../concepts/network-and-questions.md)
- [kule run](run.md), [kule status](status.md), [kule reveal](reveal.md), [kule rules](rules.md)

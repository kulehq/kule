---
title: kule commit
sidebar_label: kule commit
description: Bir görevin değişikliklerini, politikanın izin verdiği ölçüde gerçek klasöre uygular ve undo'nun geri koyabileceği bir kontrol noktası saklar.
---

# kule commit

Görevin değişikliklerini çalışma klasörüne uygular ve bir kontrol noktası
saklar. Neyin geçebileceğine politika karar verir.

## Kullanım

```sh
kule commit [OPTIONS] <TASK>
```

## Açıklama

`kule commit`, gerçek klasörünüze yazan tek komuttur. [kule status](status.md)'un
gösterdiği değişiklikleri alır ve çalışma klasörüne uygular.

**Önce politika karar verir.** Her değişikliğin bir kararı vardır (bkz.
[Politika](../concepts/policy.md)):

- **kendiliğinden geçen** değişiklikler commit edilir;
- **onay bekleyen** değişiklikler (yerleşik *balanced* politikasında
  manifest'ler, lock dosyaları, CI dosyaları, git config, toplu silmeler, …)
  yalnızca `--approve` ile commit edilir;
- **reddedilen** değişiklikler (örneğin yeni bir git hook'u) `--approve` ile
  bile asla commit edilmez.

Bir şey geri tutulursa **hiçbir şey** commit edilmez: Kule kararı yazdırır ve
`3` çıkış koduyla çıkar. Bir commit ya hep ya hiçtir.

**Bir kontrol noktası saklanır.** Kule yazmadan önce, commit'in yerine
koyacağı şeyleri görevin kendi kontrol noktası deposuna kaydeder; böylece
[kule undo](undo.md) klasörü eski hâline getirebilir. Her commit ayrıca
görevin imzalı [geçmişine](history.md) bir kayıt olarak eklenir.

**Dosyalar güvenle yazılır.** Her dosya bir yeniden adlandırma (rename)
üzerinden yerine konur; kimse yarım bir dosya görmez. Kule'nin `.env`
dosyalarında maskelediği değerler yazılırken gerçek değerlerine döndürülür.

**Çalışma klasörü bu arada değişmemiş olmalı.** Görevin de değiştirdiği bir
dosyayı biri gerçek klasörde değiştirdiyse, iki değişiklik her ikisinin de
başladığı hâlden yola çıkılarak üç yönlü birleştirilir: farklı fonksiyonlara,
bölümlere ya da anahtarlara yapılan değişikliklerin ikisi de girer. İkisi de
**aynı** fonksiyonu, bölümü ya da anahtarı değiştirdiyse commit bir çakışma
olarak reddedilir ve hiçbir şey yazılmaz. Bkz.
[Sync ve birleştirme](../concepts/sync-and-merging.md).

**Yok sayılan yollar geride kalır.** Çalışma klasörünün `.gitignore`'unun
dışarıda bıraktığı yollar (derleme çıktısı, önbellekler, `.env`),
`--include-ignored` vermediğiniz sürece kopyada kalır. Politika bunları da
değerlendirir.

Görevde bir ajan ya da bir `kule exec` komutu çalışırken ve görevde bir
terminal (uygulamanınki ya da [kule shell](shell.md)) açıkken
`kule commit` reddedilir.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve ad vermek](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--approve` | | | Politikanın bir insana sorduğu değişiklikleri de commit eder. Reddedilen değişiklikler yine asla geçmez. |
| `--remember` | | | `--approve` ile birlikte: bu klasörde, şimdi onaylanan dosyalar (manifest'ler, CI ve proje dosyaları) hakkında bir daha sormaz. Bkz. [kule rules](rules.md). |
| `--include-ignored` | | | Çalışma klasörünün `.gitignore`'unun dışarıda bıraktığı yolları da alır. |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (ya da `KULE_HOME`). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Her şey geçiyor:**

```text
$ kule commit 3422f830
committed 1 changes · checkpoint 2
```

**Bir şey onay bekliyor:**

```text
$ kule commit 3efe1ca8
policy: waits for approval
  Cargo.toml · waits for approval (manifests-and-lockfiles)
kule: not committed; rerun with --approve to approve the changes above
$ echo $?
3
$ kule commit 3efe1ca8 --approve
committed 3 changes · checkpoint 1
```

**Onaylayın ve bu klasörde bu dosyalar hakkında bir daha sorulmasın:**

```sh
kule commit 3efe1ca8 --approve --remember
```

```text
committed 3 changes · checkpoint 1
not asked again on this folder: changes to Cargo.toml (manifests-and-lockfiles)
```

**Reddedilen bir değişiklik:**

```text
$ kule commit 3422f830 --approve
policy: denied
  git hook pre-commit · denied (git-hooks)
kule: not committed; the policy denies the changes above
```

Değişiklikleri atın (`kule rollback`), ya da kopyayı `kule shell` içinde
elle düzeltip `kule status`'a tekrar bakın.

**Derleme çıktısını da alın:**

```text
$ kule commit 3422f830 --include-ignored
committed 2 changes · checkpoint 3
```

**Çalışma klasörüyle bir çakışma:**

```text
$ kule commit 3efe1ca8
kule: the workspace changed since Kule last saw it, at: README.md (both changed section App)
hint: roll back, or rerun the task on the current workspace
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `committed N changes · checkpoint C` | Değişiklikler yerine kondu. `N`, commit'in yazdığı her yolu (`.git` içindekiler dahil) sayar; bu yüzden `kule status`'un gösterdiği dosya sayısından büyük olabilir. `C`, [kule undo](undo.md)'nun kullandığı kontrol noktasıdır. |
| `merged with what the folder changed meanwhile: <paths>` | Bu dosyalar çalışma klasöründe de değişmişti ve birleştirildi. |
| `not asked again on this folder: …` | `--remember` ile: Kule'nin bundan sonra sormadan geçireceği şeyler. |
| `nothing to commit` | Kopya, çalışma klasöründekinin aynısını tutuyor. |
| `kule: Kule ended <name>, still running in the task's sandbox, to commit` | stderr'de: görevin sandbox'ında hâlâ çalışan ve Kule'nin kopyaya yazmadan önce sonlandırdığı şey. Birden fazlaysa: `kule: Kule ended <N> processes still running in the task's sandbox, to commit: <names>`. Bkz. [Süreçler ve sunucular](../concepts/sandbox.md#processes-and-servers). |
| `policy: …` ve `kule: not committed; …` | Karar değişiklikleri geri tuttu; hiçbir şey yazılmadı (çıkış kodu `3`). |

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: not committed; rerun with --approve to approve the changes above` | Neyin beklediğine bakın (`kule status`), katılıyorsanız `kule commit --approve` çalıştırın. |
| `kule: not committed; the policy denies the changes above` | Reddedilen değişiklikler asla geçmez. Değişiklikleri atın ya da o değişikliği kopyada geri alın. |
| `kule: the workspace changed since Kule last saw it, at: <paths>` | Biri bu dosyaların aynı kısımlarını gerçek klasörde değiştirdi. `kule rollback` çalıştırın, ya da (birleşebiliyorlarsa) klasörün değişikliklerini [kule sync](sync.md) ile kopyaya alıp yeniden inceleyin. |
| `kule: workspace <path> is gone …` | Gerçek klasör taşındı ya da silindi; commit'in yazacağı bir yer yok. |
| `kule: <path> changed since it was reviewed; nothing was committed` | Bir ayar dosyası inceleme ile commit arasında değişti. `kule status`'a tekrar bakın. |
| `kule: task <id> is busy: another kule command holds its lock` | Diğer komutun bitmesini bekleyin. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then commit` | Görevin terminalini kapatın (uygulamadaki sekmeyi ya da `kule shell`'den `exit` ile çıkın), sonra commit edin. Kule orada çalışanı sonlandırmaz; böylece kaydedilmemiş hiçbir şey kaybolmaz. |

## Uygulamada

Review altında **Commit**, ya da bir şey onay bekliyorsa **Approve and
commit** (onayla ve commit et) (Mod+Shift+Enter). Geri tutulan bir dosyadaki
*Don't ask again in this folder* (bu klasörde bir daha sorma) `--remember`'ın,
*Also commit these* (bunları da commit et) ise `--include-ignored`'ın
yaptığını yapar. Bkz. [Review](../app/review.md).

## Ayrıca bakın

- [İnceleme, commit ve geri alma](../concepts/review-commit-undo.md), [Politika](../concepts/policy.md)
- [kule status](status.md), [kule undo](undo.md), [kule rollback](rollback.md), [kule rules](rules.md)

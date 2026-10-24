---
title: Terminalde hızlı başlangıç
sidebar_label: Hızlı başlangıç (CLI)
description: Bir görev açan, kopyasını sandbox içinde değiştiren, inceleyen, commit eden, geri alan ve değişiklikleri atan, ardından görevi bir ajana veren beş dakikalık bir uygulama.
---

# Terminalde hızlı başlangıç

Beş dakikada küçük bir klasör üzerinde bir görev açacak, kopyasını sandbox
içinde değiştirecek, neyin değiştiğini inceleyecek, bunu gerçek klasöre
commit edecek, geri alacak ve sonunda işi bir ajana yaptıracaksınız.

`kule`'nin `PATH`'inizde olması gerekir ([Kurulum](install.md)). Aşağıdaki
her komutu olduğu gibi yapıştırabilirsiniz.

## 1. Bir demo klasörü oluşturun

```sh
mkdir -p ~/demo && cd ~/demo
printf '# Notes\n\n## Plan\n\n- write docs\n' > notes.md
printf 'hello\n' > hello.txt
```

Bu sizin **çalışma klasörünüz** (workspace): gerçek klasör. Siz commit
edene kadar Kule onu değiştirmez.

## 2. Bir görev açın

```sh
kule new ~/demo
```

```text
task c224e328 · copy at ~/.kule/tasks/c224e328/w/demo
```

**Görevin** bir kimliği (burada `c224e328`; sizinki farklı olur) ve
klasörün bir **kopyası** vardır. macOS'ta ve Btrfs ya da XFS'te kopya bir
klondur; dosyalar değişene kadar ek yer kaplamaz.

Sayfanın geri kalanı olduğu gibi yapıştırılabilsin diye kimliği bir
değişkende tutun. `kule list` her satıra bir görev yazar, kimlik başta:

```sh
kule list
```

```text
c224e328  ~/demo
```

```sh
T=c224e328      # kendi kimliğinizi kullanın
```

Bkz. [`kule new`](../cli/new.md) ve [`kule list`](../cli/list.md).

## 3. Sandbox içinde bir komut çalıştırın

[`kule exec`](../cli/exec.md) herhangi bir komutu görevin kopyasında,
sandbox içinde çalıştırır ve ardından neleri değiştirdiğini listeler:

```sh
kule exec $T -- sh -c '
  printf "\n## Later\n\n- ship it\n" >> notes.md
  echo hi > new.txt
  rm hello.txt'
```

```text
3 file changes: +1 ~1 -1
  - hello.txt
  + new.txt
  ~ notes.md
      + section Notes › Later
```

`+` oluşturulan, `~` değiştirilen, `-` silinen dosyadır. Değişen bir
dosyanın altında Kule, dosyanın *içinde* neyin değiştiğini adıyla söyler:
burada yeni bir Markdown bölümü. Kod için fonksiyonları ve metotları, JSON
ve TOML için anahtarları adlandırır.

Gerçek klasörünüze dokunulmadı:

```sh
ls ~/demo        # hello.txt  notes.md
```

## 4. İnceleyin

```sh
kule status $T
```

```text
3 file changes: +1 ~1 -1
  - hello.txt
  + new.txt
  ~ notes.md
      + section Notes › Later
policy: passes on its own
```

Son satır **politikanın kararıdır**. Kaynak kod, belge ve veri dosyaları
kendiliğinden geçer; diğer dosya türleri ve gizli dosyalar sizi bekler. Bkz. [`kule status`](../cli/status.md) (`--json`
seçeneği de vardır).

## 5. Commit edin

```sh
kule commit $T
```

```text
committed 3 changes · checkpoint 1
```

Artık değişiklikler `~/demo`'da. Commit **checkpoint 1**'i (1 numaralı
kontrol noktasını) sakladı: yerine koyduklarını, geri alınabilsin diye.

```sh
ls ~/demo        # new.txt  notes.md
kule history $T
```

```text
  0. commit 1 · 3 changes · snapshot 0d0d2c26745d
history verified: 1 entries, signed
```

Her commit ve undo, imzalı ve hash zinciriyle bağlı bir geçmişte bir
kayıttır. Bkz. [`kule commit`](../cli/commit.md) ve
[`kule history`](../cli/history.md).

## 6. Geri alın

```sh
kule undo $T
```

```text
undid checkpoint 1 (3 changes)
```

`~/demo` eski hâline döndü: `hello.txt` ve `notes.md`, yeni bölüm olmadan.
Daha geriye gitmek için `undo`'yu tekrar çalıştırın. Bkz.
[`kule undo`](../cli/undo.md).

## 7. Sizi bekleyen bir değişiklik

Bazı dosyalar neyin kurulacağını, derleneceğini ya da çalışacağını
belirler. Politika bunların kendiliğinden geçmesine izin vermez:

```sh
kule exec $T -- sh -c 'echo "{\"name\":\"demo\"}" > package.json; echo scratch > draft.txt'
kule status $T
```

```text
2 file changes: +2 ~0 -0
  + draft.txt
  + package.json
      + key name
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    package.json
```

`manifests-and-lockfiles`, soruyu soran kuralın adıdır. Düz bir commit
artık reddedilir:

```sh
kule commit $T
```

```text
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    package.json
kule: not committed; rerun with --approve to approve the changes above
```

İki seçeneğiniz var:

- `kule commit $T --approve` onaylar ve commit eder. Bu klasörde bu
  dosyalar için bir daha sorulmasın istiyorsanız `--remember` ekleyin.
- `kule rollback $T` değişiklikleri atar.

## 8. Değişiklikleri atın (roll back)

```sh
kule rollback $T
```

```text
rolled back 2 changes
```

Kopya yeniden çalışma klasörüyle aynı. Bkz.
[`kule rollback`](../cli/rollback.md).

## 9. İşi bir ajana yaptırın

Buraya kadarki her şey, işi bir ajan yaptığında da aynı şekilde çalışır.
Önce bir ajan kurup oturum açın ([Kurulum](install.md) sayfasındaki
*Ajanları kurma* bölümüne bakın), sonra:

```sh
kule run $T --agent claude-code "Add a 'Later' section to notes.md with one idea for the project."
```

- Ajan **Agent Client Protocol** (ACP) üzerinden, aynı sandbox içinde ve
  ağ trafiği görevin proxy'sinden geçerek çalışır.
- Yanıtı ve araç çağrıları geldikçe ekrana yazılır.
- Bir şeyi değiştiren her araç çağrısı bir **adım** olur:
  `kule steps $T` onları listeler.
- Politikanın bir insana soracağı bağlantılar komut satırından hemen
  reddedilir (soracak kimse yoktur). [Uygulama](first-task-in-the-app.md)
  ise bunun yerine size sorar.
- Çalıştırma, görevin [bütçelerine ve sınırlarına](../concepts/budgets-and-limits.md)
  tabidir.

Bittiğinde, eskisi gibi inceleyip karar verin:

```sh
kule status $T
kule commit $T          # ya da: kule rollback $T
```

`--agent`, `claude-code` (varsayılan), `codex` ya da `gemini` alır; ya da
eklediğiniz bir ajanın adını. Bkz. [`kule run`](../cli/run.md).

## 10. Temizlik

```sh
kule rm $T
```

```text
removed task c224e328
```

`kule rm` görevi ve kopyasını siler. Çalışma klasörüne asla dokunmaz.

## Neler öğrendiniz

```text
kule new FOLDER          make a task (a copy)
kule exec TASK -- CMD    run a command in the sandbox
kule run TASK "PROMPT"   let an agent work
kule status TASK         what changed, and the policy's verdict
kule commit TASK         apply it to the folder (keeps a checkpoint)
kule undo TASK           take the last commit back
kule rollback TASK       throw uncommitted changes away
```

| Komut | Ne yapar |
| --- | --- |
| `kule new FOLDER` | bir görev açar (bir kopya) |
| `kule exec TASK -- CMD` | sandbox içinde bir komut çalıştırır |
| `kule run TASK "PROMPT"` | işi bir ajana yaptırır |
| `kule status TASK` | neyin değiştiğini ve politikanın kararını gösterir |
| `kule commit TASK` | değişiklikleri klasöre uygular (bir kontrol noktası saklar) |
| `kule undo TASK` | son commit'i geri alır |
| `kule rollback TASK` | commit edilmemiş değişiklikleri atar |

Sırada: aynı döngü [masaüstü uygulamasında](first-task-in-the-app.md), ya
da işin altında [Kule nasıl çalışır](../concepts/how-kule-works.md).

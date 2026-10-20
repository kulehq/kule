---
title: İnceleme, commit ve geri alma
sidebar_label: İnceleme, commit ve geri alma
description: Kule bir görevin neyi değiştirdiğini nasıl gösterir, bir commit bunu bir kontrol noktasıyla nasıl güvenle uygular; rollback, undo, adımlar ve imzalı geçmiş nasıl çalışır.
---

# İnceleme, commit ve geri alma

## İnceleme: neler değişti

Kule bir görevin neyi değiştirdiğini, görevin kopyasını çalışma klasörüyle
karşılaştırarak bulur. `kule status` (ya da uygulamada **Review**) bunu
gösterir:

```text
3 file changes: +1 ~1 -1
  - hello.txt
  + new.txt
  ~ notes.md
      + section Notes › Later
policy: passes on its own
```

- `+` oluşturuldu, `~` değiştirildi, `-` silindi. Taşımalar bir silme ve
  bir oluşturma olarak değil, taşıma olarak gösterilir
  (`» photos/ → 2026/`).
- Değişen bir dosyanın altında Kule, **içinde neyin değiştiğini** bir
  insanın kullandığı adlarla söyler:
  - Rust, C#, TypeScript, JavaScript, Python ve Go'da fonksiyonlar,
    metotlar, alanlar ve tipler;
  - Markdown'da bölümler;
  - JSON ve TOML'da anahtarlar.

  Bir dosyadaki her değişiklik bunlardan birinin ya da dosyanın üst
  düzeyinin altında listelenir.
- Değişen bir resim (PNG, JPEG, GIF, WebP) uygulamada öncesi ve sonrasıyla
  gösterilir.
- Bir git deposunda dal hareketleri, yeni nesneler, index, config ve
  hook'lar kendi satırlarında gösterilir. Bkz. [Git depoları](git.md).
- Son satır [politikanın](policy.md) kararıdır.

`kule status TASK --json` aynısını betikler için JSON olarak verir.

## Commit: klasöre uygulamak

```sh
kule commit TASK
```

```text
committed 3 changes · checkpoint 1
```

Bir commit:

- kendiliğinden **geçeni** uygular; `--approve` ile **onay bekleyeni**
  de; **reddedileni** asla;
- çalışma klasörü bu arada değiştiyse reddeder, örneğin ajanın da
  değiştirdiği bir dosyayı siz düzenlediyseniz:

  ```text
  kule: the workspace changed since Kule last saw it, at: hello.txt (both changed lines 1–3)
  hint: roll back, or rerun the task on the current workspace
  ```

  ([Sync](sync-and-merging.md) önce sizin düzenlemelerinizi kopyaya
  alabilir);
- her dosyayı bir yeniden adlandırma (rename) üzerinden yazar, böylece kimse
  yarım bir dosya görmez;
- `.gitignore`'un dışladığı yolları, `--include-ignored` eklemedikçe
  kopyada bırakır;
- `.env` dosyalarında maskelenmiş gizli değerlerin gerçek değerlerini geri
  koyar (bkz. [Gizli değerler](secrets.md));
- bir **kontrol noktası** (checkpoint) saklar: commit'in yerine koyduğu
  içerik.

Uygulamada: **Commit** (Mod+Shift+Enter), ya da bir şey sizi bekliyorsa
**Approve and commit**.

## Rollback: değişiklikleri atmak

```sh
kule rollback TASK
```

```text
rolled back 2 changes
```

Kopya yeniden çalışma klasörüyle aynı olur. Uygulamada: **Roll back**
(Mod+Shift+Backspace, iki kez basın).

## Undo: bir commit'i geri almak

```sh
kule undo TASK
```

```text
undid checkpoint 1 (3 changes)
```

Undo en yeni commit'i hem çalışma klasöründe hem kopyada geri alır. Daha
geriye gitmek için yeniden çalıştırın. Bir commit'in yerine koyduğu içerik
görevin kendi Lonca deposunda saklanır (`checkpoints/store.git` altında git
nesneleri, yanlarında dosya modları ve zamanları); bu yüzden undo, git
deposu olmayan bir klasörde bile silinen dosyaları geri getirir.

Uygulamada: Timeline → **Undo** (Mod+Shift+U, iki kez basın).

## Adımlar: her araç çağrısı

Bir ajan çalışırken, bir şeyi değiştiren her araç çağrısı bir **adım**
(step) olur. `kule steps TASK` bunları listeler; uygulama Live ve Timeline'da
gösterir. Uygulamada şunları yapabilirsiniz:

- **Go back here**: kopyayı bir adımdan sonraki hâline geri getirir. Geri
  gitmenin kendisi de bir adımdır, yani onu da geri alabilirsiniz.
- **Fork from here**: kopyanın o adımdan sonraki hâlinden yeni bir görev
  başlatır.

Bir commit adımları sıfırlar. Geri dönülemeyen bir adım (64 MB'tan büyük
bir dosya, tek seferde 1 GB'tan fazlası, ikinci bir adı olan bir dosya)
nedenini söyler. `.git` geri getirilmez. Bkz. [Timeline](../app/timeline.md).

## İmzalı geçmiş

Her commit ve undo, **imzalı ve hash zincirli bir geçmişte** bir kayıttır.
İmzalama anahtarı Kule'nin ev dizininde durur ve her sandbox'tan
gizlenir.

```sh
kule history TASK
```

```text
  0. commit 1 · 3 changes · snapshot 0d0d2c26745d
history verified: 1 entries, signed
```

`kule history` her kaydı denetler ve içinde herhangi bir şey değiştirilmişse
başarısız olur.

## İlgili sayfalar

- [`kule status`](../cli/status.md), [`kule steps`](../cli/steps.md), [`kule commit`](../cli/commit.md), [`kule rollback`](../cli/rollback.md), [`kule undo`](../cli/undo.md), [`kule history`](../cli/history.md)
- [Review](../app/review.md), [Timeline](../app/timeline.md)
- [Sync ve birleştirme](sync-and-merging.md)

---
title: Git depoları
sidebar_label: Git depoları
description: Kule .git'i düz dosyalar olarak değil, anlamına göre değerlendirir - nesneler, referanslar, index, config ve hook'lar.
---

# Git depoları

Kule git'e ihtiyaç duymaz; ama bir çalışma klasörü git deposuysa, Kule
`.git`'i **dosyalar olarak değil, bir depo olarak** okur. Yoksa her
`git status` bir değişiklik gibi görünürdü: `.git/index`'i yeniden yazar,
her ref güncellemesi log'lara ekleme yapar, git bakımı da arka planda kilit
dosyaları ekleyip siler.

## Bir ajanın git işi nasıl görünür

Kopyada `main.rs`'i düzenleyip commit eden bir ajan:

```text
1 file changes: +0 ~1 -0
  ~ main.rs
      ~ function main
      + function check
git: main 86eba66 → 82180a4 · fast-forward · 1 commit
git: 3 new object files · index changed
```

`kule commit` ile dal, politikadan ve sizin incelemenizden sonra gerçek
deponuzda da ilerler.

## Her parça nasıl değerlendirilir

| Parça | Kule nasıl değerlendirir |
| --- | --- |
| **Nesneler** | Yalnızca eklenir. Undo onları `git gc`'ye bırakır. |
| **Referanslar** (dallar, etiketler) | Değerlerine göre denetlenir. Ajanın ilerlettiği dalda bir insanın yaptığı commit bir çakışmadır; `git pack-refs` ya da dal değiştirip geri dönmek çakışma değildir. |
| **Index** | stat verisiyle değil, kayıtlarıyla karşılaştırılır; bu yüzden çalışma klasöründeki `git status` ne commit'i ne undo'yu engeller. |
| **Stage'lenmiş dosyalar** | Görevin stage'lediği, dosyalarının tuttuğuyla aynı olmalıdır. Başka bir içerikle stage'lenmiş bir kayıt (ya da dosyası dururken index'ten çıkarılmış bir kayıt) sizi bekler, çünkü bir sonraki `git commit` kimsenin incelemediği bir şeyi kaydederdi. |
| **Config ve hook'lar** | Kendi satırlarında bildirilir, çünkü bir sonraki `git` komutunun kod çalıştırmasına yol açarlar. |

```text
git: config changed (it can make the next git command run code)
git: hook pre-commit changed (it runs on the next matching git command)
```

## Politika git hakkında ne der

[Balanced politika](policy.md) ile:

- bir dalı **ileri** taşımak (fast-forward) ve **yeni bir dal** oluşturmak
  kendiliğinden geçer;
- geçmişin yeniden yazılması, bir dalı silmek, `HEAD`'i değiştirmek,
  etiketler, notlar ve replacement'lar **sizi bekler**;
- git config değişiklikleri **sizi bekler**;
- **git hook'ları reddedilir**, iç içe bir deponun hook'ları da: bir sonraki
  git komutunuzda sizin kimliğinizle çalışırlar.

Git sunucuları paket kayıt deposu değildir: `github.com`'a bir bağlantı
sizi bekler, çünkü TLS içinde bir push bir fetch'ten ayırt edilemez.

## Git config ve hook'lar terminalinizi de korur

Bir görev kopyanın git config'ini ya da hook'larını değiştirmişken Kule,
kopyada **bir terminal açmayı** (ve `kule shell`'i) reddeder ve kopyayı bir
editörde açmayı da reddeder: isteminizdeki (prompt) `git status` bunları
sandbox'ın dışında çalıştırırdı. `kule path` stderr'e bir uyarı yazar. Önce
inceleyip commit edin ya da değişiklikleri atın (roll back).

## Worktree'ler

Çalışma klasörünün yanındaki bir worktree deposu görev için okunabilir
kalır, böylece git kopyada klasördeki gibi çalışır.

## İlgili sayfalar

- [İnceleme, commit ve geri alma](review-commit-undo.md)
- [Sync ve birleştirme](sync-and-merging.md): klasördeki commit'ler ve dallar sync ile gelir
- [`kule status`](../cli/status.md), [`kule shell`](../cli/shell.md), [`kule path`](../cli/path.md)

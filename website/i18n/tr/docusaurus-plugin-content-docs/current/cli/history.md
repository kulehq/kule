---
title: kule history
sidebar_label: kule history
description: Bir görevin imzalı, hash zinciriyle bağlı commit ve undo geçmişini, kayıt kayıt doğrulayarak gösterir.
---

# kule history

Görevin imzalı geçmişini (her commit ve undo) kayıt kayıt doğrulayarak
gösterir.

## Kullanım

```sh
kule history [OPTIONS] <TASK>
```

## Açıklama

Her [kule commit](commit.md) ve [kule undo](undo.md), görevin geçmişine bir
kayıt ekler. Her kayıt kendinden öncekine **hash zinciriyle** bağlıdır ve
Kule'nin ev klasöründe (`kule.key`) duran, her sandbox'tan gizlenmiş bir
anahtarla **imzalanır**. Bir commit'in kaydı, kontrol noktasını, kaç
değişiklik yaptığını ve yazdıklarının bir anlık görüntü (snapshot) hash'ini
tutar.

`kule history` zinciri okur, her imzayı ve bağlantıyı denetler ve kayıtları
en eskisinden başlayarak yazdırır. İçindeki bir şey imzalandıktan sonra
değiştirildiyse (bir kayıt düzenlendiyse, silindiyse ya da sırası
değiştiyse), denetim başarısız olur ve komut güvenemeyeceğiniz bir geçmiş
yazdırmak yerine bir hatayla çıkar.

Bir görevin klasörünüze ne yaptığını görmek ve bu kaydı hiçbir şeyin yeniden
yazmadığından emin olmak için kullanın.

Yalnızca okur; hiçbir şeyi değiştirmez.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve ad vermek](index.md). |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (ya da `KULE_HOME`). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Dört commit'li bir görev:**

```text
$ kule history 3422f830
  0. commit 1 · 9 changes · snapshot 1009aa9e9ec6
  1. commit 2 · 1 changes · snapshot ab68d01690e5
  2. commit 3 · 2 changes · snapshot 919dfa44867d
  3. commit 4 · 1 changes · snapshot 0f6b3afc0dfb
history verified: 4 entries, signed
```

**Bir commit ve ardından onun geri alınması:**

```text
$ kule history 3efe1ca8
  0. commit 1 · 3 changes · snapshot 7671876a1160
  1. undo 1
history verified: 2 entries, signed
```

**Bir betikte denetleyin:**

```sh
kule history "$TASK" > /dev/null || echo "the history of $TASK does not verify"
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `N. commit C · M changes · snapshot H` | `N` numaralı kayıt: `C` kontrol noktasını saklayan, `M` yol yazan bir commit; snapshot hash'inin ilk 12 karakteriyle. |
| `N. undo C` | `N` numaralı kayıt: `C` kontrol noktası geri alındı. |
| `history verified: N entries, signed` | Her kayıt doğrulandı. |

Commit'i olmayan bir görev yalnızca `history verified: 0 entries, signed`
yazdırır.

## Hatalar ve çözümleri

Denetim başarısız olursa `kule history` nedeni `kule:` sonrasında yazdırır
ve `2` koduyla çıkar. O görevin kaydına güvenmeyin. Çalışma klasörünü
beklediğinizle karşılaştırın ve Kule'nin dosyalarını kendiniz
değiştirmediyseniz bunu bildirin (bkz. [Güvenlik](../security.md)).

## Uygulamada

**Timeline** (Mod+3) commit'leri en yenisi üstte listeler; en yenisinde Undo
vardır. Bkz. [Timeline](../app/timeline.md).

## Ayrıca bakın

- [kule commit](commit.md), [kule undo](undo.md)
- [İnceleme, commit ve geri alma](../concepts/review-commit-undo.md)

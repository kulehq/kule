---
title: kule steps
sidebar_label: kule steps
description: Bir görevin commit edilmemiş işinin adımlarını, her ajan araç çağrısı için bir tane olmak üzere, değiştirdikleri dosyalarla listeler.
---

# kule steps

Görevin commit edilmemiş işinin adımlarını listeler: her araç çağrısının ne
değiştirdiğini.

## Kullanım

```sh
kule steps [OPTIONS] <TASK>
```

## Açıklama

Bir ajan çalışırken (bkz. [kule run](run.md)), yaptığı her araç çağrısı
(araç çağrısı: bir dosyayı düzenlemek ya da bir komut çalıştırmak gibi) bir
**adım** olur. Kule her adımdan önce dosyaların ne tuttuğunu saklar; böylece
ajanın işini parça parça görebilir, uygulamada da herhangi bir adıma geri
dönebilir ya da oradan yeni bir görev ayırabilirsiniz (fork).

`kule steps`, son commit'ten bu yana olan adımları en eskisinden başlayarak,
her birinin değiştirdiği dosyalarla listeler. [kule status](status.md)
hepsinin toplamını gösterir.

Adımlar bir commit, bir rollback, bir undo ya da bir [sync](sync.md)
sonrasında baştan başlar. `kule exec` ile ya da `kule shell` içinde
çalıştırdığınız komutlar adım değildir; yine de değişiklikleri
`kule status`'ta görünür.

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

**Bir ajan çalıştırmasından sonra** (başlıklar ajanın kendisinindir):

```text
$ kule steps 3422f830
  1. Edit src/main.rs
       ~ src/main.rs
  2. Write tests/greet.rs
       + tests/greet.rs
  3. cargo test
       + target/debug/app
       ~ Cargo.lock
```

**Uygulamada bir adıma geri döndükten sonra:** geri dönüşün kendisi de ayrı
bir adımdır ve geri koyduğu dosyalar `↺` ile işaretlenir.

**Son commit'ten beri hiçbir ajan çalışmadıysa:** `kule steps` hiçbir şey
yazdırmaz.

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `N. <title>` | Adımın numarası ve araç çağrısının, ajanın verdiği başlığı. |
| `+ path`, `~ path`, `- path` | Adımın oluşturduğu, değiştirdiği ya da sildiği bir dosya. |
| `↺ path` | Bir adıma geri dönülerek eski hâline getirilmiş bir dosya. |

## Uygulamada

**Timeline** paneli (Mod+3) adımları listeler; her birinde *Go back here*
(buraya geri dön) ve *Fork from here* (buradan ayır) vardır. Live her adımı
olduğu anda gösterir. Bkz. [Timeline](../app/timeline.md).

## Ayrıca bakın

- [kule run](run.md), [kule status](status.md)
- [Timeline](../app/timeline.md)

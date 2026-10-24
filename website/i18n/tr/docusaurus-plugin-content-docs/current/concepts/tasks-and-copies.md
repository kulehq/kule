---
title: Görevler ve kopyalar
sidebar_label: Görevler ve kopyalar
description: Görevin ne olduğu, copy-on-write kopyasının nasıl oluşturulduğu, diskte ne kadar yer tuttuğu ve görevlerin nasıl adlandırılıp sıralandığı, çatallandığı ve silindiği.
---

# Görevler ve kopyalar

## Görev nedir

**Görev** (task), tek bir klasör üzerindeki tek bir iştir. Bir görev
açtığınızda Kule:

1. ona `1a2b3c4d` gibi 8 onaltılık (hex) karakterlik bir kimlik verir;
2. klasörün (**çalışma klasörünün**) bir **kopyasını** Kule'nin ev
   klasörü altında oluşturur;
3. kopyanın yanında görevin ihtiyaç duyduğu her şeyi tutar: geçici
   dosyalar, paket önbellekleri, bir ev katmanı, adımlar ve kontrol
   noktaları.

Ajan yalnızca kopyada çalışır. Çalışma klasörü, siz commit,
[sync](sync-and-merging.md) ya da undo yapana kadar yalnızca okunur.

```sh
kule new ~/code/app
```

```text
task 1a2b3c4d · copy at ~/.kule/tasks/1a2b3c4d/w/app
```

Uygulamada: **New task…** (Mod+N) ve bir klasör seçin.

Kule'nin **git'e ihtiyacı yoktur**. Her klasör olur: kod, bir Markdown ya
da Obsidian kasası (vault), fotoğraflar, indirilenler, CSV dosyaları, henüz
güvenmediğiniz bir kurulum programı. Klasör bir git deposuysa Kule ayrıca
`.git`'i de anlar (bkz. [Git depoları](git.md)).

## Kopya nasıl oluşturulur

| Nerede | Nasıl | Disk alanı |
| --- | --- | --- |
| macOS (APFS) | Klasörün tek bir APFS klonu | Dosyalar değişene kadar neredeyse hiç |
| Linux, reflink destekli Btrfs ya da XFS | Dosya dosya, reflink ile (`FICLONE`) | Yalnızca değişenler |
| Linux, ext4 ve reflink'i olmayan diğerleri | Düz bir kopya | Klasörün tam boyutu, `.git` ve `node_modules` dahil |

:::note Disk araçları klonları tam boyutuyla sayar

Finder, `du` ve disk kullanım araçları her klonu tam boyutuyla sayar; bu
yüzden `~/.kule` gigabaytlarca yer kaplıyormuş gibi görünebilir. Gerçekte ne
kadar kullanıldığını görmek için **boş alana** bakın (`df -h` ya da System
Settings → General → Storage). Btrfs'te `btrfs filesystem du` doğruyu
söyler.

:::

Klonlar yalnızca aynı birimde oluşturulur. Başka bir diskteki bir klasör
tamamen kopyalanır.

## Görev neleri tutar

Her şey `~/.kule/tasks/<id>/` altındadır:

| Klasör | Nedir |
| --- | --- |
| `w/<folder name>` | Ajanın çalıştığı kopya. |
| `base/` | Klasörün son eşitlemedeki (sync) hâli; birleştirmek için kullanılır. |
| `tmp/` | Görevin geçici dosyaları. `tmp/caches/`, görevin kendi paket önbelleklerini (npm, cargo, pip, Go ve diğerleri) tutar. |
| `home/` | [Ev katmanı](home-layer.md): görevin komutlarının ev dizininiz olarak gördüğü yer. |
| `steps-store/`, `checkpoints/` | Bir adıma geri dönmek ve undo için gerekenler. |

macOS'ta görevin npm ve cargo önbellekleri sizinkilerin klonu olarak
başlar; böylece bir görevin ilk kurulumu yeniden indirmez. Linux'ta boş
başlarlar. Büyük önbelleklerin klonu birkaç saniye sürdüğü için uygulama,
bir görev oluşturulur ya da fork edilir edilmez klonu arka planda yapar.
Klon bitmeden gelen bir komut, terminal ya da çalıştırma onu bekler ve
bunu söyler: *Kule is still preparing this task's package caches*. Komut
satırında görevin ilk komutu önbellekleri kendisi hazırlar ve *preparing
this task's package caches* der.

Gerçekten yer kaplayanlar: ajanın değiştirdiği ya da oluşturduğu dosyalar
(derleme çıktıları da), bir görevin indirdiği paketler ve adımlarla kontrol
noktalarının sakladığı eski içerikler.

## Yok sayılan yollar

Çalışma klasörünün `.gitignore`'unun dışarıda bıraktığı yollar (derleme
çıktıları, önbellekler, `.env`) kopyada kalır. Siz istemedikçe bir commit
onları geride bırakır: `kule commit --include-ignored` ya da uygulamada
*Also commit these*. Yok sayma kuralları kopyadan değil, her zaman çalışma
klasöründen gelir.

## Birden çok görev

Bir görevde aynı anda tek bir ajan çalışır. Ajanları yan yana çalıştırmak
için, aynı klasör ya da başka klasörler üzerinde daha fazla görev açın.

Aynı klasör üzerindeki görevler birbirlerinin işini asla görmez. Kule, ikisi
de hâlâ sürerken size **nerede kesiştiklerini** söyler: aynı fonksiyon,
bölüm ya da anahtar; içine bakamadığında da aynı dosya. `kule status` bunu
listeler (`task 5e6f7a8b changes them too: task.rs · method Task::commit`),
uygulama da görevi işaretleyip Review altında gösterir. İkisi de commit
ettiğinde ne olduğu için [Eşitleme ve birleştirme](sync-and-merging.md)
sayfasına bakın.

## Görevleri düzenleme

| Ne | Komut satırı | Uygulama |
| --- | --- | --- |
| Görevleri uygulamadaki sırasıyla listelemek | [`kule list`](../cli/list.md) | Tasks paneli |
| Bir göreve ad vermek (`Downloads – march invoices`) | [`kule name`](../cli/name.md) | ⋮ menüsü → *Rename…* (F2) |
| Bir görevi renklendirmek | [`kule color`](../cli/color.md) | ⋮ menüsü → renk örnekleri |
| Bir görevi listede taşımak | – | Sürükleyin ya da Alt+Shift+↑/↓ |
| Bir görevi bir gruba koymak | [`kule group put`](../cli/group.md) | Bir grubun üzerine sürükleyin ya da *Group…* |
| Bir görevi çatallamak (fork) | – | ⋮ menüsü → *Fork task* (Mod+Shift+D) ya da Timeline → *Fork from here* |
| Bir görevi silmek | [`kule rm`](../cli/rm.md) | ⋮ menüsü → *Delete* |

**Çatallamak** (fork), aynı klasör üzerinde, bu görevin kopyasından (şu anki
hâlinden ya da bir adımdan sonraki hâlinden) başlayan yeni bir görev
oluşturur. Çatal, görevin adını "(fork)" ekiyle, rengini, grubunu,
bütçesini ve gizli değer yer tutucularını alır; kendine ait hiçbir adımı ya
da commit'i olmaz. Bunu, mevcut yönü kaybetmeden başka bir yön denemek için
kullanın.

**Silmek** klasörünüze asla dokunmaz. Görev `~/.kule/trash`'e taşınır;
Kule orayı arka planda boşaltır ve görevin kapladığı yer geri gelir.

## İlgili

- [`kule new`](../cli/new.md), [`kule list`](../cli/list.md), [`kule rm`](../cli/rm.md), [`kule path`](../cli/path.md)
- [Pencere](../app/window.md)
- [Gruplar](groups.md)

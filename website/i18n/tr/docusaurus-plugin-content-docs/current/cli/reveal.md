---
title: kule reveal
sidebar_label: kule reveal
description: Bir klasörün görevlerinin, yanındaki bir projeyi (örneğin derlemede kullandığı ortak bir kütüphaneyi) okumasına izin verin.
---

# kule reveal

Bir klasörün görevlerinin, yanında duran bir projeyi (örneğin derlemede
kullandığı ortak bir kütüphaneyi) **okumasına** izin verir. Görevler o
projeye asla yazamaz.

## Kullanım {#synopsis}

```sh
kule reveal [OPTIONS] [COMMAND]
```

| Alt komut | Ne yapar |
| --- | --- |
| [`list`](#kule-reveal-list) | Açılan projeleri ve klasörün derlemede kullandığı ama görevlerinin okuyamadığı projeleri listeler (varsayılan). |
| [`add`](#kule-reveal-add) | Klasörün görevlerinin yanındaki bir projeyi okumasına izin verir. |
| [`remove`](#kule-reveal-remove) | Projeyi yeniden gizler. |

## Açıklama {#description}

[Sandbox](../concepts/sandbox.md), bir görevin komutlarından kendi
klasörlerinizi gizler: Documents, Desktop, Downloads ve benzerleri, ayrıca
**çalışma klasörünüzün bulunduğu klasörü** ve oradaki diğer projeleri. Yani
çalışma klasörü `~/code/app` olduğunda `~/code/shared` de gizlidir.

Bu da yanındaki bir projeyi okuyan bir derlemeyi bozar:

- `Cargo.toml`'da `path = "../shared"`,
- `package.json`'da `"file:../ui"`,
- bir .NET projesinde `..\Shared\Shared.csproj`.

`kule reveal add` böyle bir projeyi klasörün görevlerine **salt okunur**
olarak açar. Kule ayrıca neyin açılacağını **önerir**: klasörün kendi
manifest dosyalarını okur (`Cargo.toml`, `package.json`, `go.mod`, `go.work`,
`pyproject.toml`, Gradle ayarları, `.csproj`/`.fsproj`/`.vbproj`/`.sln`/`.slnx`,
`Directory.*.props`) ve bunların gösterdiği projeleri adlandırır. Yalnızca
önerir; bir projeyi yalnızca siz açarsınız.

Bunu güvenli tutan kurallar:

- Liste, hiçbir sandbox'ın erişemediği Kule'nin evinde durur. Öneriler bir
  görevin kopyasından değil, her zaman gerçek klasörünüzden gelir.
- Açılan bir proje okunur, asla yazılmaz. Kule'nin onun içinde gizlediği
  şeyler (örneğin kimlik bilgisi klasörleri) gizli kalır.
- Her çalıştırmadan önce yeniden kontrol edilir. O arada bir bağlantıya
  (link) dönüşmüş ya da artık gizli klasörlerinizden birinde olmayan bir
  proje açılmaz.
- Kule, görevin kopyasının yanına açtığınız projelere giden bağlantılar
  koyar; böylece kopyadan `../shared`, klasörden gittiği yere gider.

Bir yolu yalnızca tek bir komut için açmak için
[`kule exec --reveal PATH`](exec.md) kullanın.

Tüm alt komutlar `--home` alır (bkz. [genel seçenekler](index.md)).

## kule reveal list {#kule-reveal-list}

```sh
kule reveal list [OPTIONS]
```

| Seçenek | Varsayılan | Anlamı |
| --- | --- | --- |
| `--folder FOLDER` | geçerli dizin | Önerileri gösterilecek klasör. |

```console
$ cd ~/code/app
$ kule reveal
no project beside a folder is open to its tasks
builds from ~/code/shared, which its tasks cannot read: kule reveal add '~/code/shared'
```

Açtıktan sonra:

```console
$ kule reveal
  1  ~/code/shared  · for ~/code/app
```

Saklanmış ama şu anda açılamayan bir proje nedenini gösterir; örneğin
`(not opened: …: a link; name the folder it leads to)`.

`kule new` de bir görev oluştururken aynı önerileri yazar.

## kule reveal add {#kule-reveal-add}

```sh
kule reveal add [OPTIONS] <PATH>
```

| Argüman / seçenek | Varsayılan | Anlamı |
| --- | --- | --- |
| `PATH` | | Proje klasörü; geçerli dizine göre ya da mutlak yol. |
| `--folder FOLDER` | geçerli dizin | Görevleri projeyi okuyabilecek çalışma klasörü. |

```console
$ cd ~/code/app
$ kule reveal add ../shared
kept as 1: tasks in ~/code/app read ~/code/shared, never write it
```

Bundan sonra `~/code/app` üzerindeki bir görev `../shared`'ı okuyabilir;
oraya yazmak reddedilir:

```console
$ kule exec 1046fb30 -- touch ../shared/x
touch: ../shared/x: Operation not permitted
no changes
sandbox: refused file-write-create ~/code/shared/x (touch)
```

## kule reveal remove {#kule-reveal-remove}

```sh
kule reveal remove [OPTIONS] <ID>
```

```console
$ kule reveal remove 1
closed 1: hidden again from the next command on
$ kule reveal remove 1
nothing open as 1
```

## Hatalar ve çözümler {#errors-and-fixes}

`kule reveal add`, çalışma klasörünün yanında bir proje olmayan bir yolu
reddeder. Mesaj yolla başlar ve nedenini söyler:

| Neden | Ne anlama gelir |
| --- | --- |
| `not found` / `not a folder` | Var olan bir dizin verin. |
| `a link; name the folder it leads to` | Sembolik bağlantıyı değil, gerçek klasörü verin. |
| `the workspace itself, or a folder holding it` | Yalnızca çalışma klasörünün *yanındaki* bir proje açılabilir. |
| `not in a folder Kule hides as yours (the workspace's own folder, Documents, …); the tasks read it already, or Kule keeps it hidden for good` | Yol ya görevler tarafından zaten okunabiliyor (yapacak bir şey yok) ya da Kule'nin hiçbir zaman açmadığı bir yerde, örneğin bir kimlik bilgisi klasöründe. |

## Uygulamada {#in-the-app}

*Settings → Rules* açılan projeleri listeler; görev başlığındaki **builds
from** çipi, klasörün manifest dosyalarının adlandırdığı projeleri önerir.

## Ayrıca bakın {#see-also}

- [`kule exec`](exec.md): tek bir komut için `--reveal PATH`.
- [Sandbox](../concepts/sandbox.md)

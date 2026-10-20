---
title: kule open
sidebar_label: kule open
description: Kule açılmasının güvenli olduğunu kontrol ettikten sonra, görevin kopyasındaki bir belgeyi, resmi, ses ya da video dosyasını alışılmış uygulamasıyla açar.
---

# kule open

Görevin kopyasındaki bir dosyayı kendi uygulamasıyla açar: yalnızca
belgeler, resimler, ses ve video.

## Kullanım

```sh
kule open [OPTIONS] <TASK> <FILE>
```

## Açıklama

Kopyadaki dosyaları ajan yazdı; bu yüzden Kule onlara güvenmez. Bir dosyayı
kendi uygulamasıyla açmak sandbox'ın **dışında**, sizin bilgisayarınızda
olur. Bir program, bir betik, bir web sayfası ya da makrolu bir Office
dosyası tüm erişiminizle çalışırdı. Bu yüzden `kule open` yalnızca
açılarak kod çalıştıramayan dosyaları açar ve her birini önce kontrol eder:

- **Türü Kule'nin listesindedir:** PDF, Word, Excel, PowerPoint,
  OpenDocument, Pages, Numbers, Keynote, EPUB, RTF, CSV, düz metin ve
  yaygın resim, ses ve video türleri. Asla bir program, betik, web sayfası
  ya da SVG, bir bağlantı (link) dosyası ya da makrolu bir Office dosyası
  değil.
- **Adının söylediği şeydir:** Kule dosyanın ilk baytlarını okur. Adı
  `notes.pdf` olan bir betik reddedilir.
- **Bir bağlantı üzerinden değil,** kopyanın içinden ulaşılır ve diskte
  yalnızca bir adı vardır.
- **Görevde çalışan bir ajan yoktur.** Kule kopyanın sakinleşmesini bekler;
  böylece dosya, kontrol ile açılış arasında değişemez.

Dosya macOS'ta `open` ile, Linux'ta `xdg-open` ile açılır.

`--dry-run` tüm kontrolleri yapar ve dosyayı açmadan nerede olduğunu
yazdırır.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |
| `FILE` | evet | Kopyanın içinde, kopyanın en üstüne göre bir dosya, örneğin `reports/q3.pdf`. |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--dry-run` | | | Dosyayı kontrol eder ve açmadan nerede olduğunu yazdırır. |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Ajanın oluşturduğu bir PDF'i açmak:**

```sh
kule open 3422f830 report.pdf
```

**Önce kontrol etmek:**

```text
$ kule open 3422f830 report.pdf --dry-run
/Users/me/.kule/tasks/3422f830/w/app/report.pdf
```

**Bir betik reddedilir:**

```text
$ kule open 3422f830 run.sh
kule: run.sh: Kule opens documents, pictures, sound and video with their app, not this kind of file; it would run outside the sandbox
```

**Adının söylediği şey olmayan bir dosya reddedilir:**

```text
$ kule open 3422f830 fake.pdf
kule: fake.pdf: it is not the .pdf file its name says, so Kule does not open it
```

## Çıktı

Dosya açıldığında hiçbir şey. `--dry-run` ile dosyanın mutlak yolu.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: <file>: Kule opens documents, pictures, sound and video with their app, not this kind of file; …` | Bilinçli bir tasarım. Dosyaya bir metin görüntüleyicide ya da sandbox içinde [kule exec](exec.md) ile bakın. |
| `kule: <file>: it is not the .<ext> file its name says, so Kule does not open it` | İçerik adla uyuşmuyor. Dosyaya şüpheyle yaklaşın. |
| `kule: <file>: a path inside the copy, without ..` | Kopyanın en üstüne göre bir yol kullanın. |
| `kule: an agent is at work in task <id> (process <pid>); wait until it is done` | Çalıştırmanın bitmesini bekleyin. |

## Uygulamada

**Files**'ta bir dosyaya sağ tıklayıp *Open with its app*'i seçin
(macOS'ta ⌘O, Linux'ta Ctrl+O) ya da Preview'un kendisi gösteremediği
dosyalar için sunduğu düğmeyi kullanın. Bkz.
[Files ve Preview](../app/files-and-preview.md).

## Ayrıca bakın

- [kule path](path.md)
- [Güvenlik](../security.md)

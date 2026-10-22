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
kendi uygulamasıyla açmak sandbox'ın ve proxy'nin **dışında**, sizin
bilgisayarınızda olur. Bir program, bir betik, bir web sayfası ya da makrolu
bir Office dosyası tüm erişiminizle çalışırdı; başka bir yerden şablon,
resim ya da veri yükleyen bir belge de Kule'nin politikasını aşarak ağa
ulaşırdı. Bu yüzden `kule open` yalnızca açılınca bunların hiçbirini
yapmayan dosyaları açar ve her birini önce kontrol eder:

- **Türü Kule'nin listesindedir:** PDF, Word, Excel, PowerPoint,
  OpenDocument (metin, hesap tablosu, sunum), CSV, düz metin ve yaygın
  resim, ses ve video türleri. Asla bir program, betik, web sayfası ya da
  SVG, bir bağlantı (link) dosyası ya da makrolu bir Office dosyası değil.
- **Adının söylediği şeydir:** Kule dosyanın ilk baytlarını okur. Adı
  `notes.pdf` olan bir betik reddedilir.
- **Kendi içinde kalır:** Kule Word, Excel, PowerPoint, OpenDocument ve CSV
  dosyalarının içine bakar. Şunları içereni reddeder: makro, denetim
  (control) ya da gömülü nesne; tıklayacağınız bir web sayfası ya da e-posta
  adresi dışında dosyanın dışındaki bir şeye bağlantı (şablon, resim, başka
  bir çalışma kitabı, veri bağlantısı); bir şey getiren, içeri alan ya da
  çalıştıran bir Word alanı; ağa ya da başka bir programa ulaşan bir formül;
  ve bir hesap tablosu uygulamasının formül olarak okuyacağı bir CSV hücresi.
- **Bir bağlantı üzerinden değil,** kopyanın içinden ulaşılır ve diskte
  yalnızca bir adı vardır.
- **Görevde çalışan bir ajan yoktur.**

Açılan, dosyanın **Kule'ye ait bir kopyasıdır**: kontrolden önce,
`~/.kule/opened` içinde, yalnızca sizin okuyabileceğiniz şekilde yapılır.
Ajanın sonradan kendi dosyasına yaptığı hiçbir şey uygulamanın okuduğu şey
olmaz. Kopya, dosyanın genişletilmiş özniteliklerinin (extended attributes)
hiçbirini taşımaz ve macOS'ta web'den gelen bir dosya gibi "indirilmiş"
olarak işaretlenir. Uygulamada yaptığınız değişiklikler görevin değil, bu
kopyanın içinde kalır: kopyalar açıldıktan bir gün sonra silinir.

Kopya macOS'ta `open` ile, Linux'ta `xdg-open` ile açılır.

`--dry-run` tüm kontrolleri yapar, kopyayı oluşturur ve açmadan nerede
olduğunu yazdırır.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `TASK` | evet | Görevin tam kimliği. Bkz. [Bir göreve atıf yapmak](index.md). |
| `FILE` | evet | Kopyanın içinde, kopyanın en üstüne göre bir dosya, örneğin `reports/q3.pdf`. |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--dry-run` | | | Dosyayı kontrol eder ve açmadan Kule'nin kopyasının nerede olduğunu yazdırır. |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Ajanın oluşturduğu bir PDF'i açmak:**

```sh
kule open 3422f830 report.pdf
```

**Önce kontrol etmek:**

```text
$ kule open 3422f830 report.pdf --dry-run
/Users/me/.kule/opened/18e0e1b1ae198f08-54035-0/report.pdf
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

Dosya açıldığında hiçbir şey. `--dry-run` ile Kule'nin kopyasının mutlak yolu.

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: <file>: Kule opens documents, pictures, sound and video with their app, not this kind of file; …` | Bilinçli bir tasarım. Dosyaya bir metin görüntüleyicide ya da sandbox içinde [kule exec](exec.md) ile bakın. |
| `kule: <file>: it is not the .<ext> file its name says, so Kule does not open it` | İçerik adla uyuşmuyor. Dosyaya şüpheyle yaklaşın. |
| `kule: <file>: Kule does not open it with its app: …` | Dosya bir şey çalıştırır ya da kendi dışına ulaşırdı; mesajın devamı neyi olduğunu söyler. Dosyaya Preview'da ya da sandbox içinde bakın. |
| `kule: <file>: too large for Kule to look inside, …` | 20 MB'tan büyük Word, Excel, PowerPoint, OpenDocument ve CSV dosyaları açılmaz. |
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

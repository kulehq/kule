---
title: kule new
sidebar_label: kule new
description: Ajanların ve komutların içinde çalıştığı, bir klasörün copy-on-write kopyası olan bir görev açar.
---

# kule new

Bir görev açar: ajanların ve komutların gerçek klasöre dokunmadan içinde
çalıştığı, bir klasörün copy-on-write (yazınca kopyala) kopyası.

## Kullanım

```sh
kule new [OPTIONS] <WORKSPACE>
```

## Açıklama

**Görev**, Kule'nin iş birimidir. `kule new` bir klasör (**çalışma
klasörü**, workspace) alır ve onun bir kopyasını Kule'nin ev klasöründe,
`<home>/tasks/<id>/w/<klasör adı>` altında oluşturur. Görevle yaptığınız her
şey bu kopyada olur: `kule run` ve `kule exec` onu değiştirir, `kule status`
onu çalışma klasörüyle karşılaştırır ve değişiklikleri geri yazan yalnızca
`kule commit`'tir.

Dosya sistemi destekliyorsa kopya **reflink** ile yapılır: macOS'ta APFS
klonları, Linux'ta Btrfs ve XFS üzerinde `FICLONE`. Reflink ile yapılan bir
kopya disk bloklarını orijinaliyle paylaşır; bu yüzden büyük bir klasörde
bile hızlıdır ve dosyalar değişene kadar yer kaplamaz. Reflink desteği
olmayan bir dosya sisteminde (örneğin ext4) Kule düz bir kopyaya geri
döner; bu kopya klasörün tam boyutu kadar yer kaplar. Bkz.
[Görevler ve kopyalar](../concepts/tasks-and-copies.md).

Klasör her şey olabilir: bir git deposu, bir not kasası, bir fotoğraf ya da
fatura klasörü. Kule'nin git'e ihtiyacı yoktur. Klasör bir git deposuysa
Kule onun `.git`'ini de anlar (bkz. [Git](../concepts/git.md)).

Görev açılırken Kule ayrıca:

- kopyadaki `.env` dosyalarında gizli değere benzeyen değerleri maskeler
  (bkz. [Gizli değerler](../concepts/secrets.md));
- klasörün manifest dosyalarına (`Cargo.toml`, `package.json`, `.csproj`,
  …) bakarak, yanında duran ve derlemede kullandığı projeleri (örneğin
  `../shared`) arar. Kule bir çalışma klasörünün yanındaki projeleri
  sandbox'tan gizler; bu yüzden her birini size söyler ve
  [`kule reveal add`](reveal.md) ile salt okunur olarak nasıl açacağınızı
  gösterir.

Aynı klasörde ya da başkalarında istediğiniz kadar görev açabilirsiniz.
Görevler birbirlerinin işini asla görmez; Kule aynı şeyleri değiştirdikleri
yerleri size söyler.

`kule new` çalışma klasörüne dokunmaz.

## Argümanlar

| Ad | Zorunlu | Anlamı |
| --- | --- | --- |
| `WORKSPACE` | evet | Kopyalanacak klasör. Göreli bir yol geçerli dizinden itibaren alınır. Bağlantılar (link) gerçek klasöre kadar izlenir. |

## Seçenekler

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin ev klasörü (`KULE_HOME` ile de verilebilir). Bkz. [genel seçenekler](index.md). |

## Örnekler

**Bir depo üzerinde görev açmak:**

```text
$ kule new ~/code/app
task 3422f830 · copy at ~/.kule/tasks/3422f830/w/app
```

**Depo olmayan bir klasörde görev açmak:**

```text
$ kule new ~/notes
task 40ac70b8 · copy at ~/.kule/tasks/40ac70b8/w/notes
```

**Kimliği bir betikte saklamak:**

```sh
TASK=$(kule new ~/code/app | awk '{print $2}')
kule exec "$TASK" -- npm test
```

**Yanındaki bir projeden derlenen bir klasör:**

```text
$ kule new ~/code/web
task 5e6f7a8b · copy at ~/.kule/tasks/5e6f7a8b/w/web
builds from /Users/me/code/shared, which its tasks cannot read: kule reveal add '/Users/me/code/shared'
```

## Çıktı

| Satır | Anlamı |
| --- | --- |
| `task <id> · copy at <path>` | Yeni görevin kimliği (diğer tüm komutlarda bunu kullanın) ve kopyasının yeri. |
| `builds from <path>, which its tasks cannot read: kule reveal add '<path>'` | Klasörün manifest dosyalarının adını verdiği, sandbox'tan gizli bir proje. Bu klasörün görevlerinin onu okuyabilmesi için gösterilen komutu çalıştırın. |

## Hatalar ve çözümleri

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: <path>: No such file or directory (os error 2)` | Klasör yok. Yolu kontrol edin. |
| `kule: <path> is not a directory` | Dosya değil, bir klasör verin. |
| `kule: Kule's home <home> is inside the workspace, so the copy would contain itself` | Kule'nin ev klasörünü içeren bir klasörün (örneğin ev dizininizin tamamının) kopyasını istediniz. İçindeki bir klasörü seçin ya da Kule'nin ev klasörünü `--home` ile başka yere taşıyın. |
| `kule: Kule's home <~/.kule> is inside the workspace, though this task's home is another` | Klasör, `--home` ya da `KULE_HOME` başka yeri gösterse bile, Kule'nin varsayılan yerindeki ev klasörünü içeriyor: görevleri ve gizli değerleri kopyaya girerdi. İçindeki bir klasörü seçin. |
| `kule: <folder> holds more than 1000000 files and folders, more than Kule reads; …` | Daha küçük bir klasör seçin. 8 GiB'den büyük bir dosya da, Kule onu bir gözden geçirme için hash'lerken aynı şekilde reddedilir. |

## Uygulamada

**New task…** düğmesine tıklayın (Mod+N) ve bir klasör seçin. Henüz görev
yoksa ilk açılış ekranı sizden bir klasör ister. Bkz.
[Uygulamada ilk göreviniz](../getting-started/first-task-in-the-app.md).

## Ayrıca bakın

- [Görevler ve kopyalar](../concepts/tasks-and-copies.md)
- [kule list](list.md), [kule rm](rm.md)
- [kule reveal](reveal.md)

---
title: CLI başvurusu
sidebar_label: Genel bakış
description: kule komutu, genel seçenekleri, bir göreve nasıl atıf yapılacağı ve tüm komutlar tek sayfada.
---

# CLI başvurusu

`kule`, Kule'nin komut satırı aracıdır. Masaüstü uygulamasıyla aynı görevler
üzerinde çalışır: `kule new` ile açtığınız görev uygulamada görünür,
uygulamada açtığınız bir görevi de `kule commit` ile commit edebilirsiniz.
Her şey Kule'nin ev klasöründe durur (başkasını seçmediyseniz `~/.kule`);
uygulama da CLI da burayı okur ve yazar.

Henüz kurmadıysanız [Kurulum](../getting-started/install.md) sayfasına
bakın. Önce fikirleri öğrenmek isterseniz
[Kule nasıl çalışır](../concepts/how-kule-works.md) sayfasını okuyun.

## Tek blokta döngü

Her komut bu döngünün bir yerine oturur:

```sh
kule new ~/code/app                     # 1. görev açın: klasörün copy-on-write kopyası
kule run 3422f830 "Fix the failing test" # 2a. kopyada bir ajan çalışsın
kule exec 3422f830 -- npm test           # 2b. ya da orada, sandbox içinde bir komutu kendiniz çalıştırın
kule status 3422f830                     # 3. neyin değiştiğini ve politikanın ne dediğini görün
kule commit 3422f830                     # 4a. gerçek klasöre uygulayın (bir kontrol noktası saklanır)
kule rollback 3422f830                   # 4b. ya da değişiklikleri atın
kule undo 3422f830                       # 5. son commit'i klasörden geri alın
```

Burada copy-on-write (yazınca kopyala), kopyanın ancak bir dosya
değiştiğinde yer kaplaması demektir.

4a. adımından önce gerçek klasörünüze (*çalışma klasörüne*, workspace) hiçbir
şey ulaşmaz.

## Genel seçenekler

Bunlar her komut ve alt komutta çalışır.

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` | Kule'nin görevleri, ayarları, gizli değerleri ve politikayı tuttuğu yer. `KULE_HOME` ortam değişkeniyle de verilebilir; ikisi birden verilirse seçenek kazanır. Sizin bir klasörünüz olmalıdır, link değil: Kule yoksa onu `0700` olarak oluşturur, başkalarına açık olanı kapatır (ve bunu söyler), başka her şeyi reddeder. |
| `-h`, `--help` | | | Komutun yardımını yazdırır. |
| `-V`, `--version` | | | Kule'nin sürümünü yazdırır (yalnızca en üst düzeyde). |

`kule help KOMUT`, `kule KOMUT --help` ile aynı yardımı yazdırır;
`kule help group put` alt komutlarda da çalışır.

Gerçek görevlerinize dokunmadan denemeler yapmak için ayrı bir ev klasörü
kullanın:

```sh
export KULE_HOME=/tmp/kule-try
kule new ~/code/app
```

:::note

Uygulama ile CLI, ancak aynı ev klasörünü kullandıklarında aynı görevleri
görür. Bir terminalde `KULE_HOME` ayarlarsanız, Finder'dan ya da bir
başlatıcıdan açılan uygulama yine `~/.kule` kullanır.

:::

## Bir göreve atıf yapmak

Her görevin, açıldığında yazdırılan sekiz harf ve rakamdan oluşan bir
kimliği (id) vardır:

```text
$ kule new ~/code/app
task 3422f830 · copy at ~/.kule/tasks/3422f830/w/app
```

Bir komut `TASK` beklediği her yerde bu **tam kimliği** verin. Ön ekler ve
görev adları kabul edilmez:

```text
$ kule status 3422
kule: no task 3422
```

`kule list` her kimliği klasörü, adı, rengi ve grubuyla birlikte yazdırır.
Betiklerde kimliği `kule new` çıktısından alıp saklayın:

```sh
TASK=$(kule new ~/code/app | awk '{print $2}')
```

## Tüm komutlar

### Görevler

| Komut | Ne yapar |
| --- | --- |
| [`kule new`](new.md) | Bir görev açar: bir klasörün copy-on-write kopyası. |
| [`kule list`](list.md) | Görevleri uygulamanın gösterdiği sırayla listeler. |
| [`kule name`](name.md) | Bir göreve ad verir ya da adını kaldırır. |
| [`kule color`](color.md) | Bir görevi bir renkle işaretler ya da rengini kaldırır. |
| [`kule rm`](rm.md) | Bir görevi ve kopyasını siler. Klasöre dokunulmaz. |

### Görev içinde çalışmak

| Komut | Ne yapar |
| --- | --- |
| [`kule run`](run.md) | Bir ajana (Claude Code, Codex, Gemini, …) bir prompt verir; ajan kopyada, sandbox içinde çalışır. |
| [`kule exec`](exec.md) | Kopyada, sandbox içinde tek bir komut çalıştırır, sonra neyi değiştirdiğini gösterir. |
| [`kule shell`](shell.md) | Kopyada, sandbox içinde (ya da `--outside` ile sandbox dışında) kendi kabuğunuzu açar. |
| [`kule closed`](closed.md) | Uygulamanın kopyayı açtığı terminal uygulamalarını ve editörleri kapattığınızı Kule'ye söyler; böylece ajanlar görevde yeniden çalışabilir. |
| [`kule path`](path.md) | Kopyanın ya da içindeki bir dosyanın diskte nerede olduğunu yazdırır. |
| [`kule open`](open.md) | Kopyadaki bir belgeyi, resmi ya da videoyu kendi uygulamasıyla açar. |

### İnceleme ve geçmiş

| Komut | Ne yapar |
| --- | --- |
| [`kule status`](status.md) | Commit edilmemiş değişiklikleri ve politikanın onlar hakkındaki kararını gösterir. |
| [`kule steps`](steps.md) | Ajanın araç çağrılarını ve her birinin neyi değiştirdiğini listeler. |
| [`kule commit`](commit.md) | Değişiklikleri klasöre uygular, bir kontrol noktası saklar. |
| [`kule rollback`](rollback.md) | Commit edilmemiş değişiklikleri atar. |
| [`kule undo`](undo.md) | En yeni commit'i klasörden geri alır. |
| [`kule sync`](sync.md) | Klasörde bu arada değişenleri kopyaya alır. |
| [`kule history`](history.md) | Görevin commit ve geri almalardan oluşan imzalı geçmişini doğrulayarak gösterir. |

### Maliyet ve sınırlar

| Komut | Ne yapar |
| --- | --- |
| [`kule budget`](budget.md) | Tek bir görevin ajan çalıştırmalarının ne kadar tutabileceğini gösterir ya da ayarlar. |
| [`kule daily-budget`](daily-budget.md) | Tüm görevlerin herhangi bir 24 saatte toplam ne kadar tutabileceğini gösterir ya da ayarlar. |
| [`kule limits`](limits.md) | Ajan çalıştırmaları için CPU, bellek, disk, GPU ve süre sınırlarını gösterir ya da ayarlar. |

### Gruplar

| Komut | Ne yapar |
| --- | --- |
| [`kule group`](group.md) | Her birinin kendi rengi, politikası, bütçesi ve sınırları olan görev gruplarını oluşturur ve yönetir. |

### Politika ve erişim

| Komut | Ne yapar |
| --- | --- |
| [`kule policy`](policy.md) | Geçerli politikayı yazdırır ya da `balanced` ile `careful` arasında geçiş yapar. |
| [`kule rules`](rules.md) | Kule'ye bir daha sormamasını söylediklerinizi listeler, ekler ya da unutturur. |
| [`kule reveal`](reveal.md) | Bir klasörün görevlerinin yanındaki bir projeyi (salt okunur) okumasına izin verir. |
| [`kule isolation`](isolation.md) | Bu bilgisayarın sandbox'a neleri veremediğini gösterir; bunu kabul edip etmemenizi sağlar. |

### Ev dizini ve gizli değerler

| Komut | Ne yapar |
| --- | --- |
| [`kule home`](home.md) | Bir görevin komutlarının ev dizininize yazdıklarını incelemenizi, uygulamanızı ya da atmanızı sağlar. |
| [`kule secrets`](secrets.md) | Komutların hiç görmeden kullandığı API anahtarlarını saklar. |

### Ajanlar ve eklentiler

| Komut | Ne yapar |
| --- | --- |
| [`kule agents`](kule-agents.md) | Kule'nin çalıştırabildiği ajanları listeler, inceler, ekler, dener ve onaylar. |
| [`kule ext`](ext.md) | Eklenti oluşturur, paketler, kurar, onaylar ve yönetir. |

### Daemon

| Komut | Ne yapar |
| --- | --- |
| [`kule daemon`](daemon.md) | Uygulamanın konuştuğu arka plan servisini çalıştırır. Uygulama onu sizin için başlatır. |

## kule ile betik yazmak

**Çıkış kodları.** Kule küçük ve sabit bir küme kullanır:

| Kod | Anlamı |
| --- | --- |
| `0` | İşlem başarılı. |
| `2` | Kule reddetti ya da işlem başarısız oldu. Neden, stderr'de `kule:` ifadesinden sonra yazar; bazen ardından bir `hint:` satırı gelir. Hatalı argümanlar da (argüman ayrıştırıcısından) `2` ile çıkar. |
| `3` | Yalnızca `kule commit`: politika değişiklikleri geri tuttu (onay bekliyorlar ya da reddedildiler). Hiçbir şey commit edilmedi. |
| komutun kendi kodu | `kule exec` ve `kule shell`, çalıştırdıkları komutun ya da kabuğun çıkış koduyla çıkar; böylece `kule exec TASK -- npm test && kule commit TASK` okunduğu gibi çalışır. |

**Makinenin okuyabileceği çıktı.** `kule status TASK --json`, değişiklikleri,
politikanın kararını, ajanların harcadıklarını ve daha fazlasını tek bir
JSON nesnesi olarak yazdırır; bkz. [kule status](status.md). Diğer
komutlar insanlar için, her satıra bir bilgi düşecek biçimde metin yazdırır.

**Hatalar stderr'e, sonuçlar stdout'a gider.** `kule path` stdout'a yalnızca
yolu yazdırır; bu yüzden bir uyarı da yazdırdığında bile
`cd "$(kule path TASK)"` çalışır.

**Renk.** Çıktı yalnızca bir terminalde renklidir; bir boruya (pipe)
yönlendirilen çıktı düz metindir.

## Ayrıca bakın

- [Hızlı başlangıç](../getting-started/quick-start.md): gerçek çıktılarla döngü.
- [Dosyalar ve ayarlar](../reference/files.md): Kule'nin ev klasöründe neler var.
- [Ortam değişkenleri](../reference/environment.md).

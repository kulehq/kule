---
title: kule limits
sidebar_label: kule limits
description: Bir ajan çalıştırmasının bu bilgisayardan ne kadar kullanabileceğini tüm görevler, bir grup ya da tek bir görev için gösterin ya da ayarlayın.
---

# kule limits

Bir ajan çalıştırmasının bu bilgisayardan ne kadar kullanabileceğini
gösterir ya da ayarlar: işlemci, bellek, disk, GPU ve süre.

## Kullanım {#synopsis}

```sh
kule limits [OPTIONS] [SETTINGS]...
```

## Açıklama {#description}

**Sınırlar** bir görevin her ajan çalıştırmasını sınırlar. Üç **kapsamda**
ayarlanabilirler:

| Kapsam | Nasıl seçilir | Neyi sınırlar |
| --- | --- | --- |
| Tüm görevler | seçenek yok (varsayılan) | Grubunun da görevin kendisinin de kendi değerini koymadığı her görevi. |
| Bir gruptaki her görev | `--group ID` | O gruptaki her görevi, alan alan, görevin kendi değeri olmadığı yerde. |
| Tek bir görev | `--task TASK` | Yalnızca o görevi. |

Bir görevin kendi değeri grubununkinin, bir grubunki de tüm görevlerinkinin
önüne geçer. Bir grubun sınırı, gruptaki her görevi ayrı ayrı sınırlar:
bir grupta `cpu=2`, paylaşılan 2 çekirdek değil, her görev için 2 çekirdek
demektir.

Ayar vermezseniz komut kapsamın sınırlarını yazar. Ayar verirseniz onları
değiştirir, sonra yazar. Her satır değerinin nereden geldiğini söyler;
böylece hangi kapsamı değiştirdiğinizi her zaman görürsünüz.

Süren bir çalıştırma, başladığı sınırları korur. Bir değişiklik bir sonraki
çalıştırmadan itibaren geçerlidir. `kule run` ve uygulamadan başlatılan
çalıştırmalar aynı sınırlara tabidir.

### Alanlar {#the-fields}

| Alan | Birim | Aşıldığında ne olur |
| --- | --- | --- |
| `cpu` | çekirdek (`1.5` bir buçuk demek) | Çalıştırma durdurulmaz, **yavaşlatılır**: Kule süreçlerini sırayla duraklatıp sürdürür. |
| `memory` | GB, çalıştırmanın tüm süreçleri birlikte | Süreçler hemen **öldürülür** ve çalıştırma durur. |
| `disk` | Görev oluşturulduğundan beri kopyanın, geçici dosyalarının ve ev dizini değişikliklerinin büyüyebileceği GB | Çalıştırma **durur**. Hiçbir dosya bunun ötesine yazılamaz. |
| `gpu` | `on` / `off` | `off`: macOS'ta Metal'e (ve OpenCL'e) hiçbir aygıt verilmez; Linux'ta GPU aygıtları salt okunur kalır. |
| `servers` | `on` / `off` (varsayılan `off`) | macOS: `off`, ağ trafiği proxy'den geçerken çalıştırmanın programları bağlantı dinlemez; `on`, dinlerler ve Kule onları sandbox'ın dışından ağınızdan uzak tutar; bu, kabul ettiğiniz daha zayıf yalıtımdır (bkz. [kule isolation](isolation.md)). Linux'ta her sunucu her durumda bu bilgisayarla sınırlı tutulur. |
| `run` | dakika, tek bir çalıştırma | Çalıştırma **durur**. Yanıtlarınızı beklerken geçen süre de sayılır. |
| `task` | saat, görevin tüm çalıştırmalarının toplamı | Çalıştırma **durur** ve siz artırana kadar yeni çalıştırma başlamaz. |
| `idle` | hiçbir şey yapmadan geçen dakika | Çalıştırma **durur**. "Hiçbir şey yapmamak", ajandan hiç mesaj, araç çağrısı, çıktı, işlemci kullanımı ya da yeni süreç gelmemesi demektir. Sizi bekleyen bir soru sayılmaz. |

### Değerler {#values}

| Değer | Anlamı |
| --- | --- |
| 0'dan büyük bir sayı | Alanın biriminde sınır (`memory=4`, `cpu=1.5`, `run=30`). |
| `none` | Bu kapsamda bu alan için sınır yok. |
| `on`, `off` | Yalnızca `gpu` ve `servers` için. |
| `inherit` | Yalnızca `--group` ya da `--task` ile: bu kapsamın kendi değerini bırakır ve yeniden üstteki kapsamın değerini alır. |

## Argümanlar {#arguments}

| Argüman | Anlamı |
| --- | --- |
| `SETTINGS...` | Sıfır ya da daha fazla `FIELD=VALUE` çifti, örneğin `memory=4 cpu=none gpu=off`. |

## Seçenekler {#options}

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--group` | grup kimliği | | Bu gruptaki her görevin sınırlarını gösterir ya da ayarlar. Kimlikler için bkz. `kule group`. |
| `--task` | görev kimliği | | Bu görevin kendi sınırlarını gösterir ya da ayarlar. |
| `--home` | yol | `~/.kule` (ya da `KULE_HOME`) | Kule'nin evi. Bkz. [genel seçenekler](index.md). |
| `-h`, `--help` | | | Yardımı yazdırır. |

`--group` ve `--task` birlikte kullanılamaz.

## Örnekler {#examples}

Hiçbir şey ayarlı değilken varsayılanlar:

```console
$ kule limits
limits for EVERY TASK (where its group and the task set none of their own):
  processor              no limit
  memory                 no limit
  disk                   no limit
  one run                no limit
  all of a task's runs   no limit
  doing nothing          no limit
  GPU                    may be used
  servers (macOS)        do not listen
```

Her görevi 1,5 çekirdek ve 4 GB ile sınırlayın, 10 dakika boşta kalan
çalıştırmaları durdurun ve GPU'yu kapatın:

```console
$ kule limits cpu=1.5 memory=4 idle=10 gpu=off
limits for EVERY TASK (where its group and the task set none of their own):
  processor              1.5 cores
  memory                 4 GB
  disk                   no limit
  one run                no limit
  all of a task's runs   no limit
  doing nothing          10 min
  GPU                    off
  servers (macOS)        do not listen
```

1 numaralı gruptaki her görev için daha sıkı sınırlar:

```console
$ kule limits --group 1 memory=2 run=30
limits for EACH TASK IN GROUP 1 "clients" (1 task), where a task sets none of its own:
  processor              1.5 cores (every task's)
  memory                 2 GB (the group's)
  disk                   no limit (every task's)
  one run                30 min (the group's)
  all of a task's runs   no limit (every task's)
  doing nothing          10 min (every task's)
  GPU                    off (every task's)
  servers (macOS)        do not listen (every task's)
```

O gruptaki bir görevin daha fazla belleğe ihtiyacı var:

```console
$ kule limits --task 1046fb30 memory=8
limits for TASK 1046fb30 only (in group 1 "clients"):
  processor              1.5 cores (every task's)
  memory                 8 GB (its own)
  disk                   no limit (every task's)
  one run                30 min (group "clients")
  all of a task's runs   no limit (every task's)
  doing nothing          10 min (every task's)
  GPU                    off (every task's)
  servers (macOS)        do not listen (every task's)
```

Grubun değerine geri döndürün:

```console
$ kule limits --task 1046fb30 memory=inherit
limits for TASK 1046fb30 only (in group 1 "clients"):
  processor              1.5 cores (every task's)
  memory                 2 GB (group "clients")
  ...
```

## Çıktı {#output}

- İlk satır kapsamı büyük harfle adlandırır: `EVERY TASK`, `EACH TASK IN
  GROUP …` ya da `TASK … only`.
- Grup ya da görev kapsamında parantezler her değerin nereden geldiğini
  söyler: `(its own)` (kendisinin), `(the group's)` / `(group "clients")`
  (grubun) ya da `(every task's)` (tüm görevlerin).
- `no limit` o alanı hiçbir şeyin sınırlamadığı anlamına gelir; `may be used`
  GPU'nun açık olduğu anlamına gelir; `may listen, held off the network from
  outside` sunuculara izin verildiği anlamına gelir.

## Hatalar ve çözümler {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: memory: write a limit as FIELD=VALUE, such as memory=4` | `=` ve bir değer ekleyin. |
| `kule: foo: a limit is cpu, memory, disk, gpu, servers, run, task or idle` | Bu alan adlarından birini kullanın. |
| `kule: memory=0: a number of GB above 0, or none` | Pozitif bir sayı kullanın; `none` sınırı kaldırır. |
| `kule: gpu: on, off or inherit` | `gpu` ve `servers` yalnızca bu sözcükleri alır. |
| ``kule: memory: `inherit` is for --group or --task; the limits for every task have nothing above them`` | Tüm görevler kapsamında bir değer ya da `none` kullanın. |
| `error: the argument '--group <GROUP>' cannot be used with '--task <TASK>'` | Tek bir kapsam seçin. |
| `kule: no group 9` / `kule: no task …` | Kimliği `kule group` ya da `kule list` ile kontrol edin. |

Bir sınır bir çalıştırmayı durdurduğunda çalıştırma, Live'da ve
`kule run`'ın çıktısında nedeniyle birlikte başarısız olur; örneğin
`Stopped: the run's processes held 4.2 GB, over its memory limit of 4 GB`.

## Uygulamada {#in-the-app}

*Settings → Limits*; burada **Applies to** ile *Every task* (tüm görevler),
*One group* (bir grup) ya da *One task* (tek görev) seçilir. Bir görevin ya
da grubun ⋮ menüsündeki *Limits…* o kapsamı açar; Live'da istem satırının
altındaki ölçüm çubuğu da görevin kapsamını açar. Bir görevi bir gruba
taşımak ya da gruptan çıkarmak, sınırlarını değiştirecekse önce sorar.

## Ayrıca bakın {#see-also}

- [`kule budget`](budget.md), [`kule daily-budget`](daily-budget.md): çalıştırmaların ne kadar tutabileceği.
- [`kule group`](group.md): `kule group put`, bir taşımanın görevin sınırlarını nasıl değiştirdiğini söyler.
- [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md)

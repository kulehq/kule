---
title: kule budget
sidebar_label: kule budget
description: Bir görevin ajan çalıştırmalarının toplamda ne kadar tutabileceğini gösterin ya da ayarlayın.
---

# kule budget

Bir görevin ajan çalıştırmalarının toplamda ne kadar tutabileceğini gösterir
ya da ayarlar.

## Kullanım {#synopsis}

```sh
kule budget [OPTIONS] <TASK> [AMOUNT] [CURRENCY]
kule budget [OPTIONS] <TASK> --release
```

## Açıklama {#description}

**Bütçe**, ajan çalıştırmalarının maliyetine, ajanların kendi bildirdiği
şekliyle bir üst sınır koyar. `kule budget` tek bir
[görevin](../concepts/tasks-and-copies.md) bütçesini ayarlar: o görevin ilk
çalıştırmadan itibaren tüm çalıştırmalarının toplamı.

- **Bir tutarla** çalıştırırsanız görevin bütçesini ayarlar; görevin ne
  harcadığını ve ne kaldığını yazar.
- **`off` ile** görevin bütçesini kaldırır.
- **Tutar vermezseniz** görevin bir sonraki çalıştırmasını sınırlayan *her*
  bütçeyi listeler: görevin kendi bütçesi, grubunun bütçeleri (bkz.
  [`kule group budget`](group.md)) ve tüm görevler için günlük bütçe (bkz.
  [`kule daily-budget`](daily-budget.md)). Bir sonraki çalıştırmanın en fazla
  ne kadar tutabileceğini de söyler: bunların en sıkısı.
- **`--release` ile** görevde bitmeyen çalıştırmaların bütçelerden hâlâ
  tuttuğunu bırakır (aşağıya bakın).

Bir bütçe nasıl işler:

- Üzerindeki bir bütçe tükendiğinde hiçbir çalıştırma başlamaz. Ret mesajı
  hangi bütçe olduğunu söyler.
- Bütçenin saydığı, çalıştırmaların harcadıklarına dair bir kayıt
  okunamıyorken de hiçbir çalıştırma başlamaz: Kule neyin kaldığını
  bilemez. Böyle bir kaydı hiç harcanmamış saymak yerine sayar. Kaydı
  (görevin `usage.jsonl` dosyasında) düzeltin ya da kaldırın, veya bütçeyi
  kapatın.
- Bir çalıştırma, ajanı başlamadan önce payını ayırır: kalanların en
  sıkısı; diğer çalıştırmaların ayırdığı harcanmış sayılır. Aynı bütçe
  altında aynı anda başlayan çalıştırmalar kalanı paylaşır. Çalıştırmanın
  kullanımı kaydedildiğinde yalnızca harcadığı sayılır.
- Kullanımı hiç gelmeyen bir çalıştırmanın (Kule durdurulmuştur, kayıt
  yazılamamış ya da okunamamıştır) payının tamamı sayılmaya devam eder.
  `--release` onu bırakır, böylece artık harcanmış sayılmaz, ve bunun
  kaydını görevin `usage.jsonl` dosyasında tutar. Görevde bir ajan
  çalışırken reddedilir.
- Maliyetini çalışırken bildiren bir ajanın çalıştırması, kalan tutara
  ulaştığında durdurulur.
- Claude Code maliyetini her turun sonunda bildirir; bu yüzden bir tur
  bütçeyi aşabilir ve reddedilen, *bir sonraki* çalıştırma olur.
- Maliyet bildirmeyen bir ajan (örneğin abonelikle oturum açmış bir ajan)
  bütçeyle sınırlanmaz. Kule kendisine söylenmeyeni tutamaz.
- Bir bütçe yalnızca kendi para biriminde bildirilen maliyetleri sayar.
  Ajanlar USD ile bildirir; ajanınızın başka bir para birimi bildirdiğinden
  emin değilseniz bütçeleri USD olarak tutun.

`kule budget` yalnızca görevin Kule'nin evindeki kendi ayarını değiştirir.
Görevin kopyasına ya da klasörünüze hiç dokunmaz. Kapsamların birbirine
nasıl oturduğu için [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md)
sayfasına bakın.

## Argümanlar {#arguments}

| Argüman | Anlamı |
| --- | --- |
| `TASK` | Görevin `kule list`'in yazdığı kimliği (örneğin `1046fb30`). Kimliğin tamamı gerekir. |
| `AMOUNT` | `5` ya da `2.50` gibi pozitif bir tutar, ya da bütçeyi kaldırmak için `off`. Bütçeleri göstermek için boş bırakın. |
| `CURRENCY` | Büyük harfle bir ISO 4217 kodu, örneğin `USD` ya da `EUR`. Varsayılan: `USD`. |

## Seçenekler {#options}

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--release` | | | Görevde bitmeyen çalıştırmaların bütçelerden hâlâ tuttuğunu bırakır; böylece artık harcanmış sayılmaz. Görevde bir ajan çalışırken reddedilir. `AMOUNT` ya da `CURRENCY` almaz. |
| `--home` | yol | `~/.kule` (ya da `KULE_HOME`) | Kule'nin evi. Bkz. [genel seçenekler](index.md). |
| `-h`, `--help` | | | Yardımı yazdırır. |

## Örnekler {#examples}

Bir göreve 5 USD bütçe verin:

```console
$ kule budget 1046fb30 5
spent: nothing reported in 0 runs; budget 5.00 USD, 5.00 USD left
```

Görevin bir sonraki çalıştırmasını sınırlayan her bütçeyi görün:

```console
$ kule budget 1046fb30
budgets over task 1046fb30's next run:
  the task's own                   5.00 USD, 5.00 USD left
the next run may cost at most 5.00 USD
```

Kendi bütçeleri olan bir gruptaki görev, günlük bütçe de ayarlıyken:

```console
$ kule budget 1046fb30
budgets over task 1046fb30's next run:
  group "clients", all its runs    20.00 USD, 20.00 USD left
  group "clients", any 24 hours    5.00 USD, 5.00 USD left
the next run may cost at most 5.00 USD
```

Euro cinsinden bir bütçe ayarlayın, sonra kaldırın:

```console
$ kule budget 1046fb30 2.50 EUR
spent: nothing reported in 0 runs; budget 2.50 EUR, 2.50 EUR left
$ kule budget 1046fb30 off
```

Görevin henüz hiç çalıştırması yoksa `off` hiçbir şey yazmaz.

Başka bir görevde bir çalıştırma günlük bütçe altında çalışırken, ayırdığı
bütçenin yanında görünür ve sayılır:

```console
$ kule budget 1046fb30
budgets over task 1046fb30's next run:
  every task, any 24 hours         10.00 USD, 0.00 USD left (10.00 USD set aside for runs at work or that did not end)
no run starts: what is left of the daily budget (10.00 USD) is set aside for runs at work, or that did not end, in task 7c2e9a10: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end
```

O görevde bitmeyen bir çalıştırmanın payını bırakın (`kule run`'ı
öldürülmüştü):

```console
$ kule budget 7c2e9a10 --release
let go of 10.00 USD set aside for a run that did not end; it no longer counts as spent
```

Görevin çalıştırmaları ya da kendi bütçesi varsa ardından bir `spent:`
satırı gelir.

## Çıktı {#output}

- `spent: … in N runs`, ajanların bu görev için bildirdiklerini toplar. Bazı
  çalıştırmalar token bildirip maliyet bildirmediyse satır bunların kaç tane
  olduğunu söyler (`(2 without a cost)`). Görevin bazı kayıtları
  okunamıyorsa kaç tane olduğunu söyler (`(1 record cannot be read)`).
  Çalışmakta olan ya da bitmeyen çalıştırmaların ayırdığı
  ` (10.00 USD set aside for runs at work or that did not end)` olarak
  eklenir.
- `budgets over task …'s next run` her bütçeyi kalan tutarıyla listeler.
  Tükenmiş bir bütçe `0.00` kaldığını gösterir. Ayrılan harcanmış sayılır ve
  ardından gösterilir.
- `the next run may cost at most …` kalanların en sıkısıdır.
- `no budget holds task …` hiçbir kapsamda bütçe ayarlı olmadığı anlamına
  gelir.
- `no run starts: …` bütçelerden birinin tükendiği ya da kalanının ayrıldığı
  anlamına gelir; metin hangisi olduğunu söyler.
- `let go of … set aside for a run that did not end; it no longer counts as spent`,
  `--release`'in bıraktığıdır, her pay için bir satır; hiç yoksa
  `nothing is set aside for runs that did not end in task …`.

## Hatalar ve çözümler {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: abc: an amount like 5 or 2.50, or off` | Bir sayı ya da `off` yazın. |
| `kule: a budget is a positive amount in a currency like USD, not 0 USD` | 0'dan büyük bir tutar ve büyük harfle üç harfli bir para birimi kullanın (`usd` değil, `USD`). Bir bütçeyi kaldırmak için `off` kullanın. |
| `kule: no task 1046fb3` | `kule list`'teki görev kimliğinin tamamını kullanın. |
| `task 1046fb30 has spent its budget; raise it or turn it off to run again` (`kule run`'dan ya da uygulamadan) | Bütçeyi artırın (`kule budget TASK 10`) ya da kaldırın (`kule budget TASK off`). |
| ``what is left of task 1046fb30's budget (5.00 USD) is set aside for runs at work, or that did not end, in task 1046fb30: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end`` (`kule run`'dan, uygulamadan ya da burada `no run starts:` ile) | Çalışmakta olan çalıştırmanın bitmesini bekleyin. Çalışan yoksa (Kule durdurulmuştu), mesajın adını verdiği görevde payını bırakın: `kule budget TASK --release`. |
| `kule: an agent is at work in task 1046fb30 (process 4242); its run holds its share until it ends` (`--release`'ten) | Çalıştırmanın bitmesini bekleyin ya da onu durdurun; ardından harcadığı sayılır. |
| `Kule cannot tell what task 1046fb30's budget has left: 1 record of what runs spent (a task's usage.jsonl) cannot be read; mend or remove them, or turn the budget off, to run again` (`kule run`'dan, uygulamadan ya da burada `no run starts:` ile) | Görevin `usage.jsonl` dosyasındaki okunamayan satırı düzeltin ya da kaldırın, veya bütçeyi kapatın. Tutan bir grubun ya da günlük bütçeyse mesaj onun adını verir. |

Her hata 2 durum koduyla çıkar.

## Uygulamada {#in-the-app}

**Applies to: One task** seçiliyken *Settings → Budget*, ya da görevin ⋮
menüsündeki *Budget…*. Görev başlığındaki çip, görevin ne harcadığını ve ne
kaldığını, bazı kayıtlar okunamıyorsa da kırmızıyla *N usage records
unreadable · budget held* gösterir. Çalıştırmalar bir pay tutarken *X set
aside* der ve bunu kalandan düşer. `--release` yalnızca komut satırındadır.
Pencere öndeyken değilse Kule bir bütçenin %80'i harcandığında ve tamamı
bittiğinde size haber verir.

## Ayrıca bakın {#see-also}

- [`kule daily-budget`](daily-budget.md): tüm görevler birlikte, herhangi bir 24 saatte.
- [`kule group`](group.md): bir grubun bütçeleri (`kule group budget`).
- [`kule limits`](limits.md): işlemci, bellek, disk ve süre sınırları.
- [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md)

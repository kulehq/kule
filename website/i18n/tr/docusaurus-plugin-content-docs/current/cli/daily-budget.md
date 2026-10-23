---
title: kule daily-budget
sidebar_label: kule daily-budget
description: Tüm görevlerin herhangi bir 24 saatte birlikte ne kadar tutabileceğini gösterin ya da ayarlayın.
---

# kule daily-budget

Tüm görevlerinizin herhangi bir 24 saatte birlikte ne kadar tutabileceğini
gösterir ya da ayarlar.

## Kullanım {#synopsis}

```sh
kule daily-budget [OPTIONS] [AMOUNT] [CURRENCY]
```

## Açıklama {#description}

**Günlük bütçe**, Kule'nin evindeki tüm görevlerin üzerinde tek bir
bütçedir. Son 24 saatte başlayan çalıştırmaları sayar, yani kayar: dün
öğleden sonraki bir çalıştırma yarın öğleden sonra sayılmaz olur.

- Son 24 saatin çalıştırmaları bütçeyi tükettiğinde hiçbir yerde hiçbir
  çalıştırma başlamaz.
- Başlayan bir çalıştırma, görevin kendi bütçesinin yaptığı gibi kalan
  tutarla sınırlanır (bkz. [`kule budget`](budget.md)).
- Maliyetler ajanların bildirdikleridir. Maliyet bildirmeyen çalıştırmalar
  sınırlanmaz.
- Herhangi bir görevde, bir çalıştırmanın harcadığına dair okunamayan bir
  kayıt her çalıştırmayı tutar: Kule onun ne zaman yazıldığını bilemez;
  bugünkü olabilir.

Bir tutarla komut günlük bütçeyi ayarlar; `off` ile kaldırır; ikisi de
yoksa yalnızca gösterir. Her biçim, tüm görevlerin son 24 saatte ne
harcadığını yazar.

## Argümanlar {#arguments}

| Argüman | Anlamı |
| --- | --- |
| `AMOUNT` | `20` ya da `7.50` gibi pozitif bir tutar, ya da `off`. Bütçeyi göstermek için boş bırakın. |
| `CURRENCY` | Büyük harfle bir ISO 4217 kodu, örneğin `USD` ya da `EUR`. Varsayılan: `USD`. |

## Seçenekler {#options}

| Seçenek | Değer | Varsayılan | Anlamı |
| --- | --- | --- | --- |
| `--home` | yol | `~/.kule` (ya da `KULE_HOME`) | Kule'nin evi. Bkz. [genel seçenekler](index.md). |
| `-h`, `--help` | | | Yardımı yazdırır. |

## Örnekler {#examples}

Hiçbiri ayarlı değilken gösterin:

```console
$ kule daily-budget
last 24 hours, every task: no cost reported in 0 runs; no daily budget
```

Tüm görevler için günde 20 USD'ye izin verin:

```console
$ kule daily-budget 20
last 24 hours, every task: no cost reported in 0 runs; daily budget 20.00 USD, 20.00 USD left
```

Birkaç çalıştırmadan sonra:

```console
$ kule daily-budget
last 24 hours, every task: 3.40 USD in 6 runs; daily budget 20.00 USD, 16.60 USD left
```

Kaldırın:

```console
$ kule daily-budget off
last 24 hours, every task: no cost reported in 0 runs; no daily budget
```

## Çıktı {#output}

`last 24 hours, every task: <cost> in <N> runs`, tüm görevlerin son 24 saatte
başlayan çalıştırmalarının bildirdiğidir. Maliyetler birden fazla para
biriminde geldiyse `+` ile birleştirilir. `;` işaretinden sonra günlük bütçe
ve kalanı gelir, ya da `no daily budget`.

## Hatalar ve çözümler {#errors-and-fixes}

| Mesaj | Ne yapmalı |
| --- | --- |
| `kule: abc: an amount like 20 or 7.50, or off` | Bir sayı ya da `off` yazın. |
| `kule: a budget is a positive amount in a currency like USD, not 0 USD` | 0'dan büyük bir tutar ve büyük harfle bir para birimi kodu kullanın. |
| `the daily budget is spent: every task together, in the last 24 hours; raise it or turn it off to run again` (bir çalıştırma reddedildiğinde) | Eski çalıştırmaların 24 saatin dışına çıkmasını bekleyin, bütçeyi artırın ya da kaldırın. |
| `Kule cannot tell what the daily budget has left: <N> records of what runs spent (in tasks' usage.jsonl) cannot be read; mend or remove them, or turn the budget off, to run again` (bir çalıştırma reddedildiğinde) | Görevlerin `usage.jsonl` dosyalarındaki okunamayan satırları düzeltin ya da kaldırın, veya günlük bütçeyi kapatın. |

## Uygulamada {#in-the-app}

**Applies to: Every task** seçiliyken *Settings → Budget*. Pencere önde
değilken Kule, günlük bütçenin %80'i harcandığında ve tamamı bittiğinde size
haber verir. Eski çalıştırmalar 24 saatin dışına çıktıkça %80'in altına
düşen bir bütçe, yeniden yaklaştığında tekrar haber verilir.

## Ayrıca bakın {#see-also}

- [`kule budget`](budget.md): tek bir görevin bütçesi.
- [`kule group`](group.md): bir grubun bütçeleri, kendi günlük bütçesi dahil (`kule group budget ID AMOUNT --daily`).
- [Bütçeler ve sınırlar](../concepts/budgets-and-limits.md)

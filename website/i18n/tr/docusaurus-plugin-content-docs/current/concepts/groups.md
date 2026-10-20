---
title: Gruplar
sidebar_label: Gruplar
description: Görevleri adı ve rengi olan gruplarda toplayın; her gruba kendi politikasını, ortak bütçesini ve sınırlarını verin.
---

# Gruplar

Bir **grup** görevleri bir araya toplar. Her grubun:

- bir **adı** ve bir **rengi** vardır (kendi renkleri yoksa görevleri bu
  rengi gösterir);
- görevlerinin değerlendirildiği bir **politikası** vardır;
- görevlerinin paylaştığı bir **bütçesi** vardır;
- görevlerinin her biri için **sınırları** vardır.

Grupları müşterileri, projeleri ya da iş türlerini ayrı tutmak için
kullanın: örneğin careful politikası altında günlük bütçesi olan bir
"clients" grubu ve balanced politikası altında bir "sandbox" grubu.

## Bir grup açmak ve doldurmak

```sh
kule group new clients --color blue   # bir grup aç
kule group                            # grupları, politikalarını ve görevlerini listele
kule group put TASK GROUP_ID          # bir görevi gruba koy
kule group put TASK                   # görevi grubundan çıkar
kule group rename ID "Client work"
kule group color ID teal              # ya da rengi kaldırmak için renk vermeyin
kule group delete ID                  # görevleri kalır, hiçbir grupta olmadan
```

Bir grubun kimliği (id) küçük bir sayıdır; grubu açtığınızda ve
`kule group` tarafından yazdırılır:

```text
group 1 made: clients
1  clients · the policy for every task (blue)
```

Renkler `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple` ve
`pink`'tir.

Uygulamada: *Tasks*'ın yanındaki **+** bir grup açar. Bir görevi onun
üzerine sürükleyin ya da görevin ⋮ menüsündeki *Group…*'u kullanın;
gruptan çıkarmak için *Not in a group* üzerine sürükleyin. Grubun ⋮ menüsü
rengini, politikasını, bütçesini ve sınırlarını değiştirir.

## Bir grubun politikası

```sh
kule group policy ID            # grup için geçerli kuralları yazdır
kule group policy ID default    # her görev için geçerli politika (varsayılan)
kule group policy ID careful    # diğerleri ne kullanırsa kullansın Kule'nin careful politikası
kule group policy ID balanced   # Kule'nin balanced politikası
kule group policy ID own --file group.cedar   # Cedar ile sizin yazdığınız bir politika
```

Yazdığınız bir politika saklanmadan önce denetlenir. Bir deponun kendi
retleri (`.kule/policy.cedar`) ve Kule'ye bir daha sormamasını
söyledikleriniz grubun görevleri için de geçerli olmaya devam eder. Bkz.
[Politika](policy.md).

## Bir grubun bütçesi ve sınırları

```sh
kule group budget ID 20          # görevlerinin tüm çalıştırmaları boyunca
kule group budget ID 10 --daily  # herhangi bir 24 saatte
kule group budget ID             # ikisini de ve harcananı göster
kule group budget ID off         # birini kaldır

kule limits --group ID memory=8 cpu=2
```

Bir grubun sınırları görevlerinin hepsine birlikte değil, **her birine**
uygulanır: bir grubun işlemci sınırı görev başınadır. Bir çalıştırma,
görevinin çalıştığı sırada bulunduğu grup için sayılır. Bkz.
[Bütçeler ve sınırlar](budgets-and-limits.md).

## Bir görevi taşımak önce sorar

Bir görevi bir gruba almak ya da gruptan çıkarmak, ona nasıl davranıldığını
değiştirebilir. Taşıma şunları değiştirecekse:

- görevin bekleyen değişikliklerinden herhangi birinin kararını (örneğin
  sizi bekleyen ama kendiliğinden geçecek bir değişiklik), ya da
- çalıştırmalarına uygulanan sınırları veya grup bütçelerini
  (`Its limits: memory 8 GB → 2 GB`),

Kule neyin değiştiğini listeler ve önce sorar. `kule group put` değişiklikleri
yazdırır.

## İlgili sayfalar

- [`kule group`](../cli/group.md), [`kule limits`](../cli/limits.md)
- [Görevler ve kopyalar](tasks-and-copies.md)
- [Settings](../app/settings.md)

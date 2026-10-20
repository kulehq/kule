---
title: Timeline, adımlar ve fork'lar
sidebar_label: Timeline
description: Bir görevin commit'lerden, adımlardan, bağlantılardan ve sandbox retlerinden oluşan geçmişi; bir adıma nasıl geri dönüleceği ve oradan nasıl fork alınacağı.
---

# Timeline: adımlar, geri dönmek ve fork almak

**Timeline** (Mod+3), görevin geçmişidir. "Bu görevde ne oldu, hangi
sırayla oldu" sorusunu yanıtlar.

## Neleri listeler {#what-it-lists}

| Kayıt | Nedir |
| --- | --- |
| **Commits** | En yenisi önce; her biri neyi uyguladığıyla. En yenisinde **Undo** vardır. Her commit ve undo, görevin imzalı geçmişinde de (`kule history TASK`) bir kayıttır. |
| **Steps** (adımlar) | Commit edilmemiş araç çağrıları; her biri değiştirdiği dosyalarla. Bir commit adımları baştan başlatır. |
| **Connections** (bağlantılar) | Görevin komutlarının kurduğu ağ bağlantıları ve her birinin geçirildiği, sorulduğu ya da reddedildiği. |
| **Refusals** (retler) | Sandbox'ın komutlara reddettiği işlemler. macOS bunları bildirir; Linux'ta komut hatayı görür ama Kule onu listeleyemez. |

## Bir commit'i geri almak {#undo-a-commit}

**Undo** (Mod+Shift+U, iki kez basın), görevin en yeni commit'ini
klasörünüzde ve kopyada geri alır; commit'in yerine geçtiği içeriği görevin
kontrol noktasından geri koyar. Daha geriye gitmek için tekrarlayın. Bkz.
[İnceleme, commit ve undo](../concepts/review-commit-undo.md).

## Bir adıma geri dönmek {#go-back-to-a-step}

Her adımın ve "ilk adımdan önce" kaydının iki eylemi vardır. Birincisi
**Go back here**'dır: kopyayı o adımdan sonraki haline geri getirir.

- Geri dönmek de bir adımdır; dolayısıyla onu da geri alabilirsiniz.
- Son adımdan bu yana yapılan değişiklikler önce bir adım olarak saklanır;
  böylece hiçbir şey kaybolmaz.
- Geri dönülemeyen bir adım nedenini söyler: 64 MB'ı aşan bir dosya, tek
  seferde 1 GB'tan fazlası ya da ikinci bir adı olan bir dosya (hard link).
- `.git` geri getirilmez. Bir commit adımları baştan başlatır.

## Bir görevden fork almak {#fork-a-task}

İkinci eylem **Fork from here**'dır: aynı klasörde, bu görevin kopyasının o
adımdan sonraki halinden başlayan **yeni bir görev** oluşturur. Görevin ⋮
menüsündeki *Fork task* (Mod+Shift+D) ise kopyanın şimdiki halinden fork
alır.

Fork, görevin adını "(fork)" ekiyle, rengini, grubunu, bütçesini ve gizli
değer yer tutucularını alır; kendine ait adımları ya da commit'leri yoktur.
Değişiklikleri klasöre kendi incelemesi ve commit'i ile ulaşır.

:::tip Ne zaman fork almalı

Bu yönü kaybetmeden başka bir yön denemek istediğinizde ya da iki fikir
üzerinde iki ajan çalıştırıp daha iyi sonucu tutmak istediğinizde fork alın.

:::

## Terminalden {#from-a-terminal}

[`kule steps`](../cli/steps.md) commit edilmemiş işin adımlarını listeler,
[`kule undo`](../cli/undo.md) en yeni commit'i geri alır,
[`kule history`](../cli/history.md) da imzalı geçmişi kayıt kayıt
denetleyerek yazdırır. Bir adıma geri dönmek ve fork almak yalnızca
uygulamada vardır.

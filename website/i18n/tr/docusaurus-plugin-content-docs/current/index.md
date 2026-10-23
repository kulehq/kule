---
slug: /
title: Kule nedir?
sidebar_label: Giriş
description: Kule, yapay zekâ ajanlarının klasörünüzün bir kopyasında, bir sandbox içinde çalışmasını sağlar; siz inceleyip commit etmeden gerçek klasöre hiçbir şey ulaşmaz.
---

# Kule nedir?

Kule, dosyalarınız üzerinde çalışan **yapay zekâ ajanları için bir kontrol
odasıdır**. Bu dosyalar kod olabilir; ama notlar, belgeler, fotoğraflar ya
da veriler de olabilir. İşi ajanlar yapar; siz izler, onaylar ve yön
verirsiniz.

Fikir basit: **ajan gerçek klasörünüze asla dokunmaz.** Bir kopya üzerinde,
bir sandbox (yalıtılmış ortam) içinde çalışır; neyin geri döneceğine siz
karar verirsiniz.

```text
 your folder ──copy──▶ task copy ──agent works here, in a sandbox──▶ changes
      ▲                                                                 │
      └──────────── commit (keeps a checkpoint, undo puts it back) ◀────┘
                                   review
```

:::note Kule bir önizleme (preview) sürümüdür

Kule **macOS ve Linux**'ta çalışır. Windows desteği planlanıyor. En son
ön sürüm
[`0.2.0-preview.8`](https://github.com/kulehq/kule/releases/tag/v0.2.0-preview.8):
[indirin ya da Kule'yi kaynak koddan derleyin](getting-started/install.md).

:::

## Neden kullanmalı?

Claude Code, Codex ve Gemini gibi kodlama ajanları bilgisayarınızda komut
çalıştırır. Bu normalde, sizin değiştirebildiğiniz her dosyayı
değiştirebilecekleri, kimlik bilgilerinizi okuyabilecekleri ve ağdaki her
sunucuya ulaşabilecekleri anlamına gelir. Ya her komutu izlersiniz ya da
ajana güvenirsiniz.

Kule size üçüncü bir yol sunar:

- **Ajan bir kopya üzerinde çalışır.** macOS'ta (APFS) ve Btrfs ya da XFS
  kullanan Linux'ta kopya neredeyse hiç disk alanı kaplamaz.
- **Ajan bir sandbox içinde çalışır.** Yalnızca kendi kopyasına ve kendi
  geçici dosyalarına yazabilir. `~/.ssh`, `~/.aws`, Documents, Desktop ve
  diğer projeleriniz ondan gizlenir.
- **Ağ trafiği bir proxy'den geçer.** Paket depoları ve ajanın kendi model
  API'si geçer. Geri kalan her şey sizin yanıtınızı bekler ve her bağlantı
  kaydedilir.
- **Neyin değiştiğini bir politika değerlendirir.** Sıradan dosya
  düzenlemeleri kendiliğinden geçer. Neyin kurulacağını, derleneceğini ya da
  çalışacağını belirleyen değişiklikler (`package.json`, lock dosyaları, CI
  dosyaları, git hook'ları…) sizin onayınızı bekler.
- **Siz commit etmeden klasörünüze hiçbir şey ulaşmaz.** Her commit bir
  kontrol noktası saklar; böylece `undo` klasörü eski hâline getirir.
- **Gizli değerler gizli kalır.** Ajan yalnızca yer tutucuları görür;
  Kule'nin proxy'si gerçek API anahtarını yalnızca doğru sunucuya giden
  isteklere koyar.

## Kule'yi kullanmanın iki yolu

| | Nedir | Buradan başlayın |
| --- | --- | --- |
| **Masaüstü uygulaması** | Görevleriniz, ajanla sohbet, Review ve Timeline'ın bulunduğu bir pencere. Günlük iş için en uygunu. | [Uygulamada ilk göreviniz](getting-started/first-task-in-the-app.md) |
| **`kule` komutu** | Aynı motor, terminalden: görev açın, sandbox içinde komut ya da ajan çalıştırın, inceleyip commit edin. | [Hızlı başlangıç](getting-started/quick-start.md) |

İkisi de aynı görevler üzerinde çalışır; dilediğiniz gibi birlikte
kullanabilirsiniz.

## Kule'nin kullandığı terimler

Bu terimlerle belgelerin her yerinde karşılaşacaksınız:

| Terim | Anlamı |
| --- | --- |
| **Workspace (çalışma klasörü)** | Gerçek klasörünüz; Kule'ye verdiğiniz klasör. |
| **Task (görev)** | Bir çalışma klasörü üzerindeki tek bir iş. `1a2b3c4d` gibi 8 karakterlik bir kimliği vardır. |
| **Copy (kopya)** | Görevin, çalışma klasörünün copy-on-write (yazınca kopyala) kopyası. Ajan yalnızca burada çalışır. |
| **Effect (etki)** | Bir komutun değiştirdiği tek bir şey: oluşturulan, değiştirilen ya da silinen bir dosya, taşınan bir git dalı, eklenen bir hook. |
| **Policy (politika)** | Her etkiyi değerlendiren kurallar. Bir kuralın kararı **pass** (kendiliğinden geçer), **ask** (onay bekler) ya da **deny** (reddedilir, asla geçmez) olur. |
| **Commit** | Görevin değişikliklerini çalışma klasörüne uygulamak. |
| **Checkpoint (kontrol noktası)** | Bir commit'in yerine koyduğu şeyler; `undo` onları geri getirebilsin diye saklanır. |
| **Step (adım)** | Ajanın bir şeyi değiştiren tek bir araç çağrısı. Herhangi bir adıma geri dönebilirsiniz. |
| **Question (soru)** | Politikanın size sorduğu ve Approve ya da Deny bekleyen bir ağ bağlantısı. |
| **Home layer (ev katmanı)** | Görevin ev dizininize (`~/.zshrc`, `~/.config`) yaptığı yazmaların, siz inceleyesiniz diye düştüğü yer. |
| **Placeholder (yer tutucu)** | Ajanın bir gizli değerin gerçek değeri yerine gördüğü şey. |

[Kule nasıl çalışır](concepts/how-kule-works.md) sayfası hepsini tek bir
sayfada açıklar.

## Sırada ne var

- [Kule'yi kurun](getting-started/install.md)
- [Hızlı başlangıç: terminalde beş dakika](getting-started/quick-start.md)
- [Uygulamada ilk göreviniz](getting-started/first-task-in-the-app.md)
- [Tanıtım videosunu izleyin](intro-video.mdx)
- [Bütün `kule` komutları](cli/index.md)
- [Güvenlik modeli](security.md)

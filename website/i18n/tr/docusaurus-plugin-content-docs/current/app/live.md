---
title: Live, ajan sohbeti
sidebar_label: Live (ajan sohbeti)
description: Bir ajana istem verin; yanıtını, araç çağrılarını ve çıktısını geldikçe izleyin, sorularını yanıtlayın ve onu durdurun.
---

# Live: ajan sohbeti

**Live** olarak da anılan **Agent** paneli (Mod+1), bir ajana ne yapacağını
söylediğiniz ve onu çalışırken izlediğiniz yerdir. Yanındaki terminal gibi
okunur: konuşma paneli doldurur, istem (prompt) ise en altta, siz yazdıkça
büyüyen bir komut satırıdır.

## Prompt yazmak {#writing-a-prompt}

1. İmleci prompt satırına getirin: üzerine tıklayın ya da **Mod+L**'ye
   basın.
2. Ajanın ne yapması gerektiğini yazın. **Shift+Enter** yeni bir satır
   açar.
3. **Enter**'a (ya da **Mod+Enter**'a) basın veya **Run**'a tıklayın.

Prompt satırının altında:

| Denetim | Ne yapar |
| --- | --- |
| **Agent picker** (ajan seçici) | Prompt'u alacak ajanı seçer: Claude Code, Codex, Gemini ya da sizin eklediğiniz bir ajan (bkz. [Ajanlar](../using-agents.md)). |
| **waits** (bekleme süresi) | Ajan, politikanın size sorduğu bir yere bağlanmak istediğinde yanıtınızı ne kadar bekleyeceği: 1, 5, 15 (varsayılan) ya da 30 dakika, veya 1, 2, 4 ya da 8 saat. Yanıt gelmezse bağlantı reddedilir ve ajan onsuz devam eder. |
| **Measure bar** (ölçüm çubuğu) | Bir çalıştırma sürerken: bilgisayardan ne kadar kullandığı, görevin sınırlarına göre. Bkz. [aşağısı](#the-measure-bar). |

Preview'da sormak için seçtiğiniz yerler, prompt satırının üstünde
**etiketler** (chip) olarak görünür; bkz.
[Files ve Preview](files-and-preview.md).

Bir görevde aynı anda bir ajan çalışır. Ajanları yan yana çalıştırmak için
aynı klasörde ya da başka klasörlerde daha fazla görev oluşturun. Commit
etmeden önce ajanı yeniden çalıştırabilirsiniz: siz karar verene kadar
değişiklikler kopyada birikir.

## Live'ın gösterdikleri {#what-live-shows}

Geldikçe şunlar görünür:

- **Prompt'unuz**, *You* altında kendi bloğunda, ajanın yanıtı da onun
  adının altında. Her iletinin bir **kopyalama düğmesi** vardır (işaretçi
  iletinin üzerindeyken görünür); iletiyi yazıldığı gibi, Markdown'ıyla
  birlikte kopyalar.
- **Ajanın yanıtı**, Markdown olarak düzenlenmiş; kod blokları bir
  editördeki gibi renklendirilmiş.
- Ajan paylaştığında, daha sönük bir stilde **düşünmesi**.
- **Araç çağrıları**, her biri bir satır: çalışırken `·`, bittiğinde `✓`,
  başarısız olduğunda `✗`; okun ardından da çağrının değiştirdiği dosyalar.
  Bir şeyi değiştiren her çağrı, bakabileceğiniz ve geri dönebileceğiniz
  bir **adımdır** (bkz. [Timeline](timeline.md)).
- **Her çağrının yazdırdıkları**, örneğin bir komutun çıktısı, kendi
  satırının altında:
  - komut çalışırken akar;
  - çağrı çalışırken ya da başarısız olduğunda açıktır, sorunsuz
    bittiğinde kapanır (açmak ya da kapatmak için satıra tıklayın);
  - terminal renkleri ve imleç hareketleri ayıklanır, her çağrının
    çıktısının son 64 KB'ı saklanır.
- **Notlar**: ajanın planı, ona verilen izinler, Kule'nin durdurduğu bir
  sunucu, bir ajanın alamadığı resimler, Kule'nin görevin sandbox'ında
  sonlandırdıkları (bkz. [Süreçler ve sunucular](../concepts/sandbox.md#processes-and-servers))
  ve Kule'nin yapamadıkları:
  - *Kule could not read the extensions' MCP servers (…); this run goes
    without them* (Kule eklentilerin MCP sunucularını okuyamadı; bu
    çalıştırma onlarsız sürer);
  - *Kule could not write the agent's log (…); what the agent writes to it
    is not kept for the rest of this run* (Kule ajanın günlüğüne yazamadı;
    ajanın oraya yazdıkları çalıştırmanın geri kalanında saklanmaz). Ajan
    devam eder.
- Ajanın programı, politikanın kendiliğinden geçirmediği bir yere bağlanmak
  istediğinde **soru kartları** (bkz. [aşağısı](#questions)).
- **Çalıştırmanın sonu**: *Done* ya da nedeniyle birlikte *Stopped: …*, her
  biri sandbox'ın reddettiği işlemlerin sayısıyla; ya da *The run failed: …*.
  Sizin istediğiniz bir durdurma, başarısızlık olarak değil durdurma olarak
  görünür. Satırın altındaki *What the sandbox refused* reddedilenlerin
  listesini açar: aynı ret bir kez, kaç kez olduğuyla (`×3`), en yeni 20'si,
  ardından *… and N more refusals in the Timeline*. Linux'ta satır, Linux'un
  sandbox'ın reddettiklerini Kule'ye söylemediğini ve her reddin komuta bir
  hata olarak ulaştığını belirtir. Kule bir reddin bir aracın çalışmasını neden engellediğini biliyorsa, bunu o
  satırın altında söyler. Örneğin ekran isteyen bir program (`--headless`
  ile bile LibreOffice) bir görevde çalışamaz ve macOS onun beklenmedik
  şekilde kapandığını bildirebilir. Ajana da aynı şey, bir sonraki
  prompt'unda, Kule'nin sözleriyle, bir kez söylenir. macOS'ta gri bir
  satır, çalıştırma sırasında sistem günlüğünün çekirdek mesajlarını ne
  zaman düşürdüğünü söyler; yoğun bir Mac bunu birkaç saniyede bir yapar. O
  anda yapılmış bir ret listede yoktur.

Daemon bir görevin son olaylarını saklar; böylece daha sonra açılan bir
pencere bu arada olanları gösterir. Daha eski olaylar bırakıldıysa o pencere
*N earlier events of this task's runs are no longer kept (Kule keeps the
latest 2000, up to 32 MiB)* ile başlar (görevin çalıştırmalarının N eski
olayı artık saklanmıyor; Kule en son 2000 tanesini, en çok 32 MiB'a kadar
saklar).

:::tip Seçince kopyala

Live'da fareyle seçtiğiniz metin hemen panoya gider ve işaretçinin yanında
küçük bir *Copied* görünür. Bunu *Settings → Appearance → Copying* altında
kapatabilirsiniz.

:::

## Sorular {#questions}

Ajanın programı, [politikanın](../concepts/policy.md) kendiliğinden
geçirmediği bir host'a (sunucu adına) bağlandığında, bağlantı **bekler** ve
Live'da bir kart görünür. Görev ayrıca Tasks panelinde *Needs you* altına
taşınır ve pencere öndeyken değilse sistem bildirimleri size haber verir.

Kart host'u ve port'u, bağlantıyı bekleten kuralı ve bağlantının ne zaman
reddedileceğini gösteren bir geri sayımı içerir. Onaylamanın neyi
geçireceğini de söyler: Kule'nin içine bakmadığı bir tünel (yalnızca TLS
hello'sunun bu host'u istediğini denetler, istemiyorsa keser) ya da Kule'nin
içine baktığı bir bağlantı (orada doldurduğu gizli değerlerle ya da kayıt
deposunun yalnızca okunabildiği bilgisiyle).

Yanıtlandıktan sonra kart çalıştırmada bir satıra dönüşür: "You approved
api.example:443". Kule ardından bağlantıyı keserse, satır bunu nedeniyle
birlikte, çalıştırma sürerken söyler ve bir bildirim size haber verir.
Kule'nin sizin yanıtınız olmadan reddettiği bir soru (kimse zamanında
yanıtlamadı, çalıştırma bitti, çok fazla soru bekliyordu), bunu Kule'nin
yaptığını söyleyen ayrı bir satırdır.

| Düğme | Tuşlar | Ne yapar |
| --- | --- | --- |
| **Approve** | Mod+Shift+Y | Yalnızca bu bağlantının geçmesine izin verir. Kart göründükten sonra kısa bir an (0,6 sn) Approve ne tıklama ne tuş alır; böylece imlecinizin altında beliren bir kart yanlışlıkla onaylanmaz. |
| **Approve and remember** | varsayılan yok | Bu host'a ve port'a bu klasörde bundan sonra sorulmadan izin verir. *Settings → Rules*, Kule'ye hatırlamasını söylediklerinizi listeler ve bir kuralı unutturur. |
| **Deny** | Mod+Shift+N | Bağlantıyı reddeder. Ajan onsuz devam eder. |

Bir sorunun ne kadar bekleyeceğini prompt'un altındaki **waits** ayarı
belirler (siz değiştirmedikçe 15 dakika; 1 dakikadan 8 saate kadar;
kapatılamaz). Bkz. [Ağ ve sorular](../concepts/network-and-questions.md).

## Ölçüm çubuğu {#the-measure-bar}

Bir çalıştırma sürerken, prompt'un altındaki çubuk, çalıştırmanın
bilgisayardan ne kadar kullandığını görevin
[sınırlarına](../concepts/budgets-and-limits.md) göre, iki saniyede bir
güncelleyerek gösterir. Örneğin:

```
CPU 140% / 200% · 1.2 GB / 4 GB · disk +350 MB · 7 processes
```

- Bir sınıra yaklaşınca **kehribar** rengine döner.
- İşlemci sınırı çalıştırmayı yavaşlattığı sürece **slowed** ekler.
- Görevin sınırlarını değiştirmek için **üzerine tıklayın**.

Bir sınır bir çalıştırmayı durdurduğunda, Live hangi sınırın olduğunu ve ne
ölçtüğünü söyler; örneğin "Stopped: the run's processes held 4.2 GB, over
its memory limit of 4 GB". O görev ekrandaki görev değilse bir bildirim bunu
haber verir.

## Stop {#stop}

Bir çalıştırma sürerken **Stop** (Mod+Shift+K), Run'ın yerini alır. Ajanın
sırasını (turn), ajanın kendisinde yapacağınız gibi iptal eder. Düğme,
çalıştırma bitene kadar *Stopping…* yazar; sırasını on saniye içinde
bitirmeyen bir ajan öldürülür. Şimdiye kadar değiştirdiği her şey, sizin
incelemeniz için kopyada kalır.

## Çalıştırmadan sonra {#after-the-run}

Live çalıştırmanın bittiğini söylediğinde, neyin değiştiğini görmek ve
commit etmek ya da değişiklikleri atmak (roll back) için **Review**'u
(Mod+2) açın. Bkz. [Review](review.md).

## Terminalden {#from-a-terminal}

[`kule run`](../cli/run.md), bir ajana terminalden prompt verir; aynı
bütçelere ve sınırlara tabidir. [`kule steps`](../cli/steps.md), her araç
çağrısının neyi değiştirdiğini listeler. Terminalde sorulacak kimse yoktur;
bu yüzden politikanın bir kişiye sorduğu bir bağlantı orada hemen
reddedilir.

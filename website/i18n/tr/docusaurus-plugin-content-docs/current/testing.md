---
title: Kule'yi test etmeye yardım edin
sidebar_label: Test etmeye yardım edin
description: Kule'yi macOS'ta ya da Linux'ta denemek için senaryolar, her birinde nelere bakılacağı ve bulduklarınızı gizli bilgi paylaşmadan nasıl bildireceğiniz.
---

# Kule'yi test etmeye yardım edin

Kule bir önizleme sürümü. Şu an en çok ihtiyacı olan şey, insanların onu
kendi bilgisayarlarında, kendi işlerinde kullanması. Her türlü bildirim işe
yarar: bir hata, anlaşılmayan bir adım, işinizi engelleyen bir ret ve her
şeyin yolunda gittiği bir deneme de.

Bu sayfada altı senaryo var. Hangisini isterseniz seçin; her biri 5 ile 20
dakika sürer. Her birinin sonunda kendinize tek bir soru sorun:
**beklemediğiniz ne oldu?**

## Başlamadan önce

- **Platformlar:** macOS 13 ya da sonrası (Apple silicon ya da Intel) ve
  Linux x86_64. Windows henüz teste hazır değil.
- **En son önizlemeyi kullanın.** Düzeltmeler eski sürümlere taşınmaz;
  eski bir sürümde bulduğunuz bir sorun düzelmiş olabilir. Hangi sürümde
  olduğunuzu `kule --version` söyler.
- **Klasörünüzü dikkatle seçin.** Kule siz commit edene kadar klasörünüzü
  değiştirmez, ama bu bir önizleme. Bir test klasörüyle ya da bütün işi
  commit edilip push edilmiş bir projeyle başlayın.
- **Ajanlar kendi hesaplarınızı kullanır.** Bir çalıştırma, Kule olmadan
  olacağı gibi, ajanınızın kendi kotasından ya da API kredisinden harcar.

## 1. Kurun ve kontrol edin

[Kurulum](getting-started/install.md) sayfasını izleyin: sürümler
sayfasından `.dmg`, `.deb` ya da `.rpm`, ya da kaynak koddan derleme.
Sonra şunları çalıştırın:

```sh
kule --version
kule agents list
kule isolation
```

**Bakın:** sayfanın açıklamadığı bir adım, sayfadakinden farklı çalışan bir
komut, başarısız olan bir checksum ya da attestation kontrolü, macOS'un
uygulamayı açmayı reddetmesi, Linux'ta `kule isolation`'ın eksik bir şey
listelemesi.

## 2. Ajansız döngü

[Terminalde hızlı başlangıç](getting-started/quick-start.md) sayfasının 1–8.
ve 10. adımlarını izleyin. Hiçbiri ajana ya da ağa ihtiyaç duymaz.

**Bakın:** sayfada gösterilenden farklı bir çıktı, `kule status`'ta
görünmeyen bir değişiklik, `kule undo`'nun klasörü tam olarak geri
getirmemesi.

## 3. Uygulama, bir ajanla

Bir ajanda (Claude Code, Codex ya da Gemini) oturum açın ve
[Uygulamada ilk göreviniz](getting-started/first-task-in-the-app.md)
sayfasını izleyin.

**Bakın:** terminalde çalıştığı hâlde eksik görünen bir ajan, anlamadığınız
bir soru kartı, Review'ın bir değişikliği yanlış adlandırması (fonksiyon,
bölüm ya da anahtar), Commit ya da Undo'nun düğmede yazanı yapmaması.

## 4. Kendi işiniz

Kendi projelerinizden birinde bir görev açın ve ajandan zaten isteyeceğiniz
bir şeyi isteyin: küçük bir düzeltme, bir test, bir refactor, bir bağımlılık
güncellemesi.

**Bakın:**

- politikanın size çok sık sorması ya da görmek isteyeceğiniz bir şeyi
  kendiliğinden geçirmesi;
- işinizin ihtiyaç duyduğu bir aracı, komutu ya da sunucuyu Kule'nin
  reddetmesi ve reddin nedenini söyleyip söylemediği;
- görevin kopyasının ne kadar sürede oluştuğu ve ne kadar yer kapladığı;
- ajanın yapıp Kule'nin size göstermediği herhangi bir şey.

## 5. Kendi kurulumunuz

Kule birkaç makinede test ediliyor. Sizinki büyük ihtimalle farklı, asıl
mesele de bu. Şunlarda nasıl gittiğini anlatın:

- Ubuntu dışında bir Linux dağıtımı, ya da kullanıcı namespace kısıtıyla
  Ubuntu 24.04;
- APFS, Btrfs ya da XFS dışında bir dosya sistemi;
- büyük bir depo ya da bir monorepo;
- Intel bir Mac;
- ACP konuşan başka bir ajan (bkz. [Ajanlar](using-agents.md)).

## 6. Sınırlar

Kule'nin sözü, bir ajanın görevin dışına uzanamayacağıdır: kopyasının
dışına, politikanın etrafından, gizli bilgilerinize ya da politikanın
reddettiği bir sunucuya. Bu sınırları [Güvenlik](security.md), her
platformun henüz neyi yapmadığını [Platformlar](platforms.md) listeler.

Bunlardan birini aşmanın bir yolunu bulursanız **issue açmayın**:
[SECURITY.md](https://github.com/kulehq/kule/blob/main/SECURITY.md)'nin
dediği gibi gizlice bildirin. Düzeltme gizli yapılır ve önce yayımlanır;
güvenlik bildirisi, istemezseniz dışında, sizin adınızı anar.

## Bulduklarınızı bildirmek

[github.com/kulehq/kule/issues/new/choose](https://github.com/kulehq/kule/issues/new/choose)
adresinde bir issue açın ve birini seçin. Formlar İngilizcedir; Türkçe
yazabilirsiniz.

| Form | Ne için |
| --- | --- |
| **Bug report** | Bir şey bozuldu ya da belgelerde yazanı yapmadı. |
| **Test report** | Bir senaryonun nasıl gittiği: ne belirsizdi, ne yavaştı, ne engel oldu. "Her şey çalıştı" da memnuniyetle karşılanır. |
| **Idea or change** | Kule'nin yapması ya da farklı yapması gereken bir şey. |

Sorular ve uzun sohbetler için
[Discussions](https://github.com/kulehq/kule/discussions) da açık.

Formlar en çok işe yarayanı sorar: sürüm, platform, ajan, adımlar ve
`kule isolation` çıktısı.

### Gizli bilgilerinizi dışarıda tutun

Issue'lar herkese açıktır. Bir şey yapıştırmadan önce:

- API anahtarlarını, token'ları, parolaları ve çerezleri silin;
- yayımlamayacağınız şeyleri silin: özel yollar, sunucu adları, dosya
  içerikleri, işinizle ilgili prompt'lar;
- daemon'un logundan (`~/.kule/kuled.log`) dosyanın tamamını değil, yalnızca
  sorunun etrafındaki satırları yapıştırın.

Bunlar olmadan daha kısa bir bildirim de iyi bir bildirimdir.

## Sonra ne olur

Her bildirim okunur. Düzeltme bir sonraki önizlemeyle çıkar ve issue'su
hangisi olduğunu söyler. Her önizlemenin neyi değiştirdiğini
[CHANGELOG](https://github.com/kulehq/kule/blob/main/CHANGELOG.md) listeler.
